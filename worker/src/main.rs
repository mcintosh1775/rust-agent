use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use std::{env, time::Duration};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use worker::{process_once, WorkerConfig, WorkerCycleOutcome};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await
        .context("failed to connect worker to Postgres")?;

    let config = WorkerConfig::from_env()?;
    let signer_identity = config.nostr_signer.resolve_identity()?;
    info!(
        worker_id = %config.worker_id,
        lease_for_secs = config.lease_for.as_secs(),
        requeue_limit = config.requeue_limit,
        poll_ms = config.poll_interval.as_millis(),
        skill_env_allowlist_count = config.skill_env_allowlist.len(),
        llm_mode = config.llm.mode.as_str(),
        llm_local_configured = config.llm.local.is_some(),
        llm_remote_configured = config.llm.remote.is_some(),
        llm_remote_egress_enabled = config.llm.remote_egress_enabled,
        llm_remote_allowlist_count = config.llm.remote_host_allowlist.len(),
        llm_remote_token_budget_per_run = config.llm.remote_token_budget_per_run,
        llm_remote_cost_per_1k_tokens_usd = config.llm.remote_cost_per_1k_tokens_usd,
        local_exec_enabled = config.local_exec.enabled,
        local_exec_read_roots = config.local_exec.read_roots.len(),
        local_exec_write_roots = config.local_exec.write_roots.len(),
        slack_webhook_configured = config.slack_webhook_url.is_some(),
        slack_send_timeout_ms = config.slack_send_timeout.as_millis(),
        slack_max_attempts = config.slack_max_attempts,
        slack_retry_backoff_ms = config.slack_retry_backoff.as_millis(),
        payment_nwc_enabled = config.payment_nwc_enabled,
        payment_nwc_uri_configured = config.payment_nwc_uri.as_deref().map(str::trim).is_some_and(|v| !v.is_empty()),
        payment_nwc_wallet_uri_count = config.payment_nwc_wallet_uris.len(),
        payment_nwc_wallet_default_configured = config.payment_nwc_wallet_uris.contains_key("*"),
        payment_nwc_timeout_ms = config.payment_nwc_timeout.as_millis(),
        payment_nwc_route_strategy = config.payment_nwc_route_strategy.as_str(),
        payment_nwc_route_fallback_enabled = config.payment_nwc_route_fallback_enabled,
        payment_nwc_mock_balance_msat = config.payment_nwc_mock_balance_msat,
        payment_max_spend_msat_per_run = ?config.payment_max_spend_msat_per_run,
        payment_approval_threshold_msat = ?config.payment_approval_threshold_msat,
        payment_max_spend_msat_per_tenant = ?config.payment_max_spend_msat_per_tenant,
        payment_max_spend_msat_per_agent = ?config.payment_max_spend_msat_per_agent,
        trigger_scheduler_enabled = config.trigger_scheduler_enabled,
        trigger_tenant_max_inflight_runs = config.trigger_tenant_max_inflight_runs,
        trigger_scheduler_lease_enabled = config.trigger_scheduler_lease_enabled,
        trigger_scheduler_lease_name = %config.trigger_scheduler_lease_name,
        trigger_scheduler_lease_ttl_ms = config.trigger_scheduler_lease_ttl.as_millis(),
        nostr_signer_mode = config.nostr_signer.mode.as_str(),
        nostr_relay_count = config.nostr_relays.len(),
        nostr_publish_timeout_ms = config.nostr_publish_timeout.as_millis(),
        "worker started"
    );
    if let Some(identity) = signer_identity {
        info!(
            nostr_signer_mode = identity.mode.as_str(),
            nostr_public_key = %identity.public_key,
            "nostr signer configured"
        );
    } else {
        warn!(
            nostr_signer_mode = config.nostr_signer.mode.as_str(),
            "nostr signer not configured; Nostr signing is disabled for this worker"
        );
    }

    run_forever(&pool, &config).await
}

async fn run_forever(pool: &sqlx::PgPool, config: &WorkerConfig) -> Result<()> {
    loop {
        match process_once(pool, config).await? {
            WorkerCycleOutcome::ClaimedAndSucceeded { run_id } => {
                info!(%run_id, "worker processed run successfully");
                continue;
            }
            WorkerCycleOutcome::ClaimedAndFailed { run_id } => {
                warn!(%run_id, "worker processed run with failure");
                continue;
            }
            WorkerCycleOutcome::Idle {
                requeued_expired_runs,
            } => {
                if requeued_expired_runs > 0 {
                    warn!(requeued_expired_runs, "requeued expired runs");
                }
            }
        }

        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed waiting for ctrl-c")?;
                info!("worker shutdown signal received");
                break;
            }
            _ = tokio::time::sleep(config.poll_interval) => {}
        }
    }

    // Small grace period for any inflight logging flush.
    tokio::time::sleep(Duration::from_millis(25)).await;
    Ok(())
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
