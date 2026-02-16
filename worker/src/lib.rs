use agent_core::{
    append_audit_event, claim_next_queued_run, mark_run_failed, mark_run_succeeded,
    renew_run_lease, requeue_expired_runs, NewAuditEvent,
};
use anyhow::{Context, Result};
use core as agent_core;
use serde_json::json;
use sqlx::PgPool;
use std::{env, time::Duration};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub worker_id: String,
    pub lease_for: Duration,
    pub requeue_limit: i64,
    pub poll_interval: Duration,
}

impl WorkerConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            worker_id: env::var("WORKER_ID")
                .unwrap_or_else(|_| format!("worker-{}", Uuid::new_v4())),
            lease_for: Duration::from_secs(read_env_u64("WORKER_LEASE_SECS", 30)?),
            requeue_limit: read_env_i64("WORKER_REQUEUE_LIMIT", 100)?,
            poll_interval: Duration::from_millis(read_env_u64("WORKER_POLL_MS", 750)?),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerCycleOutcome {
    ClaimedAndCompleted { run_id: Uuid },
    Idle { requeued_expired_runs: u64 },
}

pub async fn process_once(pool: &PgPool, config: &WorkerConfig) -> Result<WorkerCycleOutcome> {
    let requeued_expired_runs = requeue_expired_runs(pool, config.requeue_limit).await?;

    let Some(claimed_run) =
        claim_next_queued_run(pool, &config.worker_id, config.lease_for).await?
    else {
        return Ok(WorkerCycleOutcome::Idle {
            requeued_expired_runs,
        });
    };

    append_audit_event(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: claimed_run.id,
            step_id: None,
            tenant_id: claimed_run.tenant_id.clone(),
            agent_id: Some(claimed_run.agent_id),
            user_id: claimed_run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "run.claimed".to_string(),
            payload_json: json!({
                "attempts": claimed_run.attempts,
                "lease_owner": claimed_run.lease_owner,
                "lease_expires_at": claimed_run.lease_expires_at,
            }),
        },
    )
    .await?;

    let renewed =
        renew_run_lease(pool, claimed_run.id, &config.worker_id, config.lease_for).await?;
    if !renewed {
        append_audit_event(
            pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id: claimed_run.id,
                step_id: None,
                tenant_id: claimed_run.tenant_id.clone(),
                agent_id: Some(claimed_run.agent_id),
                user_id: claimed_run.triggered_by_user_id,
                actor: format!("worker:{}", config.worker_id),
                event_type: "run.lease_renew_failed".to_string(),
                payload_json: json!({}),
            },
        )
        .await?;

        let _ = mark_run_failed(
            pool,
            claimed_run.id,
            &config.worker_id,
            json!({
                "code": "LEASE_RENEW_FAILED",
                "message": "worker failed to renew run lease after claim"
            }),
        )
        .await?;

        return Ok(WorkerCycleOutcome::Idle {
            requeued_expired_runs,
        });
    }

    append_audit_event(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: claimed_run.id,
            step_id: None,
            tenant_id: claimed_run.tenant_id.clone(),
            agent_id: Some(claimed_run.agent_id),
            user_id: claimed_run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "run.processing_started".to_string(),
            payload_json: json!({
                "recipe_id": claimed_run.recipe_id,
                "attempts": claimed_run.attempts,
            }),
        },
    )
    .await?;

    let completed = mark_run_succeeded(pool, claimed_run.id, &config.worker_id).await?;
    if completed {
        append_audit_event(
            pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id: claimed_run.id,
                step_id: None,
                tenant_id: claimed_run.tenant_id,
                agent_id: Some(claimed_run.agent_id),
                user_id: claimed_run.triggered_by_user_id,
                actor: format!("worker:{}", config.worker_id),
                event_type: "run.completed".to_string(),
                payload_json: json!({
                    "status": "succeeded",
                    "attempts": claimed_run.attempts,
                }),
            },
        )
        .await?;

        Ok(WorkerCycleOutcome::ClaimedAndCompleted {
            run_id: claimed_run.id,
        })
    } else {
        mark_run_failed(
            pool,
            claimed_run.id,
            &config.worker_id,
            json!({
                "code": "RUN_FINALIZE_FAILED",
                "message": "worker could not mark run as succeeded"
            }),
        )
        .await?;

        Ok(WorkerCycleOutcome::Idle {
            requeued_expired_runs,
        })
    }
}

fn read_env_u64(key: &str, default: u64) -> Result<u64> {
    match env::var(key) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("invalid integer for {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn read_env_i64(key: &str, default: i64) -> Result<i64> {
    match env::var(key) {
        Ok(value) => value
            .parse::<i64>()
            .with_context(|| format!("invalid integer for {key}: {value}")),
        Err(_) => Ok(default),
    }
}
