use serde_json::Value;
use sqlx::{PgPool, Row};
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewRun {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub input_json: Value,
    pub requested_capabilities: Value,
    pub granted_capabilities: Value,
    pub status: String,
    pub error_json: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub status: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewStep {
    pub id: Uuid,
    pub run_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub user_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub input_json: Value,
    pub error_json: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct StepRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub user_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewActionRequest {
    pub id: Uuid,
    pub step_id: Uuid,
    pub action_type: String,
    pub args_json: Value,
    pub justification: Option<String>,
    pub status: String,
    pub decision_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActionRequestRecord {
    pub id: Uuid,
    pub step_id: Uuid,
    pub action_type: String,
    pub status: String,
    pub decision_reason: Option<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewActionResult {
    pub id: Uuid,
    pub action_request_id: Uuid,
    pub status: String,
    pub result_json: Option<Value>,
    pub error_json: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ActionResultRecord {
    pub id: Uuid,
    pub action_request_id: Uuid,
    pub status: String,
    pub executed_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewAuditEvent {
    pub id: Uuid,
    pub run_id: Uuid,
    pub step_id: Option<Uuid>,
    pub tenant_id: String,
    pub agent_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub actor: String,
    pub event_type: String,
    pub payload_json: Value,
}

#[derive(Debug, Clone)]
pub struct AuditEventRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub step_id: Option<Uuid>,
    pub actor: String,
    pub event_type: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct AuditEventDetailRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub step_id: Option<Uuid>,
    pub actor: String,
    pub event_type: String,
    pub payload_json: Value,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewArtifact {
    pub id: Uuid,
    pub run_id: Uuid,
    pub path: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub checksum: Option<String>,
    pub storage_ref: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub path: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub storage_ref: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct RunLeaseRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub status: String,
    pub input_json: Value,
    pub granted_capabilities: Value,
    pub attempts: i32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct RunStatusRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub status: String,
    pub requested_capabilities: Value,
    pub granted_capabilities: Value,
    pub created_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
    pub error_json: Option<Value>,
    pub attempts: i32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<OffsetDateTime>,
}

pub async fn create_run(pool: &PgPool, new_run: &NewRun) -> Result<RunRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO runs (
            id,
            tenant_id,
            agent_id,
            triggered_by_user_id,
            recipe_id,
            status,
            input_json,
            requested_capabilities,
            granted_capabilities,
            error_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, created_at
        "#,
    )
    .bind(new_run.id)
    .bind(&new_run.tenant_id)
    .bind(new_run.agent_id)
    .bind(new_run.triggered_by_user_id)
    .bind(&new_run.recipe_id)
    .bind(&new_run.status)
    .bind(&new_run.input_json)
    .bind(&new_run.requested_capabilities)
    .bind(&new_run.granted_capabilities)
    .bind(&new_run.error_json)
    .fetch_one(pool)
    .await?;

    Ok(RunRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        triggered_by_user_id: row.get("triggered_by_user_id"),
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    })
}

pub async fn get_run_status(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Uuid,
) -> Result<Option<RunStatusRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               agent_id,
               triggered_by_user_id,
               recipe_id,
               status,
               requested_capabilities,
               granted_capabilities,
               created_at,
               started_at,
               finished_at,
               error_json,
               attempts,
               lease_owner,
               lease_expires_at
        FROM runs
        WHERE tenant_id = $1
          AND id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| RunStatusRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        triggered_by_user_id: row.get("triggered_by_user_id"),
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        requested_capabilities: row.get("requested_capabilities"),
        granted_capabilities: row.get("granted_capabilities"),
        created_at: row.get("created_at"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        error_json: row.get("error_json"),
        attempts: row.get("attempts"),
        lease_owner: row.get("lease_owner"),
        lease_expires_at: row.get("lease_expires_at"),
    }))
}

pub async fn create_step(pool: &PgPool, new_step: &NewStep) -> Result<StepRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO steps (
            id,
            run_id,
            tenant_id,
            agent_id,
            user_id,
            name,
            status,
            input_json,
            error_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, run_id, tenant_id, agent_id, user_id, name, status, created_at
        "#,
    )
    .bind(new_step.id)
    .bind(new_step.run_id)
    .bind(&new_step.tenant_id)
    .bind(new_step.agent_id)
    .bind(new_step.user_id)
    .bind(&new_step.name)
    .bind(&new_step.status)
    .bind(&new_step.input_json)
    .bind(&new_step.error_json)
    .fetch_one(pool)
    .await?;

    Ok(StepRecord {
        id: row.get("id"),
        run_id: row.get("run_id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        user_id: row.get("user_id"),
        name: row.get("name"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    })
}

pub async fn mark_step_succeeded(
    pool: &PgPool,
    step_id: Uuid,
    output_json: Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE steps
        SET status = 'succeeded',
            output_json = $2,
            finished_at = now()
        WHERE id = $1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(step_id)
    .bind(output_json)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn mark_step_failed(
    pool: &PgPool,
    step_id: Uuid,
    error_json: Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE steps
        SET status = 'failed',
            error_json = $2,
            finished_at = now()
        WHERE id = $1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(step_id)
    .bind(error_json)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn create_action_request(
    pool: &PgPool,
    new_request: &NewActionRequest,
) -> Result<ActionRequestRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO action_requests (
            id,
            step_id,
            action_type,
            args_json,
            justification,
            status,
            decision_reason
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, step_id, action_type, status, decision_reason, created_at
        "#,
    )
    .bind(new_request.id)
    .bind(new_request.step_id)
    .bind(&new_request.action_type)
    .bind(&new_request.args_json)
    .bind(&new_request.justification)
    .bind(&new_request.status)
    .bind(&new_request.decision_reason)
    .fetch_one(pool)
    .await?;

    Ok(ActionRequestRecord {
        id: row.get("id"),
        step_id: row.get("step_id"),
        action_type: row.get("action_type"),
        status: row.get("status"),
        decision_reason: row.get("decision_reason"),
        created_at: row.get("created_at"),
    })
}

pub async fn update_action_request_status(
    pool: &PgPool,
    action_request_id: Uuid,
    status: &str,
    decision_reason: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE action_requests
        SET status = $2,
            decision_reason = $3
        WHERE id = $1
        "#,
    )
    .bind(action_request_id)
    .bind(status)
    .bind(decision_reason)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn create_action_result(
    pool: &PgPool,
    new_result: &NewActionResult,
) -> Result<ActionResultRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO action_results (
            id,
            action_request_id,
            status,
            result_json,
            error_json
        )
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (action_request_id) DO UPDATE
            SET status = EXCLUDED.status,
                result_json = EXCLUDED.result_json,
                error_json = EXCLUDED.error_json,
                executed_at = now()
        RETURNING id, action_request_id, status, executed_at
        "#,
    )
    .bind(new_result.id)
    .bind(new_result.action_request_id)
    .bind(&new_result.status)
    .bind(&new_result.result_json)
    .bind(&new_result.error_json)
    .fetch_one(pool)
    .await?;

    Ok(ActionResultRecord {
        id: row.get("id"),
        action_request_id: row.get("action_request_id"),
        status: row.get("status"),
        executed_at: row.get("executed_at"),
    })
}

pub async fn append_audit_event(
    pool: &PgPool,
    new_event: &NewAuditEvent,
) -> Result<AuditEventRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO audit_events (
            id,
            run_id,
            step_id,
            tenant_id,
            agent_id,
            user_id,
            actor,
            event_type,
            payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, run_id, step_id, actor, event_type, created_at
        "#,
    )
    .bind(new_event.id)
    .bind(new_event.run_id)
    .bind(new_event.step_id)
    .bind(&new_event.tenant_id)
    .bind(new_event.agent_id)
    .bind(new_event.user_id)
    .bind(&new_event.actor)
    .bind(&new_event.event_type)
    .bind(&new_event.payload_json)
    .fetch_one(pool)
    .await?;

    Ok(AuditEventRecord {
        id: row.get("id"),
        run_id: row.get("run_id"),
        step_id: row.get("step_id"),
        actor: row.get("actor"),
        event_type: row.get("event_type"),
        created_at: row.get("created_at"),
    })
}

pub async fn list_run_audit_events(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Uuid,
    limit: i64,
) -> Result<Vec<AuditEventDetailRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, run_id, step_id, actor, event_type, payload_json, created_at
        FROM audit_events
        WHERE tenant_id = $1
          AND run_id = $2
        ORDER BY created_at ASC, id ASC
        LIMIT $3
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| AuditEventDetailRecord {
            id: row.get("id"),
            run_id: row.get("run_id"),
            step_id: row.get("step_id"),
            actor: row.get("actor"),
            event_type: row.get("event_type"),
            payload_json: row.get("payload_json"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn persist_artifact_metadata(
    pool: &PgPool,
    new_artifact: &NewArtifact,
) -> Result<ArtifactRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO artifacts (
            id,
            run_id,
            path,
            content_type,
            size_bytes,
            checksum,
            storage_ref
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, run_id, path, content_type, size_bytes, storage_ref, created_at
        "#,
    )
    .bind(new_artifact.id)
    .bind(new_artifact.run_id)
    .bind(&new_artifact.path)
    .bind(&new_artifact.content_type)
    .bind(new_artifact.size_bytes)
    .bind(&new_artifact.checksum)
    .bind(&new_artifact.storage_ref)
    .fetch_one(pool)
    .await?;

    Ok(ArtifactRecord {
        id: row.get("id"),
        run_id: row.get("run_id"),
        path: row.get("path"),
        content_type: row.get("content_type"),
        size_bytes: row.get("size_bytes"),
        storage_ref: row.get("storage_ref"),
        created_at: row.get("created_at"),
    })
}

pub async fn claim_next_queued_run(
    pool: &PgPool,
    worker_id: &str,
    lease_for: Duration,
) -> Result<Option<RunLeaseRecord>, sqlx::Error> {
    let lease_ms = clamp_lease_ms(lease_for);
    let row = sqlx::query(
        r#"
        WITH candidate AS (
            SELECT id
            FROM runs
            WHERE status = 'queued'
              AND (lease_expires_at IS NULL OR lease_expires_at < now())
            ORDER BY created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        UPDATE runs
        SET status = 'running',
            started_at = COALESCE(started_at, now()),
            attempts = attempts + 1,
            lease_owner = $1,
            lease_expires_at = now() + ($2::bigint * interval '1 millisecond')
        WHERE id IN (SELECT id FROM candidate)
        RETURNING id,
                  tenant_id,
                  agent_id,
                  triggered_by_user_id,
                  recipe_id,
                  status,
                  input_json,
                  granted_capabilities,
                  attempts,
                  lease_owner,
                  lease_expires_at,
                  created_at,
                  started_at
        "#,
    )
    .bind(worker_id)
    .bind(lease_ms)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| RunLeaseRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        triggered_by_user_id: row.get("triggered_by_user_id"),
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        input_json: row.get("input_json"),
        granted_capabilities: row.get("granted_capabilities"),
        attempts: row.get("attempts"),
        lease_owner: row.get("lease_owner"),
        lease_expires_at: row.get("lease_expires_at"),
        created_at: row.get("created_at"),
        started_at: row.get("started_at"),
    }))
}

pub async fn renew_run_lease(
    pool: &PgPool,
    run_id: Uuid,
    worker_id: &str,
    lease_for: Duration,
) -> Result<bool, sqlx::Error> {
    let lease_ms = clamp_lease_ms(lease_for);
    let result = sqlx::query(
        r#"
        UPDATE runs
        SET lease_expires_at = now() + ($3::bigint * interval '1 millisecond')
        WHERE id = $1
          AND status = 'running'
          AND lease_owner = $2
          AND lease_expires_at IS NOT NULL
          AND lease_expires_at > now()
        "#,
    )
    .bind(run_id)
    .bind(worker_id)
    .bind(lease_ms)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn mark_run_succeeded(
    pool: &PgPool,
    run_id: Uuid,
    worker_id: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE runs
        SET status = 'succeeded',
            finished_at = now(),
            lease_owner = NULL,
            lease_expires_at = NULL
        WHERE id = $1
          AND status = 'running'
          AND lease_owner = $2
        "#,
    )
    .bind(run_id)
    .bind(worker_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn mark_run_failed(
    pool: &PgPool,
    run_id: Uuid,
    worker_id: &str,
    error_json: Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE runs
        SET status = 'failed',
            finished_at = now(),
            error_json = $3,
            lease_owner = NULL,
            lease_expires_at = NULL
        WHERE id = $1
          AND status = 'running'
          AND lease_owner = $2
        "#,
    )
    .bind(run_id)
    .bind(worker_id)
    .bind(error_json)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn requeue_expired_runs(pool: &PgPool, limit: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        WITH expired AS (
            SELECT id
            FROM runs
            WHERE status = 'running'
              AND lease_expires_at IS NOT NULL
              AND lease_expires_at < now()
            ORDER BY lease_expires_at
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE runs
        SET status = 'queued',
            lease_owner = NULL,
            lease_expires_at = NULL
        WHERE id IN (SELECT id FROM expired)
        "#,
    )
    .bind(limit.max(0))
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

fn clamp_lease_ms(lease_for: Duration) -> i64 {
    lease_for.as_millis().clamp(1, i64::MAX as u128) as i64
}
