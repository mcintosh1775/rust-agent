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
        skill_script_sha256_configured = config.skill_script_sha256.is_some(),
        approval_required_action_type_count = config.approval_required_action_types.len(),
        local_exec_enabled = config.local_exec.enabled,
        local_exec_read_roots = config.local_exec.read_roots.len(),
        local_exec_write_roots = config.local_exec.write_roots.len(),
        slack_webhook_configured = config.slack_webhook_url.is_some(),
        slack_send_timeout_ms = config.slack_send_timeout.as_millis(),
        slack_max_attempts = config.slack_max_attempts,
        slack_retry_backoff_ms = config.slack_retry_backoff.as_millis(),
        message_whitenoise_dest_allowlist_count = config.message_whitenoise_destination_allowlist.len(),
        message_slack_dest_allowlist_count = config.message_slack_destination_allowlist.len(),
        nostr_signer_mode = config.nostr_signer.mode.as_str(),
        nostr_relay_count = config.nostr_relays.len(),
        nostr_publish_timeout_ms = config.nostr_publish_timeout.as_millis(),
        "worker started: runtime baseline"
    );

    info!(
        llm_mode = config.llm.mode.as_str(),
        llm_max_input_bytes = config.llm.max_input_bytes,
        llm_local_configured = config.llm.local.is_some(),
        llm_remote_configured = config.llm.remote.is_some(),
        llm_large_input_threshold_bytes = config.llm.large_input_threshold_bytes,
        llm_large_input_policy = config.llm.large_input_policy.as_str(),
        llm_large_input_summary_target_bytes = config.llm.large_input_summary_target_bytes,
        llm_context_retrieval_top_k = config.llm.context_retrieval_top_k,
        llm_context_retrieval_max_bytes = config.llm.context_retrieval_max_bytes,
        llm_context_retrieval_chunk_bytes = config.llm.context_retrieval_chunk_bytes,
        llm_admission_enabled = config.llm.admission_enabled,
        llm_admission_interactive_max_inflight = config.llm.admission_interactive_max_inflight,
        llm_admission_batch_max_inflight = config.llm.admission_batch_max_inflight,
        llm_cache_enabled = config.llm.cache_enabled,
        llm_cache_ttl_secs = config.llm.cache_ttl_secs,
        llm_cache_max_entries = config.llm.cache_max_entries,
        llm_distributed_enabled = config.llm.distributed_enabled,
        llm_distributed_fail_open = config.llm.distributed_fail_open,
        llm_distributed_owner = %config.llm.distributed_owner,
        llm_distributed_admission_enabled = config.llm.distributed_admission_enabled,
        llm_distributed_admission_lease_ms = config.llm.distributed_admission_lease_ms,
        llm_distributed_cache_enabled = config.llm.distributed_cache_enabled,
        llm_distributed_cache_namespace_max_entries = config.llm.distributed_cache_namespace_max_entries,
        llm_remote_egress_enabled = config.llm.remote_egress_enabled,
        llm_remote_egress_class = config.llm.remote_egress_class.as_str(),
        llm_remote_allowlist_count = config.llm.remote_host_allowlist.len(),
        llm_remote_cost_per_1k_tokens_usd = config.llm.remote_cost_per_1k_tokens_usd,
        "worker started: llm gateway baseline"
    );

    info!(
        llm_verifier_enabled = config.llm.verifier_enabled,
        llm_verifier_mode = config.llm.verifier_mode.as_str(),
        llm_verifier_min_score_pct = config.llm.verifier_min_score_pct,
        llm_verifier_escalate_remote = config.llm.verifier_escalate_remote,
        llm_verifier_min_response_chars = config.llm.verifier_min_response_chars,
        llm_verifier_judge_configured = config.llm.verifier_judge.is_some(),
        llm_verifier_judge_timeout_ms = config.llm.verifier_judge_timeout.as_millis(),
        llm_verifier_judge_fail_open = config.llm.verifier_judge_fail_open,
        llm_slo_interactive_max_latency_ms = ?config.llm.slo_interactive_max_latency_ms,
        llm_slo_batch_max_latency_ms = ?config.llm.slo_batch_max_latency_ms,
        llm_slo_alert_threshold_pct = ?config.llm.slo_alert_threshold_pct,
        llm_slo_breach_escalate_remote = config.llm.slo_breach_escalate_remote,
        llm_remote_token_budget_per_run = config.llm.remote_token_budget_per_run,
        llm_remote_token_budget_per_tenant = config.llm.remote_token_budget_per_tenant,
        llm_remote_token_budget_per_agent = config.llm.remote_token_budget_per_agent,
        llm_remote_token_budget_per_model = config.llm.remote_token_budget_per_model,
        llm_remote_token_budget_window_secs = config.llm.remote_token_budget_window_secs,
        llm_remote_token_budget_soft_alert_threshold_pct = config
            .llm
            .remote_token_budget_soft_alert_threshold_pct,
        "worker started: llm verifier + budget + slo"
    );

    info!(
        payment_nwc_enabled = config.payment_nwc_enabled,
        payment_nwc_uri_configured = config
            .payment_nwc_uri
            .as_deref()
            .map(str::trim)
            .is_some_and(|v| !v.is_empty()),
        payment_nwc_wallet_uri_count = config.payment_nwc_wallet_uris.len(),
        payment_nwc_wallet_default_configured = config.payment_nwc_wallet_uris.contains_key("*"),
        payment_nwc_timeout_ms = config.payment_nwc_timeout.as_millis(),
        payment_nwc_route_strategy = config.payment_nwc_route_strategy.as_str(),
        payment_nwc_route_fallback_enabled = config.payment_nwc_route_fallback_enabled,
        payment_nwc_route_rollout_percent = config.payment_nwc_route_rollout_percent,
        payment_nwc_route_health_fail_threshold = config.payment_nwc_route_health_fail_threshold,
        payment_nwc_route_health_cooldown_secs = config.payment_nwc_route_health_cooldown.as_secs(),
        payment_nwc_mock_balance_msat = config.payment_nwc_mock_balance_msat,
        payment_cashu_enabled = config.payment_cashu_enabled,
        payment_cashu_mint_count = config.payment_cashu_mint_uris.len(),
        payment_cashu_default_mint = ?config.payment_cashu_default_mint,
        payment_cashu_timeout_ms = config.payment_cashu_timeout.as_millis(),
        payment_cashu_max_spend_msat_per_run = ?config.payment_cashu_max_spend_msat_per_run,
        payment_cashu_http_enabled = config.payment_cashu_http_enabled,
        payment_cashu_http_allow_insecure = config.payment_cashu_http_allow_insecure,
        payment_cashu_auth_header = %config.payment_cashu_auth_header,
        payment_cashu_auth_configured = config.payment_cashu_auth_token.is_some(),
        payment_cashu_route_strategy = config.payment_cashu_route_strategy.as_str(),
        payment_cashu_route_fallback_enabled = config.payment_cashu_route_fallback_enabled,
        payment_cashu_route_rollout_percent = config.payment_cashu_route_rollout_percent,
        payment_cashu_route_health_fail_threshold = config.payment_cashu_route_health_fail_threshold,
        payment_cashu_route_health_cooldown_secs = config.payment_cashu_route_health_cooldown.as_secs(),
        payment_cashu_mock_enabled = config.payment_cashu_mock_enabled,
        payment_cashu_mock_balance_msat = config.payment_cashu_mock_balance_msat,
        payment_max_spend_msat_per_run = ?config.payment_max_spend_msat_per_run,
        payment_approval_threshold_msat = ?config.payment_approval_threshold_msat,
        payment_max_spend_msat_per_tenant = ?config.payment_max_spend_msat_per_tenant,
        payment_max_spend_msat_per_agent = ?config.payment_max_spend_msat_per_agent,
        trigger_scheduler_enabled = config.trigger_scheduler_enabled,
        trigger_tenant_max_inflight_runs = config.trigger_tenant_max_inflight_runs,
        trigger_scheduler_lease_enabled = config.trigger_scheduler_lease_enabled,
        trigger_scheduler_lease_name = %config.trigger_scheduler_lease_name,
        trigger_scheduler_lease_ttl_ms = config.trigger_scheduler_lease_ttl.as_millis(),
        memory_compaction_enabled = config.memory_compaction_enabled,
        memory_compaction_min_records = config.memory_compaction_min_records,
        memory_compaction_max_groups_per_cycle = config.memory_compaction_max_groups_per_cycle,
        memory_compaction_min_age_secs = config.memory_compaction_min_age.as_secs(),
        agent_context_enabled = config.agent_context_enabled,
        agent_context_required = config.agent_context_required,
        agent_context_root = %config.agent_context_loader.root_dir.display(),
        agent_context_required_file_count = config.agent_context_loader.required_files.len(),
        agent_context_max_file_bytes = config.agent_context_loader.max_file_bytes,
        agent_context_max_total_bytes = config.agent_context_loader.max_total_bytes,
        agent_context_max_dynamic_files_per_dir = config
            .agent_context_loader
            .max_dynamic_files_per_dir,
        compliance_siem_delivery_enabled = config.compliance_siem_delivery_enabled,
        compliance_siem_delivery_batch_size = config.compliance_siem_delivery_batch_size,
        compliance_siem_delivery_lease_ms = config.compliance_siem_delivery_lease.as_millis(),
        compliance_siem_delivery_retry_backoff_ms =
            config.compliance_siem_delivery_retry_backoff.as_millis(),
        compliance_siem_delivery_retry_jitter_max_ms = config
            .compliance_siem_delivery_retry_jitter_max
            .as_millis(),
        compliance_siem_http_enabled = config.compliance_siem_delivery_http_enabled,
        compliance_siem_http_timeout_ms = config.compliance_siem_delivery_http_timeout.as_millis(),
        compliance_siem_http_auth_header = %config.compliance_siem_delivery_http_auth_header,
        compliance_siem_http_auth_configured = config.compliance_siem_delivery_http_auth_token.is_some(),
        "worker started: payment + scheduler + compliance"
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
