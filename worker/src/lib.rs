use agent_core::{
    append_audit_event_dual, claim_next_queued_run_with_limits_dual,
    claim_pending_compliance_siem_delivery_records_dual, compact_memory_records_dual,
    create_action_request_dual, create_action_result_dual, create_llm_token_usage_record_dual,
    create_or_get_payment_request_dual, create_payment_result_dual, create_step_dual,
    count_inflight_runs_dual, default_agent_context_required_files,
    dispatch_next_due_trigger_with_limits_dual,
    get_latest_payment_result_dual, load_agent_context_snapshot,
    mark_compliance_siem_delivery_record_dead_lettered_dual,
    mark_compliance_siem_delivery_record_delivered_dual,
    mark_compliance_siem_delivery_record_failed_dual, mark_run_failed_dual,
    mark_run_succeeded_dual, mark_step_failed_dual, mark_step_succeeded_dual,
    normalize_agent_context_required_files, persist_artifact_metadata_dual, redact_json,
    redact_text, renew_run_lease_dual, requeue_expired_runs_dual, resolve_secret_value,
    sum_executed_payment_amount_msat_for_agent_dual,
    sum_executed_payment_amount_msat_for_tenant_dual, sum_llm_consumed_tokens_for_agent_since_dual,
    sum_llm_consumed_tokens_for_model_since_dual, sum_llm_consumed_tokens_for_tenant_since_dual,
    try_acquire_scheduler_lease_dual, update_action_request_status_dual,
    update_payment_request_status_dual, ActionRequest as PolicyActionRequest,
    AgentContextLoaderConfig, CachedSecretResolver, CapabilityGrant as PolicyCapabilityGrant,
    DenyReason,
    CapabilityKind as PolicyCapabilityKind, CliSecretResolver, DbPool, GrantSet, NewActionRequest,
    NewActionResult, NewArtifact, NewAuditEvent, NewLlmTokenUsageRecord, NewPaymentRequest,
    NewPaymentResult, NewStep, PolicyDecision, SchedulerLeaseParams,
};
use anyhow::{anyhow, Context, Result};
use core as agent_core;
use nostr::nips::nip47::{
    GetBalanceResponse, MakeInvoiceRequest, PayInvoiceRequest, Request as NwcRequest,
};
use nostr::{PublicKey, SecretKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use skillrunner::{
    CapabilityGrant as SkillCapabilityGrant, InvokeContext, InvokeRequest, RunnerConfig,
    SkillRunner,
};
use sqlx::PgPool;
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use time::OffsetDateTime;
use uuid::Uuid;

pub mod llm;
pub mod local_exec;
pub mod nip46_signer;
pub mod nip47_wallet;
pub mod nostr_transport;
pub mod signer;
pub mod slack;

use llm::{execute_llm_infer, policy_scope_for_action as llm_policy_scope_for_action, LlmConfig};
use local_exec::{execute_local_exec, parse_roots_from_env, LocalExecConfig};
use nip46_signer::sign_event_with_bunker;
use nip47_wallet::send_nwc_request;
use nostr_transport::{build_text_note_unsigned, publish_signed_event, publish_text_note};
use signer::{NostrSignerConfig, NostrSignerMode};
use slack::send_webhook_message;

// --- Worker strategy and runtime configuration ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentNwcRouteStrategy {
    Ordered,
    DeterministicHash,
}

impl PaymentNwcRouteStrategy {
    fn from_env() -> Result<Self> {
        Self::from_env_var("PAYMENT_NWC_ROUTE_STRATEGY")
    }

    fn from_env_var(var_name: &str) -> Result<Self> {
        match env::var(var_name)
            .unwrap_or_else(|_| "ordered".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "ordered" => Ok(Self::Ordered),
            "deterministic_hash" | "hash" => Ok(Self::DeterministicHash),
            other => Err(anyhow!(
                "invalid {var_name} `{other}` (supported: ordered, deterministic_hash)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ordered => "ordered",
            Self::DeterministicHash => "deterministic_hash",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub worker_id: String,
    pub lease_for: Duration,
    pub requeue_limit: i64,
    pub poll_interval: Duration,
    pub skill_command: String,
    pub skill_args: Vec<String>,
    pub skill_recipe_commands: BTreeMap<String, Vec<String>>,
    pub skill_timeout: Duration,
    pub skill_max_output_bytes: usize,
    pub skill_env_allowlist: Vec<String>,
    pub skill_script_sha256: Option<String>,
    pub approval_required_action_types: Vec<String>,
    pub llm: LlmConfig,
    pub local_exec: LocalExecConfig,
    pub artifact_root: PathBuf,
    pub nostr_signer: NostrSignerConfig,
    pub nostr_relays: Vec<String>,
    pub nostr_publish_timeout: Duration,
    pub slack_webhook_url: Option<String>,
    pub slack_send_timeout: Duration,
    pub slack_max_attempts: u32,
    pub slack_retry_backoff: Duration,
    pub message_whitenoise_destination_allowlist: Vec<String>,
    pub message_slack_destination_allowlist: Vec<String>,
    pub payment_nwc_enabled: bool,
    pub payment_nwc_uri: Option<String>,
    pub payment_nwc_wallet_uris: BTreeMap<String, String>,
    pub payment_nwc_timeout: Duration,
    pub payment_nwc_route_strategy: PaymentNwcRouteStrategy,
    pub payment_nwc_route_fallback_enabled: bool,
    pub payment_nwc_route_rollout_percent: u8,
    pub payment_nwc_route_health_fail_threshold: u32,
    pub payment_nwc_route_health_cooldown: Duration,
    pub payment_nwc_mock_balance_msat: u64,
    pub payment_cashu_enabled: bool,
    pub payment_cashu_mint_uris: BTreeMap<String, String>,
    pub payment_cashu_default_mint: Option<String>,
    pub payment_cashu_timeout: Duration,
    pub payment_cashu_max_spend_msat_per_run: Option<u64>,
    pub payment_cashu_http_enabled: bool,
    pub payment_cashu_http_allow_insecure: bool,
    pub payment_cashu_auth_header: String,
    pub payment_cashu_auth_token: Option<String>,
    pub payment_cashu_route_strategy: PaymentNwcRouteStrategy,
    pub payment_cashu_route_fallback_enabled: bool,
    pub payment_cashu_route_rollout_percent: u8,
    pub payment_cashu_route_health_fail_threshold: u32,
    pub payment_cashu_route_health_cooldown: Duration,
    pub payment_cashu_mock_enabled: bool,
    pub payment_cashu_mock_balance_msat: u64,
    pub payment_max_spend_msat_per_run: Option<u64>,
    pub payment_approval_threshold_msat: Option<u64>,
    pub payment_max_spend_msat_per_tenant: Option<u64>,
    pub payment_max_spend_msat_per_agent: Option<u64>,
    pub trigger_scheduler_enabled: bool,
    pub trigger_tenant_max_inflight_runs: i64,
    pub trigger_dispatch_max_inflight_runs: i64,
    pub claim_max_inflight_runs: i64,
    pub trigger_scheduler_lease_enabled: bool,
    pub trigger_scheduler_lease_name: String,
    pub trigger_scheduler_lease_ttl: Duration,
    pub memory_compaction_enabled: bool,
    pub memory_compaction_min_records: i64,
    pub memory_compaction_max_groups_per_cycle: i64,
    pub memory_compaction_min_age: Duration,
    pub agent_context_enabled: bool,
    pub agent_context_required: bool,
    pub agent_context_loader: AgentContextLoaderConfig,
    pub compliance_siem_delivery_enabled: bool,
    pub compliance_siem_delivery_batch_size: i64,
    pub compliance_siem_delivery_lease: Duration,
    pub compliance_siem_delivery_retry_backoff: Duration,
    pub compliance_siem_delivery_retry_jitter_max: Duration,
    pub compliance_siem_delivery_http_enabled: bool,
    pub compliance_siem_delivery_http_timeout: Duration,
    pub compliance_siem_delivery_http_auth_header: String,
    pub compliance_siem_delivery_http_auth_token: Option<String>,
}

impl WorkerConfig {
    fn resolve_skill_script_path() -> String {
        let manifest_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../skills/python/summarize_transcript/main.py");

        let fallback_roots = ["WORKER_ARTIFACT_ROOT", "SOLO_LITE_ARTIFACT_ROOT"];
        if manifest_candidate.is_file() {
            return manifest_candidate.to_string_lossy().to_string();
        }

        for root_var in fallback_roots {
            if let Ok(root) = env::var(root_var) {
                let candidate = Path::new(&root).join("skills/python/summarize_transcript/main.py");
                if candidate.is_file() {
                    return candidate.to_string_lossy().to_string();
                }
            }
        }

        manifest_candidate.to_string_lossy().to_string()
    }

    pub fn from_env() -> Result<Self> {
        let skill_command =
            env::var("WORKER_SKILL_COMMAND").unwrap_or_else(|_| "python3".to_string());
        let default_skill_script = Self::resolve_skill_script_path();
        let skill_script = if let Ok(configured) = env::var("WORKER_SKILL_SCRIPT") {
            if Path::new(&configured).is_file() {
                configured
            } else if let Some(root) = env::var("WORKER_ARTIFACT_ROOT")
                .ok()
                .or_else(|| env::var("SOLO_LITE_ARTIFACT_ROOT").ok())
            {
                let staged_script = Path::new(&root)
                    .join("skills/python/summarize_transcript/main.py")
                    .to_string_lossy()
                    .to_string();
                if Path::new(&staged_script).is_file() {
                    staged_script
                } else {
                    default_skill_script
                }
            } else {
                default_skill_script
            }
        } else {
            default_skill_script
        };
        let mut skill_args = vec![skill_script];
        if let Ok(extra) = env::var("WORKER_SKILL_ARGS") {
            skill_args.extend(extra.split_whitespace().map(ToString::to_string));
        }
        let skill_recipe_commands =
            read_env_skill_recipe_commands("WORKER_SKILL_RECIPE_COMMANDS")?;

        let local_exec_enabled = read_env_bool("WORKER_LOCAL_EXEC_ENABLED", false);
        let local_exec_read_roots = if local_exec_enabled {
            parse_roots_from_env(
                read_env_csv("WORKER_LOCAL_EXEC_READ_ROOTS"),
                "WORKER_LOCAL_EXEC_READ_ROOTS",
            )?
        } else {
            Vec::new()
        };
        let local_exec_write_roots = if local_exec_enabled {
            parse_roots_from_env(
                read_env_csv("WORKER_LOCAL_EXEC_WRITE_ROOTS"),
                "WORKER_LOCAL_EXEC_WRITE_ROOTS",
            )?
        } else {
            Vec::new()
        };
        let agent_context_required_files = {
            let configured = normalize_agent_context_required_files(&read_env_csv(
                "WORKER_AGENT_CONTEXT_REQUIRED_FILES",
            ));
            if configured.is_empty() {
                default_agent_context_required_files()
            } else {
                configured
            }
        };
        let agent_context_root =
            env::var("WORKER_AGENT_CONTEXT_ROOT").unwrap_or_else(|_| "agent_context".to_string());

        Ok(Self {
            worker_id: env::var("WORKER_ID")
                .unwrap_or_else(|_| format!("worker-{}", Uuid::new_v4())),
            lease_for: Duration::from_secs(read_env_u64("WORKER_LEASE_SECS", 30)?),
            requeue_limit: read_env_i64("WORKER_REQUEUE_LIMIT", 100)?,
            poll_interval: Duration::from_millis(read_env_u64("WORKER_POLL_MS", 750)?),
            skill_command,
            skill_args,
            skill_recipe_commands,
            skill_timeout: Duration::from_millis(read_env_u64("WORKER_SKILL_TIMEOUT_MS", 5000)?),
            skill_max_output_bytes: read_env_u64("WORKER_SKILL_MAX_OUTPUT_BYTES", 64 * 1024)?
                as usize,
            skill_env_allowlist: read_env_csv("WORKER_SKILL_ENV_ALLOWLIST"),
            skill_script_sha256: read_env_optional_sha256("WORKER_SKILL_SCRIPT_SHA256")?,
            approval_required_action_types: normalize_action_type_list(read_env_csv(
                "WORKER_APPROVAL_REQUIRED_ACTION_TYPES",
            )),
            llm: LlmConfig::from_env()?,
            local_exec: LocalExecConfig {
                enabled: local_exec_enabled,
                timeout: Duration::from_millis(read_env_u64("WORKER_LOCAL_EXEC_TIMEOUT_MS", 2000)?),
                max_output_bytes: read_env_u64("WORKER_LOCAL_EXEC_MAX_OUTPUT_BYTES", 16 * 1024)?
                    as usize,
                max_memory_bytes: read_env_u64(
                    "WORKER_LOCAL_EXEC_MAX_MEMORY_BYTES",
                    256 * 1024 * 1024,
                )?,
                max_processes: read_env_u64("WORKER_LOCAL_EXEC_MAX_PROCESSES", 32)?,
                read_roots: local_exec_read_roots,
                write_roots: local_exec_write_roots,
            },
            artifact_root: PathBuf::from(
                env::var("WORKER_ARTIFACT_ROOT").unwrap_or_else(|_| "artifacts".to_string()),
            ),
            nostr_signer: NostrSignerConfig::from_env()?,
            nostr_relays: read_env_csv("NOSTR_RELAYS"),
            nostr_publish_timeout: Duration::from_millis(read_env_u64(
                "NOSTR_PUBLISH_TIMEOUT_MS",
                4000,
            )?),
            slack_webhook_url: read_env_secret("SLACK_WEBHOOK_URL", "SLACK_WEBHOOK_URL_REF")?,
            slack_send_timeout: Duration::from_millis(read_env_u64("SLACK_SEND_TIMEOUT_MS", 4000)?),
            slack_max_attempts: read_env_u64("SLACK_MAX_ATTEMPTS", 3)?.max(1) as u32,
            slack_retry_backoff: Duration::from_millis(read_env_u64(
                "SLACK_RETRY_BACKOFF_MS",
                500,
            )?),
            message_whitenoise_destination_allowlist: read_env_csv(
                "WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST",
            ),
            message_slack_destination_allowlist: read_env_csv(
                "WORKER_MESSAGE_SLACK_DEST_ALLOWLIST",
            ),
            payment_nwc_enabled: read_env_bool("PAYMENT_NWC_ENABLED", false),
            payment_nwc_uri: read_env_secret("PAYMENT_NWC_URI", "PAYMENT_NWC_URI_REF")?,
            payment_nwc_wallet_uris: read_env_secret_map(
                "PAYMENT_NWC_WALLET_URIS",
                "PAYMENT_NWC_WALLET_URIS_REF",
            )?,
            payment_nwc_timeout: Duration::from_millis(read_env_u64(
                "PAYMENT_NWC_TIMEOUT_MS",
                5000,
            )?),
            payment_nwc_route_strategy: PaymentNwcRouteStrategy::from_env()?,
            payment_nwc_route_fallback_enabled: read_env_bool(
                "PAYMENT_NWC_ROUTE_FALLBACK_ENABLED",
                true,
            ),
            payment_nwc_route_rollout_percent: read_env_u8(
                "PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT",
                100,
                0,
                100,
            )?,
            payment_nwc_route_health_fail_threshold: read_env_u64(
                "PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD",
                3,
            )? as u32,
            payment_nwc_route_health_cooldown: Duration::from_secs(read_env_u64(
                "PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS",
                60,
            )?),
            payment_nwc_mock_balance_msat: read_env_u64(
                "PAYMENT_NWC_MOCK_BALANCE_MSAT",
                1_000_000,
            )?,
            payment_cashu_enabled: read_env_bool("PAYMENT_CASHU_ENABLED", false),
            payment_cashu_mint_uris: read_env_secret_map(
                "PAYMENT_CASHU_MINT_URIS",
                "PAYMENT_CASHU_MINT_URIS_REF",
            )?,
            payment_cashu_default_mint: env::var("PAYMENT_CASHU_DEFAULT_MINT")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            payment_cashu_timeout: Duration::from_millis(read_env_u64(
                "PAYMENT_CASHU_TIMEOUT_MS",
                5000,
            )?),
            payment_cashu_max_spend_msat_per_run: read_env_optional_u64(
                "PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN",
            )?,
            payment_cashu_http_enabled: read_env_bool("PAYMENT_CASHU_HTTP_ENABLED", false),
            payment_cashu_http_allow_insecure: read_env_bool(
                "PAYMENT_CASHU_HTTP_ALLOW_INSECURE",
                false,
            ),
            payment_cashu_auth_header: read_env_non_empty_string(
                "PAYMENT_CASHU_AUTH_HEADER",
                "authorization",
            ),
            payment_cashu_auth_token: read_env_secret(
                "PAYMENT_CASHU_AUTH_TOKEN",
                "PAYMENT_CASHU_AUTH_TOKEN_REF",
            )?,
            payment_cashu_route_strategy: PaymentNwcRouteStrategy::from_env_var(
                "PAYMENT_CASHU_ROUTE_STRATEGY",
            )?,
            payment_cashu_route_fallback_enabled: read_env_bool(
                "PAYMENT_CASHU_ROUTE_FALLBACK_ENABLED",
                true,
            ),
            payment_cashu_route_rollout_percent: read_env_u8(
                "PAYMENT_CASHU_ROUTE_ROLLOUT_PERCENT",
                100,
                0,
                100,
            )?,
            payment_cashu_route_health_fail_threshold: read_env_u64(
                "PAYMENT_CASHU_ROUTE_HEALTH_FAIL_THRESHOLD",
                3,
            )? as u32,
            payment_cashu_route_health_cooldown: Duration::from_secs(read_env_u64(
                "PAYMENT_CASHU_ROUTE_HEALTH_COOLDOWN_SECS",
                60,
            )?),
            payment_cashu_mock_enabled: read_env_bool("PAYMENT_CASHU_MOCK_ENABLED", false),
            payment_cashu_mock_balance_msat: read_env_u64(
                "PAYMENT_CASHU_MOCK_BALANCE_MSAT",
                1_000_000,
            )?,
            payment_max_spend_msat_per_run: read_env_optional_u64(
                "PAYMENT_MAX_SPEND_MSAT_PER_RUN",
            )?,
            payment_approval_threshold_msat: read_env_optional_u64(
                "PAYMENT_APPROVAL_THRESHOLD_MSAT",
            )?,
            payment_max_spend_msat_per_tenant: read_env_optional_u64(
                "PAYMENT_MAX_SPEND_MSAT_PER_TENANT",
            )?,
            payment_max_spend_msat_per_agent: read_env_optional_u64(
                "PAYMENT_MAX_SPEND_MSAT_PER_AGENT",
            )?,
            trigger_scheduler_enabled: read_env_bool("WORKER_TRIGGER_SCHEDULER_ENABLED", true),
            trigger_tenant_max_inflight_runs: read_env_i64(
                "WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS",
                100,
            )?
            .max(1),
            trigger_dispatch_max_inflight_runs: read_env_i64(
                "WORKER_TRIGGER_DISPATCH_MAX_INFLIGHT_RUNS",
                1_000,
            )?
            .max(1),
            claim_max_inflight_runs: read_env_i64("WORKER_CLAIM_MAX_INFLIGHT_RUNS", 1_000)?.max(1),
            trigger_scheduler_lease_enabled: read_env_bool(
                "WORKER_TRIGGER_SCHEDULER_LEASE_ENABLED",
                true,
            ),
            trigger_scheduler_lease_name: env::var("WORKER_TRIGGER_SCHEDULER_LEASE_NAME")
                .unwrap_or_else(|_| "default".to_string()),
            trigger_scheduler_lease_ttl: Duration::from_millis(read_env_u64(
                "WORKER_TRIGGER_SCHEDULER_LEASE_TTL_MS",
                3000,
            )?),
            memory_compaction_enabled: read_env_bool("WORKER_MEMORY_COMPACTION_ENABLED", true),
            memory_compaction_min_records: read_env_i64(
                "WORKER_MEMORY_COMPACTION_MIN_RECORDS",
                10,
            )?
            .max(2),
            memory_compaction_max_groups_per_cycle: read_env_i64(
                "WORKER_MEMORY_COMPACTION_MAX_GROUPS_PER_CYCLE",
                5,
            )?
            .max(1),
            memory_compaction_min_age: Duration::from_secs(read_env_u64(
                "WORKER_MEMORY_COMPACTION_MIN_AGE_SECS",
                300,
            )?),
            agent_context_enabled: read_env_bool("WORKER_AGENT_CONTEXT_ENABLED", false),
            agent_context_required: read_env_bool("WORKER_AGENT_CONTEXT_REQUIRED", false),
            agent_context_loader: AgentContextLoaderConfig {
                root_dir: PathBuf::from(agent_context_root),
                required_files: agent_context_required_files,
                max_file_bytes: read_env_u64("WORKER_AGENT_CONTEXT_MAX_FILE_BYTES", 64 * 1024)?
                    as usize,
                max_total_bytes: read_env_u64("WORKER_AGENT_CONTEXT_MAX_TOTAL_BYTES", 256 * 1024)?
                    as usize,
                max_dynamic_files_per_dir: read_env_u64(
                    "WORKER_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR",
                    8,
                )? as usize,
            },
            compliance_siem_delivery_enabled: read_env_bool(
                "WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED",
                false,
            ),
            compliance_siem_delivery_batch_size: read_env_i64(
                "WORKER_COMPLIANCE_SIEM_DELIVERY_BATCH_SIZE",
                10,
            )?
            .clamp(1, 200),
            compliance_siem_delivery_lease: Duration::from_millis(read_env_u64(
                "WORKER_COMPLIANCE_SIEM_DELIVERY_LEASE_MS",
                30_000,
            )?),
            compliance_siem_delivery_retry_backoff: Duration::from_millis(read_env_u64(
                "WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_BACKOFF_MS",
                5_000,
            )?),
            compliance_siem_delivery_retry_jitter_max: Duration::from_millis(read_env_u64(
                "WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_JITTER_MAX_MS",
                1_000,
            )?),
            compliance_siem_delivery_http_enabled: read_env_bool(
                "WORKER_COMPLIANCE_SIEM_HTTP_ENABLED",
                false,
            ),
            compliance_siem_delivery_http_timeout: Duration::from_millis(read_env_u64(
                "WORKER_COMPLIANCE_SIEM_HTTP_TIMEOUT_MS",
                5_000,
            )?),
            compliance_siem_delivery_http_auth_header: read_env_non_empty_string(
                "WORKER_COMPLIANCE_SIEM_HTTP_AUTH_HEADER",
                "authorization",
            ),
            compliance_siem_delivery_http_auth_token: read_env_secret(
                "WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN",
                "WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN_REF",
            )?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerCycleOutcome {
    ClaimedAndSucceeded { run_id: Uuid },
    ClaimedAndFailed { run_id: Uuid },
    Idle { requeued_expired_runs: u64 },
}

// --- Worker cycle entrypoint and control flow ---

pub async fn process_once(pool: &PgPool, config: &WorkerConfig) -> Result<WorkerCycleOutcome> {
    let db_pool = DbPool::Postgres(pool.clone());
    process_once_dual(&db_pool, config).await
}

pub async fn process_once_dual(pool: &DbPool, config: &WorkerConfig) -> Result<WorkerCycleOutcome> {
    let requeued_expired_runs = requeue_expired_runs_dual(pool, config.requeue_limit).await?;
    if config.memory_compaction_enabled {
        let compaction_cutoff = OffsetDateTime::now_utc()
            - time::Duration::seconds(config.memory_compaction_min_age.as_secs() as i64);
        let compaction_stats = compact_memory_records_dual(
            pool,
            compaction_cutoff,
            config.memory_compaction_min_records,
            config.memory_compaction_max_groups_per_cycle,
        )
        .await?;

        if compaction_stats.processed_groups > 0 {
            for group in compaction_stats.groups {
                let Some(run_id) = group.representative_run_id else {
                    continue;
                };
                append_audit_event_with_trace(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id,
                        step_id: group.representative_step_id,
                        tenant_id: group.tenant_id,
                        agent_id: Some(group.agent_id),
                        user_id: None,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "memory.compacted".to_string(),
                        payload_json: json!({
                            "memory_kind": group.memory_kind,
                            "scope": group.scope,
                            "source_count": group.source_count,
                        "source_entry_ids": group.source_entry_ids,
                    }),
                },
                None,
            )
            .await?;
            }
        }
    }
    if config.compliance_siem_delivery_enabled {
        process_compliance_siem_delivery_outbox(pool, config).await?;
    }

    if config.trigger_scheduler_enabled {
        let global_inflight = count_inflight_runs_dual(pool).await?;
        let should_dispatch = if global_inflight >= config.trigger_dispatch_max_inflight_runs {
            false
        } else if config.trigger_scheduler_lease_enabled {
            try_acquire_scheduler_lease_dual(
                pool,
                &SchedulerLeaseParams {
                    lease_name: config.trigger_scheduler_lease_name.clone(),
                    lease_owner: config.worker_id.clone(),
                    lease_for: config.trigger_scheduler_lease_ttl,
                },
            )
            .await?
        } else {
            true
        };
        if should_dispatch {
            if let Some(dispatched) = dispatch_next_due_trigger_with_limits_dual(
                pool,
                config.trigger_tenant_max_inflight_runs,
            )
            .await?
            {
                append_audit_event_with_trace(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: dispatched.run_id,
                        step_id: None,
                        tenant_id: dispatched.tenant_id,
                        agent_id: Some(dispatched.agent_id),
                        user_id: dispatched.triggered_by_user_id,
                        actor: format!("trigger-scheduler:{}", config.worker_id),
                        event_type: "run.created".to_string(),
                        payload_json: json!({
                            "recipe_id": dispatched.recipe_id,
                            "source": "trigger_scheduler",
                            "trigger_id": dispatched.trigger_id,
                            "trigger_type": dispatched.trigger_type,
                            "trigger_event_id": dispatched.trigger_event_id,
                            "scheduled_for": dispatched.scheduled_for,
                            "next_fire_at": dispatched.next_fire_at,
                        }),
                    },
                    None,
                )
                .await?;
            }
        }
    }

    let Some(claimed_run) = claim_next_queued_run_with_limits_dual(
        pool,
        &config.worker_id,
        config.lease_for,
        config.claim_max_inflight_runs,
        config.trigger_tenant_max_inflight_runs,
    )
    .await?
    else {
        return Ok(WorkerCycleOutcome::Idle {
            requeued_expired_runs,
        });
    };
    let claimed_run_trace_id = extract_trace_id_from_run_input(&claimed_run.input_json);

    append_audit_event_with_trace(
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
            claimed_run_trace_id.as_deref(),
        )
        .await?;

    let renewed =
        renew_run_lease_dual(pool, claimed_run.id, &config.worker_id, config.lease_for).await?;
    if !renewed {
        append_audit_event_with_trace(
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
            claimed_run_trace_id.as_deref(),
        )
        .await?;

        let _ = mark_run_failed_dual(
            pool,
            claimed_run.id,
            &config.worker_id,
            redact_json(&json!({
                "code": "LEASE_RENEW_FAILED",
                "message": "worker failed to renew run lease after claim"
            })),
        )
        .await?;

        return Ok(WorkerCycleOutcome::Idle {
            requeued_expired_runs,
        });
    }

    append_audit_event_with_trace(
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
        claimed_run_trace_id.as_deref(),
    )
    .await?;

    let run_result =
        process_claimed_run(pool, config, &claimed_run, claimed_run_trace_id.as_deref()).await;
    match run_result {
        Ok(()) => {
            let completed =
                mark_run_succeeded_dual(pool, claimed_run.id, &config.worker_id).await?;
            if completed {
                append_audit_event_with_trace(
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
                    claimed_run_trace_id.as_deref(),
                )
                .await?;

                Ok(WorkerCycleOutcome::ClaimedAndSucceeded {
                    run_id: claimed_run.id,
                })
            } else {
                mark_run_failed_dual(
                    pool,
                    claimed_run.id,
                    &config.worker_id,
                    redact_json(&json!({
                        "code": "RUN_FINALIZE_FAILED",
                        "message": "worker could not mark run as succeeded"
                    })),
                )
                .await?;

                Ok(WorkerCycleOutcome::ClaimedAndFailed {
                    run_id: claimed_run.id,
                })
            }
        }
        Err(error) => {
            let error_message = redact_text(&format!("{error:#}"));
            mark_run_failed_dual(
                pool,
                claimed_run.id,
                &config.worker_id,
                redact_json(&json!({
                    "code": "RUN_EXECUTION_FAILED",
                    "message": error_message,
                })),
            )
            .await?;

            append_audit_event_with_trace(
                pool,
                &NewAuditEvent {
                    id: Uuid::new_v4(),
                    run_id: claimed_run.id,
                    step_id: None,
                    tenant_id: claimed_run.tenant_id,
                    agent_id: Some(claimed_run.agent_id),
                    user_id: claimed_run.triggered_by_user_id,
                    actor: format!("worker:{}", config.worker_id),
                    event_type: "run.failed".to_string(),
                    payload_json: json!({"error": error_message}),
                },
                claimed_run_trace_id.as_deref(),
            )
            .await?;

            Ok(WorkerCycleOutcome::ClaimedAndFailed {
                run_id: claimed_run.id,
            })
        }
    }
}

enum SiemDeliveryAttempt {
    Delivered {
        http_status: Option<i32>,
    },
    Failed {
        http_status: Option<i32>,
        error: String,
        retryable: bool,
    },
}

// --- Compliance SIEM outbox processor ---

async fn process_compliance_siem_delivery_outbox(
    pool: &DbPool,
    config: &WorkerConfig,
) -> Result<()> {
    let records = claim_pending_compliance_siem_delivery_records_dual(
        pool,
        &config.worker_id,
        config.compliance_siem_delivery_lease,
        config.compliance_siem_delivery_batch_size,
    )
    .await?;
    if records.is_empty() {
        return Ok(());
    }

    let http_client = if config.compliance_siem_delivery_http_enabled {
        Some(
            reqwest::Client::builder()
                .timeout(config.compliance_siem_delivery_http_timeout)
                .build()
                .context("failed building SIEM delivery HTTP client")?,
        )
    } else {
        None
    };
    let retry_ms = config
        .compliance_siem_delivery_retry_backoff
        .as_millis()
        .clamp(1, i64::MAX as u128) as i64;
    let jitter_max_ms = config
        .compliance_siem_delivery_retry_jitter_max
        .as_millis()
        .clamp(0, i64::MAX as u128) as i64;

    for record in records {
        match attempt_compliance_siem_delivery(record.clone(), http_client.as_ref(), config).await {
            SiemDeliveryAttempt::Delivered { http_status } => {
                mark_compliance_siem_delivery_record_delivered_dual(pool, record.id, http_status)
                    .await?;
            }
            SiemDeliveryAttempt::Failed {
                http_status,
                error,
                retryable,
            } => {
                if retryable {
                    let retry_jitter_ms =
                        deterministic_retry_jitter_ms(record.id, record.attempts, jitter_max_ms);
                    let retry_at = OffsetDateTime::now_utc()
                        + time::Duration::milliseconds(retry_ms.saturating_add(retry_jitter_ms));
                    mark_compliance_siem_delivery_record_failed_dual(
                        pool,
                        record.id,
                        error.as_str(),
                        http_status,
                        retry_at,
                    )
                    .await?;
                } else {
                    mark_compliance_siem_delivery_record_dead_lettered_dual(
                        pool,
                        record.id,
                        error.as_str(),
                        http_status,
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

async fn attempt_compliance_siem_delivery(
    record: agent_core::ComplianceSiemDeliveryRecord,
    http_client: Option<&reqwest::Client>,
    config: &WorkerConfig,
) -> SiemDeliveryAttempt {
    let target = record.delivery_target.trim();

    if target.eq_ignore_ascii_case("mock://success") {
        return SiemDeliveryAttempt::Delivered {
            http_status: Some(200),
        };
    }
    if target.eq_ignore_ascii_case("mock://fail") {
        return SiemDeliveryAttempt::Failed {
            http_status: None,
            error: "mock failure target".to_string(),
            retryable: true,
        };
    }

    if target.starts_with("http://") || target.starts_with("https://") {
        if !config.compliance_siem_delivery_http_enabled {
            return SiemDeliveryAttempt::Failed {
                http_status: None,
                error: "HTTP delivery is disabled by WORKER_COMPLIANCE_SIEM_HTTP_ENABLED"
                    .to_string(),
                retryable: false,
            };
        }
        let Some(client) = http_client else {
            return SiemDeliveryAttempt::Failed {
                http_status: None,
                error: "HTTP delivery client not configured".to_string(),
                retryable: false,
            };
        };

        let response = client
            .post(target)
            .header("content-type", record.content_type.as_str())
            .header("x-secureagnt-siem-adapter", record.adapter.as_str());
        let response = with_optional_auth_header(
            response,
            config.compliance_siem_delivery_http_auth_header.as_str(),
            config.compliance_siem_delivery_http_auth_token.as_deref(),
        )
        .body(record.payload_ndjson)
        .send()
        .await;

        return match response {
            Ok(resp) => {
                let status = resp.status().as_u16() as i32;
                if resp.status().is_success() {
                    SiemDeliveryAttempt::Delivered {
                        http_status: Some(status),
                    }
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    let truncated = body.chars().take(512).collect::<String>();
                    SiemDeliveryAttempt::Failed {
                        http_status: Some(status),
                        error: format!("http delivery failed: status={status} body={truncated}"),
                        retryable: !is_non_retryable_siem_http_status(status),
                    }
                }
            }
            Err(err) => SiemDeliveryAttempt::Failed {
                http_status: None,
                error: format!("http delivery request failed: {err}"),
                retryable: true,
            },
        };
    }

    SiemDeliveryAttempt::Failed {
        http_status: None,
        error: format!("unsupported SIEM delivery target scheme: {target}"),
        retryable: false,
    }
}

fn is_non_retryable_siem_http_status(status: i32) -> bool {
    matches!(status, 400 | 401 | 403 | 404 | 405 | 410 | 422)
}

fn deterministic_retry_jitter_ms(record_id: Uuid, attempt_count: i32, jitter_max_ms: i64) -> i64 {
    if jitter_max_ms <= 0 {
        return 0;
    }
    let mut accum: u64 = 0;
    for byte in record_id.as_bytes() {
        accum = accum.wrapping_mul(131).wrapping_add(u64::from(*byte));
    }
    let salted = accum.wrapping_add(attempt_count.max(0) as u64);
    (salted % jitter_max_ms as u64) as i64
}

fn with_optional_auth_header<'a>(
    builder: reqwest::RequestBuilder,
    header_name: &'a str,
    token: Option<&'a str>,
) -> reqwest::RequestBuilder {
    let Some(raw_token) = token.map(str::trim).filter(|value| !value.is_empty()) else {
        return builder;
    };
    let token_value = if header_name.eq_ignore_ascii_case("authorization")
        && !raw_token.to_ascii_lowercase().starts_with("bearer ")
    {
        format!("Bearer {raw_token}")
    } else {
        raw_token.to_string()
    };
    builder.header(header_name, token_value)
}

async fn process_claimed_run(
    pool: &DbPool,
    config: &WorkerConfig,
    run: &agent_core::RunLeaseRecord,
    run_trace_id: Option<&str>,
) -> Result<()> {
    let step = create_step_dual(
        pool,
        &NewStep {
            id: Uuid::new_v4(),
            run_id: run.id,
            tenant_id: run.tenant_id.clone(),
            agent_id: run.agent_id,
            user_id: run.triggered_by_user_id,
            name: run.recipe_id.clone(),
            status: "running".to_string(),
            input_json: run.input_json.clone(),
            error_json: None,
        },
    )
    .await?;

    append_audit_event_with_trace(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: run.id,
            step_id: Some(step.id),
            tenant_id: run.tenant_id.clone(),
            agent_id: Some(run.agent_id),
            user_id: run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "step.started".to_string(),
            payload_json: json!({"step_name": step.name}),
        },
        run_trace_id,
    )
    .await?;

    let grants = parse_grant_set(&run.granted_capabilities);
    let skill_input = prepare_skill_input_with_agent_context(
        pool,
        config,
        run,
        step.id,
        run_trace_id,
    )
    .await?;
    let invoke_result = match invoke_skill(
        config,
        run,
        step.id,
        run_trace_id,
        skill_input,
        &grants,
    )
    .await {
        Ok(result) => result,
        Err(error) => {
            let error_message = redact_text(&format!("{error:#}"));
            mark_step_failed_dual(
                pool,
                step.id,
                redact_json(&json!({"code": "SKILL_INVOKE_FAILED", "message": error_message})),
            )
            .await?;
            append_audit_event_with_trace(
                pool,
                &NewAuditEvent {
                    id: Uuid::new_v4(),
                    run_id: run.id,
                    step_id: Some(step.id),
                    tenant_id: run.tenant_id.clone(),
                    agent_id: Some(run.agent_id),
                    user_id: run.triggered_by_user_id,
                    actor: format!("worker:{}", config.worker_id),
                    event_type: "step.failed".to_string(),
                    payload_json: json!({"error": error_message}),
                },
                run_trace_id,
            )
            .await?;
            return Err(error);
        }
    };

    append_audit_event_with_trace(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: run.id,
            step_id: Some(step.id),
            tenant_id: run.tenant_id.clone(),
            agent_id: Some(run.agent_id),
            user_id: run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "skill.invoked".to_string(),
            payload_json: json!({
                "action_request_count": invoke_result.action_requests.len(),
                "agent_context_enabled": config.agent_context_enabled,
            }),
        },
            run_trace_id,
    )
    .await?;

    let mut action_execution_context = ActionExecutionContext {
        remote_llm_tokens_remaining: config.llm.remote_token_budget_per_run,
        payment_spend_msat: 0,
        last_llm_response_text: None,
    };

    for mut skill_action in invoke_result.action_requests {
        normalize_skill_action(&mut skill_action);
        let action_request_id = Uuid::new_v4();
        create_action_request_dual(
            pool,
            &NewActionRequest {
                id: action_request_id,
                step_id: step.id,
                action_type: skill_action.action_type.clone(),
                args_json: redact_json(&skill_action.args),
                justification: Some(redact_text(&skill_action.justification)),
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        append_audit_event_with_trace(
            pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id: run.id,
                step_id: Some(step.id),
                tenant_id: run.tenant_id.clone(),
                agent_id: Some(run.agent_id),
                user_id: run.triggered_by_user_id,
                actor: format!("worker:{}", config.worker_id),
                event_type: "action.requested".to_string(),
                payload_json: json!({
                    "action_id": skill_action.action_id,
                    "action_type": skill_action.action_type,
                }),
            },
            run_trace_id,
        )
        .await?;

        let policy_request = match to_policy_request(&skill_action, config) {
            Ok(policy_request) => policy_request,
            Err(contract_error) => {
                let reason_str = contract_error.reason.as_str();
                let reason_detail = contract_error.detail;
                let reason_detail_for_event = reason_detail.clone();
                update_action_request_status_dual(
                    pool,
                    action_request_id,
                    "denied",
                    Some(reason_str),
                )
                .await?;
                create_action_result_dual(
                    pool,
                    &NewActionResult {
                        id: Uuid::new_v4(),
                        action_request_id,
                        status: "denied".to_string(),
                        result_json: None,
                        error_json: Some(redact_json(&json!({
                            "code": "POLICY_DENIED",
                            "reason": reason_str,
                            "detail": reason_detail,
                        }))),
                    },
                )
                .await?;

                append_audit_event_with_trace(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "action.denied".to_string(),
                        payload_json: json!({
                            "action_type": skill_action.action_type,
                            "reason": reason_str,
                            "source": "contract",
                            "detail": reason_detail_for_event,
                        }),
                    },
                    run_trace_id,
                )
                .await?;

                let _ = mark_step_failed_dual(
                    pool,
                    step.id,
                    redact_json(&json!({
                        "code": "ACTION_DENIED",
                        "reason": reason_str,
                    })),
                )
                .await?;

                return Err(anyhow!("action denied by contract validation"));
            }
        };
        match grants.is_action_allowed(&policy_request) {
            PolicyDecision::Allow => {
                if requires_governance_approval(config, &policy_request.action_type)
                    && !is_action_governance_approved(&skill_action)
                {
                    let reason_str = "approval_required";
                    update_action_request_status_dual(
                        pool,
                        action_request_id,
                        "denied",
                        Some(reason_str),
                    )
                    .await?;
                    create_action_result_dual(
                        pool,
                        &NewActionResult {
                            id: Uuid::new_v4(),
                            action_request_id,
                            status: "denied".to_string(),
                            result_json: None,
                            error_json: Some(redact_json(&json!({
                                "code": "POLICY_DENIED",
                                "reason": reason_str,
                            }))),
                        },
                    )
                    .await?;

                    append_audit_event_with_trace(
                        pool,
                        &NewAuditEvent {
                            id: Uuid::new_v4(),
                            run_id: run.id,
                            step_id: Some(step.id),
                            tenant_id: run.tenant_id.clone(),
                            agent_id: Some(run.agent_id),
                            user_id: run.triggered_by_user_id,
                            actor: format!("worker:{}", config.worker_id),
                            event_type: "action.denied".to_string(),
                            payload_json: json!({
                                "action_type": policy_request.action_type,
                                "reason": reason_str,
                                "source": "governance",
                            }),
                        },
                        run_trace_id,
                    )
                    .await?;

                    let _ = mark_step_failed_dual(
                        pool,
                        step.id,
                        redact_json(&json!({
                            "code": "ACTION_DENIED",
                            "reason": reason_str,
                        })),
                    )
                    .await?;

                    return Err(anyhow!(
                        "action denied by governance approval gate: {}",
                        reason_str
                    ));
                }

                update_action_request_status_dual(pool, action_request_id, "allowed", None).await?;
                append_audit_event_with_trace(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                    event_type: "action.allowed".to_string(),
                    payload_json: json!({"action_type": policy_request.action_type}),
                },
                run_trace_id,
                )
                .await?;

                let result_json = match execute_action(
                    pool,
                    run,
                    action_request_id,
                    &skill_action,
                    config,
                    &mut action_execution_context,
                )
                .await
                {
                    Ok(result_json) => result_json,
                    Err(error) => {
                        let error_message = redact_text(&format!("{error:#}"));
                        update_action_request_status_dual(
                            pool,
                            action_request_id,
                            "failed",
                            Some("execution_failed"),
                        )
                        .await?;
                        create_action_result_dual(
                            pool,
                            &NewActionResult {
                                id: Uuid::new_v4(),
                                action_request_id,
                                status: "failed".to_string(),
                                result_json: None,
                                error_json: Some(redact_json(&json!({
                                    "code": "ACTION_EXECUTION_FAILED",
                                    "message": error_message,
                                }))),
                            },
                        )
                        .await?;
                        append_audit_event_with_trace(
                            pool,
                            &NewAuditEvent {
                                id: Uuid::new_v4(),
                                run_id: run.id,
                                step_id: Some(step.id),
                                tenant_id: run.tenant_id.clone(),
                                agent_id: Some(run.agent_id),
                                user_id: run.triggered_by_user_id,
                                actor: format!("worker:{}", config.worker_id),
                                event_type: "action.failed".to_string(),
                                payload_json: json!({
                                    "action_type": policy_request.action_type,
                                    "error": error_message,
                                }),
                            },
                            run_trace_id,
                        )
                        .await?;
                        let _ = mark_step_failed_dual(
                            pool,
                            step.id,
                            redact_json(&json!({
                                "code": "ACTION_EXECUTION_FAILED",
                                "message": error_message,
                            })),
                        )
                        .await?;
                        return Err(anyhow!("action execution failed: {}", error_message));
                    }
                };

                update_action_request_status_dual(pool, action_request_id, "executed", None)
                    .await?;
                let (llm_budget_soft_alerts, llm_slo_alerts) =
                    if policy_request.action_type == "llm.infer" {
                        (
                            extract_llm_budget_soft_alerts(&result_json),
                            extract_llm_slo_alerts(&result_json),
                        )
                    } else {
                        (Vec::new(), Vec::new())
                    };
                create_action_result_dual(
                    pool,
                    &NewActionResult {
                        id: Uuid::new_v4(),
                        action_request_id,
                        status: "executed".to_string(),
                        result_json: Some(redact_json(&result_json)),
                        error_json: None,
                    },
                )
                .await?;

                if !llm_budget_soft_alerts.is_empty() {
                    append_audit_event_with_trace(
                        pool,
                        &NewAuditEvent {
                            id: Uuid::new_v4(),
                            run_id: run.id,
                            step_id: Some(step.id),
                            tenant_id: run.tenant_id.clone(),
                            agent_id: Some(run.agent_id),
                            user_id: run.triggered_by_user_id,
                            actor: format!("worker:{}", config.worker_id),
                            event_type: "llm.budget.soft_alert".to_string(),
                            payload_json: json!({
                                "action_type": policy_request.action_type,
                                "action_request_id": action_request_id,
                                "alerts": llm_budget_soft_alerts,
                            }),
                        },
                        run_trace_id,
                    )
                    .await?;
                }
                if !llm_slo_alerts.is_empty() {
                    append_audit_event_with_trace(
                        pool,
                        &NewAuditEvent {
                            id: Uuid::new_v4(),
                            run_id: run.id,
                            step_id: Some(step.id),
                            tenant_id: run.tenant_id.clone(),
                            agent_id: Some(run.agent_id),
                            user_id: run.triggered_by_user_id,
                            actor: format!("worker:{}", config.worker_id),
                            event_type: "llm.slo.alert".to_string(),
                            payload_json: json!({
                                "action_type": policy_request.action_type,
                                "action_request_id": action_request_id,
                                "alerts": llm_slo_alerts,
                            }),
                        },
                        run_trace_id,
                    )
                    .await?;
                }

                append_audit_event_with_trace(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "action.executed".to_string(),
                        payload_json: json!({
                            "action_type": policy_request.action_type,
                            "result": result_json,
                        }),
                    },
                    run_trace_id,
                )
                .await?;
            }
            PolicyDecision::Deny(reason) => {
                let reason_str = reason.as_str();
                update_action_request_status_dual(
                    pool,
                    action_request_id,
                    "denied",
                    Some(reason_str),
                )
                .await?;
                create_action_result_dual(
                    pool,
                    &NewActionResult {
                        id: Uuid::new_v4(),
                        action_request_id,
                        status: "denied".to_string(),
                        result_json: None,
                        error_json: Some(redact_json(&json!({
                            "code": "POLICY_DENIED",
                            "reason": reason_str,
                        }))),
                    },
                )
                .await?;

                append_audit_event_with_trace(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "action.denied".to_string(),
                        payload_json: json!({
                            "action_type": policy_request.action_type,
                            "reason": reason_str,
                        }),
                    },
                    run_trace_id,
                )
                .await?;

                let _ = mark_step_failed_dual(
                    pool,
                    step.id,
                    redact_json(&json!({
                        "code": "ACTION_DENIED",
                        "reason": reason_str,
                    })),
                )
                .await?;

                return Err(anyhow!("action denied by policy: {}", reason_str));
            }
        }
    }

    mark_step_succeeded_dual(pool, step.id, invoke_result.output.clone()).await?;
    append_audit_event_with_trace(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: run.id,
            step_id: Some(step.id),
            tenant_id: run.tenant_id.clone(),
            agent_id: Some(run.agent_id),
            user_id: run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "step.completed".to_string(),
            payload_json: json!({}),
        },
        run_trace_id,
    )
        .await?;

    Ok(())
}

async fn prepare_skill_input_with_agent_context(
    pool: &DbPool,
    config: &WorkerConfig,
    run: &agent_core::RunLeaseRecord,
    step_id: Uuid,
    run_trace_id: Option<&str>,
) -> Result<Value> {
    let mut input = inject_runtime_metadata_payload(run.input_json.clone(), run);
    if !config.agent_context_enabled {
        return Ok(input);
    }

    match load_agent_context_snapshot(&config.agent_context_loader, &run.tenant_id, run.agent_id) {
        Ok(snapshot) => {
            append_audit_event_with_trace(
                pool,
                &NewAuditEvent {
                    id: Uuid::new_v4(),
                    run_id: run.id,
                    step_id: Some(step_id),
                    tenant_id: run.tenant_id.clone(),
                    agent_id: Some(run.agent_id),
                    user_id: run.triggered_by_user_id,
                    actor: format!("worker:{}", config.worker_id),
                    event_type: "agent.context.loaded".to_string(),
                    payload_json: snapshot.summary_json(),
                },
                run_trace_id,
            )
            .await?;
            input = inject_agent_context_payload(input, snapshot.skill_context_json());
            Ok(input)
        }
        Err(agent_core::AgentContextLoadError::NotFound { searched_paths }) => {
            append_audit_event_with_trace(
                pool,
                &NewAuditEvent {
                    id: Uuid::new_v4(),
                    run_id: run.id,
                    step_id: Some(step_id),
                    tenant_id: run.tenant_id.clone(),
                    agent_id: Some(run.agent_id),
                    user_id: run.triggered_by_user_id,
                    actor: format!("worker:{}", config.worker_id),
                    event_type: "agent.context.not_found".to_string(),
                    payload_json: json!({
                        "searched_paths": searched_paths,
                        "required": config.agent_context_required,
                    }),
                },
                run_trace_id,
            )
            .await?;
            if config.agent_context_required {
                return Err(anyhow!(
                    "agent context profile required but not found for agent {}",
                    run.agent_id
                ));
            }
            Ok(input)
        }
        Err(error) => {
            append_audit_event_with_trace(
                pool,
                &NewAuditEvent {
                    id: Uuid::new_v4(),
                    run_id: run.id,
                    step_id: Some(step_id),
                    tenant_id: run.tenant_id.clone(),
                    agent_id: Some(run.agent_id),
                    user_id: run.triggered_by_user_id,
                    actor: format!("worker:{}", config.worker_id),
                    event_type: "agent.context.error".to_string(),
                    payload_json: json!({
                        "error": redact_text(&error.to_string()),
                        "required": config.agent_context_required,
                    }),
                },
                run_trace_id,
            )
            .await?;
            if config.agent_context_required {
                return Err(anyhow!("agent context profile load failed: {error}"));
            }
            Ok(input)
        }
    }
}

fn inject_agent_context_payload(input: Value, agent_context: Value) -> Value {
    match input {
        Value::Object(mut map) => {
            map.insert("agent_context".to_string(), agent_context);
            Value::Object(map)
        }
        other => {
            let mut map = serde_json::Map::new();
            map.insert("input".to_string(), other);
            map.insert("agent_context".to_string(), agent_context);
            Value::Object(map)
        }
    }
}

fn inject_runtime_metadata_payload(input: Value, run: &agent_core::RunLeaseRecord) -> Value {
    let metadata = json!({
        "tenant_id": run.tenant_id,
        "agent_id": run.agent_id,
        "run_id": run.id.to_string(),
        "recipe_id": run.recipe_id,
    });

    match input {
        Value::Object(mut map) => {
            map.entry("runtime".to_string())
                .or_insert(metadata);
            Value::Object(map)
        }
        other => {
            let mut map = serde_json::Map::new();
            map.insert("input".to_string(), other);
            map.insert("runtime".to_string(), metadata);
            Value::Object(map)
        }
    }
}

async fn invoke_skill(
    config: &WorkerConfig,
    run: &agent_core::RunLeaseRecord,
    step_id: Uuid,
    run_trace_id: Option<&str>,
    input: Value,
    grants: &GrantSet,
) -> Result<skillrunner::InvokeResult> {
    let (command, args) = resolve_skill_command_for_run(config, &run.recipe_id)?;
    verify_skill_script_provenance(config, &command, &args)?;

    let runner = SkillRunner::new(RunnerConfig {
        command,
        args,
        timeout: config.skill_timeout,
        max_output_bytes: config.skill_max_output_bytes,
        env_allowlist: config.skill_env_allowlist.clone(),
    });

    let granted_capabilities = grants
        .grants
        .iter()
        .map(|grant| SkillCapabilityGrant {
            capability: capability_kind_to_action_type(&grant.kind).to_string(),
            scope: grant.scope.clone(),
        })
        .collect();

    let request = InvokeRequest {
        id: Uuid::new_v4().to_string(),
        context: InvokeContext {
            tenant_id: run.tenant_id.clone(),
            run_id: run.id.to_string(),
            step_id: step_id.to_string(),
            time_budget_ms: config.skill_timeout.as_millis().clamp(1, u64::MAX as u128) as u64,
            trace_id: run_trace_id.map(str::to_string),
            granted_capabilities,
        },
        input,
    };

    let result = runner.invoke(request).await?;
    Ok(result.invoke_result)
}

fn resolve_skill_command_for_run(
    config: &WorkerConfig,
    recipe_id: &str,
) -> Result<(String, Vec<String>)> {
    if let Some(recipe_command) = config.skill_recipe_commands.get(recipe_id) {
        if recipe_command.is_empty() {
            return Err(anyhow!(
                "WORKER_SKILL_RECIPE_COMMANDS entry for `{recipe_id}` is empty"
            ));
        }
        return Ok((
            recipe_command[0].clone(),
            recipe_command[1..].to_vec(),
        ));
    }

    Ok((config.skill_command.clone(), config.skill_args.clone()))
}

fn verify_skill_script_provenance(
    config: &WorkerConfig,
    command: &str,
    args: &[String],
) -> Result<()> {
    let Some(expected_digest) = config.skill_script_sha256.as_deref() else {
        return Ok(());
    };
    let script_path = if let Some(first_arg) = args.first() {
        if first_arg.starts_with('-') {
            command
        } else {
            first_arg.as_str()
        }
    } else {
        command
    };
    if script_path.trim().is_empty() {
        return Err(anyhow!(
            "WORKER_SKILL_SCRIPT_SHA256 requires WORKER_SKILL_SCRIPT or executable path"
        ));
    }
    let script_bytes = fs::read(script_path).with_context(|| {
        format!(
            "failed reading skill script for provenance check: {}",
            script_path
        )
    })?;
    let actual_digest = format!("{:x}", Sha256::digest(script_bytes));
    if actual_digest != expected_digest {
        return Err(anyhow!(
            "WORKER_SKILL_SCRIPT_SHA256 mismatch for {} (expected={}, got={})",
            script_path,
            expected_digest,
            actual_digest
        ));
    }
    Ok(())
}

async fn execute_action(
    pool: &DbPool,
    run: &agent_core::RunLeaseRecord,
    action_request_id: Uuid,
    action: &skillrunner::ActionRequest,
    config: &WorkerConfig,
    execution_context: &mut ActionExecutionContext,
) -> Result<Value> {
    match action.action_type.as_str() {
        "object.write" => {
            execute_object_write_action(pool, run, &action.args, &config.artifact_root).await
        }
        "message.send" => {
            execute_message_send_action(pool, run, &action.args, config, execution_context).await
        }
        "message.receive" => {
            execute_message_receive_action(pool, run, &action.args, config).await
        }
        "payment.send" => {
            execute_payment_send_action(
                pool,
                run,
                action_request_id,
                &action.args,
                config,
                execution_context,
            )
            .await
        }
        "llm.infer" => {
            execute_llm_infer_action(
                pool,
                run,
                action_request_id,
                &action.args,
                config,
                execution_context,
            )
            .await
        }
        "local.exec" => execute_local_exec_action(&action.args, config).await,
        other => Err(anyhow!("unsupported action type: {}", other)),
    }
}

// --- Action execution and governance contract checks ---

#[derive(Debug, Clone)]
struct ActionExecutionContext {
    remote_llm_tokens_remaining: Option<u64>,
    payment_spend_msat: u64,
    last_llm_response_text: Option<String>,
}

fn requires_governance_approval(config: &WorkerConfig, action_type: &str) -> bool {
    if config.approval_required_action_types.is_empty() {
        return false;
    }
    let action_type = normalized_action_type(action_type);
    config
        .approval_required_action_types
        .iter()
        .any(|candidate| candidate == &action_type)
}

fn is_action_governance_approved(action: &skillrunner::ActionRequest) -> bool {
    if action
        .args
        .get("approved")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    if action.action_type == "payment.send" {
        return action
            .args
            .get("payment_approved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }
    false
}

async fn execute_object_write_action(
    pool: &DbPool,
    run: &agent_core::RunLeaseRecord,
    args: &Value,
    artifact_root: &Path,
) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("object.write args.path is required"))?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("object.write args.content is required"))?;

    let safe_rel_path = sanitize_relative_path(path)?;
    let tenant_root = tenant_scoped_artifact_root(artifact_root, &run.tenant_id)?;
    let full_path = tenant_root.join(&safe_rel_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }
    fs::write(&full_path, content)
        .with_context(|| format!("failed writing artifact {}", full_path.display()))?;

    let artifact = persist_artifact_metadata_dual(
        pool,
        &NewArtifact {
            id: Uuid::new_v4(),
            run_id: run.id,
            path: safe_rel_path.to_string_lossy().to_string(),
            content_type: "text/markdown".to_string(),
            size_bytes: content.len() as i64,
            checksum: None,
            storage_ref: full_path.to_string_lossy().to_string(),
        },
    )
    .await?;

    Ok(json!({
        "artifact_id": artifact.id,
        "path": artifact.path,
        "size_bytes": artifact.size_bytes,
        "storage_ref": artifact.storage_ref,
    }))
}

async fn execute_message_send_action(
    pool: &DbPool,
    run: &agent_core::RunLeaseRecord,
    args: &Value,
    config: &WorkerConfig,
    execution_context: &ActionExecutionContext,
) -> Result<Value> {
    let destination = args
        .get("destination")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("message.send args.destination is required"))?;
    let mut content = args
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| args.get("content").and_then(Value::as_str))
        .ok_or_else(|| anyhow!("message.send args.text (or args.content) is required"))?;
    let mut content = content.to_string();
    let placeholder = "{{llm_response}}";
    if content.contains(placeholder) {
        let llm_response = execution_context
            .last_llm_response_text
            .as_deref()
            .ok_or_else(|| anyhow!("message.send content placeholder requires prior llm.infer result"))?;
        content = content.replace(placeholder, llm_response);
    }

    let parsed_destination = ParsedMessageDestination::parse("message.send", destination)?;
    enforce_message_destination_allowlist(config, &parsed_destination, "message.send")?;
    let signer_identity = match parsed_destination.provider {
        MessageProvider::WhiteNoise => {
            Some(config.nostr_signer.resolve_identity()?.ok_or_else(|| {
                anyhow!("message.send to White Noise requires a configured Nostr signer identity")
            })?)
        }
        MessageProvider::Slack => None,
    };
    let (delivery_state, delivery_result, delivery_error, delivery_context) =
        match parsed_destination.provider {
            MessageProvider::WhiteNoise => {
                if config.nostr_relays.is_empty() {
                    (
                        "queued_local_outbox",
                        None,
                        None,
                        Some(json!({"transport":"outbox_only"})),
                    )
                } else {
                    let whitenoise_signer = signer_identity.as_ref().ok_or_else(|| {
                        anyhow!("message.send to White Noise requires a configured Nostr signer identity")
                    })?;
                    let (publish_result, publish_error, publish_context) =
                        attempt_whitenoise_publish(
                            config,
                            whitenoise_signer,
                            parsed_destination.target,
                            content,
                        )
                        .await;
                    if let Some(result) = publish_result {
                        (
                            "published_nostr",
                            Some(json!({
                                "event_id": result.event_id,
                                "accepted_relays": result.accepted_relays,
                                "relay_results": result.relay_results,
                            })),
                            None,
                            publish_context,
                        )
                    } else {
                        ("queued_local_outbox", None, publish_error, publish_context)
                    }
                }
            }
            MessageProvider::Slack => {
                attempt_slack_send(config, parsed_destination.target, content).await
            }
        };

    let outbox_message = json!({
        "provider": parsed_destination.provider.as_str(),
        "destination": destination,
        "target": parsed_destination.target,
        "text": content,
        "nostr_signer_mode": signer_identity.as_ref().map(|identity| identity.mode.as_str()),
        "nostr_public_key": signer_identity.as_ref().map(|identity| identity.public_key.as_str()),
        "delivery_state": delivery_state,
        "delivery_result": delivery_result,
        "delivery_error": delivery_error,
        "delivery_context": delivery_context,
    });
    let outbox_bytes = serde_json::to_vec_pretty(&outbox_message)
        .with_context(|| "failed serializing message.send outbox payload")?;

    let relative_path = PathBuf::from("messages")
        .join(parsed_destination.provider.as_str())
        .join(format!("{}.json", Uuid::new_v4()));
    let tenant_root = tenant_scoped_artifact_root(&config.artifact_root, &run.tenant_id)?;
    let full_path = tenant_root.join(&relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create message outbox dir {}", parent.display()))?;
    }
    fs::write(&full_path, &outbox_bytes)
        .with_context(|| format!("failed writing message outbox {}", full_path.display()))?;

    let artifact = persist_artifact_metadata_dual(
        pool,
        &NewArtifact {
            id: Uuid::new_v4(),
            run_id: run.id,
            path: relative_path.to_string_lossy().to_string(),
            content_type: "application/json".to_string(),
            size_bytes: outbox_bytes.len() as i64,
            checksum: None,
            storage_ref: full_path.to_string_lossy().to_string(),
        },
    )
    .await?;

    Ok(json!({
        "provider": parsed_destination.provider.as_str(),
        "destination": destination,
        "delivery_state": delivery_state,
        "artifact_id": artifact.id,
        "path": artifact.path,
        "size_bytes": artifact.size_bytes,
        "storage_ref": artifact.storage_ref,
        "nostr_public_key": signer_identity.as_ref().map(|identity| identity.public_key.as_str()),
        "delivery_result": delivery_result,
        "delivery_error": delivery_error,
        "delivery_context": delivery_context,
    }))
}

async fn execute_message_receive_action(
    pool: &DbPool,
    run: &agent_core::RunLeaseRecord,
    args: &Value,
    config: &WorkerConfig,
) -> Result<Value> {
    let source = args
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("message.receive args.source is required"))?;
    let content = args
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| args.get("content").and_then(Value::as_str))
        .ok_or_else(|| {
            anyhow!("message.receive args.text (or args.content) is required")
        })?;

    let parsed_source = ParsedMessageDestination::parse("message.receive", source)?;
    enforce_message_destination_allowlist(config, &parsed_source, "message.receive")?;

    let receive_message = json!({
        "direction": "inbound",
        "provider": parsed_source.provider.as_str(),
        "source": source,
        "target": parsed_source.target,
        "text": content,
    });
    let receive_bytes = serde_json::to_vec_pretty(&receive_message)
        .with_context(|| "failed serializing message.receive payload")?;

    let relative_path = PathBuf::from("messages")
        .join(parsed_source.provider.as_str())
        .join("received")
        .join(format!("{}.json", Uuid::new_v4()));
    let tenant_root = tenant_scoped_artifact_root(&config.artifact_root, &run.tenant_id)?;
    let full_path = tenant_root.join(&relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create message receive dir {}", parent.display()))?;
    }
    fs::write(&full_path, &receive_bytes)
        .with_context(|| format!("failed writing message receive artifact {}", full_path.display()))?;

    let artifact = persist_artifact_metadata_dual(
        pool,
        &NewArtifact {
            id: Uuid::new_v4(),
            run_id: run.id,
            path: relative_path.to_string_lossy().to_string(),
            content_type: "application/json".to_string(),
            size_bytes: receive_bytes.len() as i64,
            checksum: None,
            storage_ref: full_path.to_string_lossy().to_string(),
        },
    )
    .await?;

    Ok(json!({
        "direction": "inbound",
        "provider": parsed_source.provider.as_str(),
        "source": source,
        "artifact_id": artifact.id,
        "path": artifact.path,
        "size_bytes": artifact.size_bytes,
        "storage_ref": artifact.storage_ref,
    }))
}

fn enforce_message_destination_allowlist(
    config: &WorkerConfig,
    destination: &ParsedMessageDestination<'_>,
    action_type: &str,
) -> Result<()> {
    let (allowlist, provider_name) = match destination.provider {
        MessageProvider::WhiteNoise => (
            &config.message_whitenoise_destination_allowlist,
            "whitenoise",
        ),
        MessageProvider::Slack => (&config.message_slack_destination_allowlist, "slack"),
    };
    if allowlist.is_empty() {
        return Ok(());
    }
    let target = destination.target.trim();
    if allowlist.iter().any(|candidate| candidate.trim() == target) {
        return Ok(());
    }
    Err(anyhow!(
        "{} target `{}` is not allowlisted for provider `{}`",
        action_type,
        target,
        provider_name
    ))
}

async fn execute_payment_send_action(
    pool: &DbPool,
    run: &agent_core::RunLeaseRecord,
    action_request_id: Uuid,
    args: &Value,
    config: &WorkerConfig,
    execution_context: &mut ActionExecutionContext,
) -> Result<Value> {
    let destination = args
        .get("destination")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("payment.send args.destination is required"))?;
    let parsed_destination = ParsedPaymentDestination::parse(destination)?;
    let operation = PaymentOperation::parse(
        args.get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("payment.send args.operation is required"))?,
    )?;
    let idempotency_key = args
        .get("idempotency_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("payment.send args.idempotency_key is required"))?;
    let amount_msat_u64 = args.get("amount_msat").and_then(Value::as_u64);
    let payment_approved = args
        .get("payment_approved")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if operation.requires_amount() && amount_msat_u64.is_none() {
        return Err(anyhow!(
            "payment.send args.amount_msat is required for {}",
            operation.as_str()
        ));
    }
    let amount_msat_i64 = amount_msat_u64.map(|value| value as i64);
    let request_payload = json!({
        "destination": destination,
        "operation": operation.as_str(),
        "idempotency_key": idempotency_key,
        "amount_msat": amount_msat_u64,
        "payment_approved": payment_approved,
        "invoice": args.get("invoice"),
        "description": args.get("description"),
    });

    let payment_request = create_or_get_payment_request_dual(
        pool,
        &NewPaymentRequest {
            id: Uuid::new_v4(),
            action_request_id,
            run_id: run.id,
            tenant_id: run.tenant_id.clone(),
            agent_id: run.agent_id,
            provider: parsed_destination.provider.as_str().to_string(),
            operation: operation.as_str().to_string(),
            destination: destination.to_string(),
            idempotency_key: idempotency_key.to_string(),
            amount_msat: amount_msat_i64,
            request_json: request_payload.clone(),
            status: "requested".to_string(),
        },
    )
    .await?;

    let is_duplicate = payment_request.action_request_id != action_request_id;
    if is_duplicate {
        let prior_result = get_latest_payment_result_dual(pool, payment_request.id).await?;
        return Ok(json!({
            "provider": parsed_destination.provider.as_str(),
            "destination": destination,
            "operation": operation.as_str(),
            "status": "duplicate",
            "payment_request_id": payment_request.id,
            "idempotency_key": payment_request.idempotency_key,
            "prior_request_status": payment_request.status,
            "prior_result_status": prior_result.as_ref().map(|record| record.status.clone()),
            "prior_result": prior_result.and_then(|record| record.result_json),
        }));
    }

    if matches!(operation, PaymentOperation::PayInvoice) {
        if let (Some(threshold), Some(amount_msat)) =
            (config.payment_approval_threshold_msat, amount_msat_u64)
        {
            if amount_msat >= threshold && !payment_approved {
                let message = format!(
                    "payment.send requires approval for amount {} msat (threshold={})",
                    amount_msat, threshold
                );
                persist_failed_payment_request(
                    pool,
                    payment_request.id,
                    "PAYMENT_APPROVAL_REQUIRED",
                    &message,
                )
                .await;
                return Err(anyhow!(message));
            }
        }
        if let (Some(limit), Some(amount_msat)) =
            (config.payment_max_spend_msat_per_run, amount_msat_u64)
        {
            if execution_context
                .payment_spend_msat
                .saturating_add(amount_msat)
                > limit
            {
                let message = format!(
                    "payment.send run spend budget exceeded (remaining={}, requested={})",
                    limit.saturating_sub(execution_context.payment_spend_msat),
                    amount_msat
                );
                persist_failed_payment_request(
                    pool,
                    payment_request.id,
                    "PAYMENT_RUN_BUDGET_EXCEEDED",
                    &message,
                )
                .await;
                return Err(anyhow!(message));
            }
        }
        if let (Some(limit), Some(amount_msat)) =
            (config.payment_max_spend_msat_per_tenant, amount_msat_u64)
        {
            let tenant_spent =
                sum_executed_payment_amount_msat_for_tenant_dual(pool, &run.tenant_id)
                    .await?
                    .max(0) as u64;
            if tenant_spent.saturating_add(amount_msat) > limit {
                let message = format!(
                    "payment.send tenant spend budget exceeded (remaining={}, requested={})",
                    limit.saturating_sub(tenant_spent),
                    amount_msat
                );
                persist_failed_payment_request(
                    pool,
                    payment_request.id,
                    "PAYMENT_TENANT_BUDGET_EXCEEDED",
                    &message,
                )
                .await;
                return Err(anyhow!(message));
            }
        }
        if let (Some(limit), Some(amount_msat)) =
            (config.payment_max_spend_msat_per_agent, amount_msat_u64)
        {
            let agent_spent =
                sum_executed_payment_amount_msat_for_agent_dual(pool, &run.tenant_id, run.agent_id)
                    .await?
                    .max(0) as u64;
            if agent_spent.saturating_add(amount_msat) > limit {
                let message = format!(
                    "payment.send agent spend budget exceeded (remaining={}, requested={})",
                    limit.saturating_sub(agent_spent),
                    amount_msat
                );
                persist_failed_payment_request(
                    pool,
                    payment_request.id,
                    "PAYMENT_AGENT_BUDGET_EXCEEDED",
                    &message,
                )
                .await;
                return Err(anyhow!(message));
            }
        }
    }

    if matches!(parsed_destination.provider, PaymentProvider::Cashu) {
        let execution_output = execute_cashu_payment_scaffold(
            pool,
            &parsed_destination,
            operation,
            idempotency_key,
            payment_request.id,
            amount_msat_u64,
            args,
            config,
        )
        .await?;
        let payment_result = create_payment_result_dual(
            pool,
            &NewPaymentResult {
                id: Uuid::new_v4(),
                payment_request_id: payment_request.id,
                status: "executed".to_string(),
                result_json: Some(execution_output.clone()),
                error_json: None,
            },
        )
        .await?;
        let _ = update_payment_request_status_dual(pool, payment_request.id, "executed").await;

        if matches!(operation, PaymentOperation::PayInvoice) {
            execution_context.payment_spend_msat = execution_context
                .payment_spend_msat
                .saturating_add(amount_msat_u64.unwrap_or(0));
        }

        let outbox_record = json!({
            "provider": parsed_destination.provider.as_str(),
            "target": parsed_destination.target,
            "operation": operation.as_str(),
            "request": request_payload,
            "result": execution_output,
        });
        let outbox_bytes = serde_json::to_vec_pretty(&outbox_record)
            .with_context(|| "failed serializing payment.send outbox payload")?;
        let relative_path = PathBuf::from("payments")
            .join(parsed_destination.provider.as_str())
            .join(format!("{}.json", Uuid::new_v4()));
        let tenant_root = tenant_scoped_artifact_root(&config.artifact_root, &run.tenant_id)?;
        let full_path = tenant_root.join(&relative_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create payment outbox dir {}", parent.display())
            })?;
        }
        fs::write(&full_path, &outbox_bytes)
            .with_context(|| format!("failed writing payment outbox {}", full_path.display()))?;
        let artifact = persist_artifact_metadata_dual(
            pool,
            &NewArtifact {
                id: Uuid::new_v4(),
                run_id: run.id,
                path: relative_path.to_string_lossy().to_string(),
                content_type: "application/json".to_string(),
                size_bytes: outbox_bytes.len() as i64,
                checksum: None,
                storage_ref: full_path.to_string_lossy().to_string(),
            },
        )
        .await?;

        return Ok(json!({
            "provider": parsed_destination.provider.as_str(),
            "destination": destination,
            "operation": operation.as_str(),
            "status": "executed",
            "payment_request_id": payment_request.id,
            "payment_result_id": payment_result.id,
            "idempotency_key": payment_request.idempotency_key,
            "token_accounting": {
                "payment_spend_msat": execution_context.payment_spend_msat,
                "payment_budget_msat": config.payment_max_spend_msat_per_run,
                "payment_approval_threshold_msat": config.payment_approval_threshold_msat,
                "payment_budget_tenant_msat": config.payment_max_spend_msat_per_tenant,
                "payment_budget_agent_msat": config.payment_max_spend_msat_per_agent,
            },
            "result": execution_output,
            "artifact_id": artifact.id,
            "path": artifact.path,
            "storage_ref": artifact.storage_ref,
        }));
    }

    if !config.payment_nwc_enabled {
        let error_message = "payment.send is disabled; set PAYMENT_NWC_ENABLED=1".to_string();
        persist_failed_payment_request(
            pool,
            payment_request.id,
            "PAYMENT_DISABLED",
            &error_message,
        )
        .await;
        return Err(anyhow!(error_message));
    }

    if parsed_destination.target.contains("nostr+walletconnect://") {
        let message = "payment.send destination target must be a logical wallet id; configure PAYMENT_NWC_URI/PAYMENT_NWC_URI_REF for credentials".to_string();
        persist_failed_payment_request(
            pool,
            payment_request.id,
            "PAYMENT_INVALID_DESTINATION",
            &message,
        )
        .await;
        return Err(anyhow!(message));
    }

    let resolved_routes =
        resolve_nwc_uris_for_wallet(config, parsed_destination.target, idempotency_key);
    let candidate_nwc_uris = resolved_routes.candidates;
    if candidate_nwc_uris.is_empty() && !config.payment_nwc_wallet_uris.is_empty() {
        let message = format!(
            "payment wallet `{}` is not configured; set PAYMENT_NWC_WALLET_URIS/PAYMENT_NWC_WALLET_URIS_REF (or wildcard `*`)",
            parsed_destination.target
        );
        persist_failed_payment_request(
            pool,
            payment_request.id,
            "PAYMENT_WALLET_NOT_CONFIGURED",
            &message,
        )
        .await;
        return Err(anyhow!(message));
    }

    let execution_output = if !candidate_nwc_uris.is_empty() {
        let request = build_nwc_request(operation, args, amount_msat_u64)?;
        let mut route_errors = Vec::new();
        let mut selected_route = None;
        let mut nwc_outcome = None;
        let mut attempted_routes = 0usize;
        let mut skipped_unhealthy_routes = 0usize;
        for (route_index, nwc_uri) in candidate_nwc_uris.iter().enumerate() {
            if !should_attempt_payment_route(
                config,
                "nwc",
                parsed_destination.target,
                nwc_uri,
                config.payment_nwc_route_health_fail_threshold,
                config.payment_nwc_route_health_cooldown,
            ) {
                skipped_unhealthy_routes = skipped_unhealthy_routes.saturating_add(1);
                continue;
            }
            attempted_routes = attempted_routes.saturating_add(1);
            match send_nwc_request(nwc_uri.as_str(), &request, config.payment_nwc_timeout).await {
                Ok(outcome) => {
                    mark_payment_route_success(config, "nwc", parsed_destination.target, nwc_uri);
                    selected_route = Some(route_index + 1);
                    nwc_outcome = Some(outcome);
                    break;
                }
                Err(error) => {
                    mark_payment_route_failure(config, "nwc", parsed_destination.target, nwc_uri);
                    route_errors.push(format!("route {}: {:#}", route_index + 1, error));
                    if !config.payment_nwc_route_fallback_enabled {
                        break;
                    }
                }
            }
        }

        let nwc_outcome = match nwc_outcome {
            Some(outcome) => outcome,
            None => {
                let message = if route_errors.is_empty() && skipped_unhealthy_routes > 0 {
                    format!(
                        "payment.send all candidate routes are temporarily unhealthy (skipped={})",
                        skipped_unhealthy_routes
                    )
                } else if skipped_unhealthy_routes > 0 {
                    format!(
                        "{} | skipped_unhealthy_routes={}",
                        route_errors.join(" | "),
                        skipped_unhealthy_routes
                    )
                } else {
                    route_errors.join(" | ")
                };
                persist_failed_payment_request(
                    pool,
                    payment_request.id,
                    "PAYMENT_NWC_REQUEST_FAILED",
                    &message,
                )
                .await;
                return Err(anyhow!(message));
            }
        };
        let route_meta = json!({
            "strategy": config.payment_nwc_route_strategy.as_str(),
            "fallback_enabled": config.payment_nwc_route_fallback_enabled,
            "rollout_percent": config.payment_nwc_route_rollout_percent,
            "rollout_limited": resolved_routes.rollout_limited,
            "candidate_count": candidate_nwc_uris.len(),
            "attempted_count": attempted_routes,
            "skipped_unhealthy_count": skipped_unhealthy_routes,
            "selected_route_index": selected_route,
            "error_count": route_errors.len(),
            "health_fail_threshold": config.payment_nwc_route_health_fail_threshold,
            "health_cooldown_secs": config.payment_nwc_route_health_cooldown.as_secs(),
            "errors": route_errors,
        });

        match operation {
            PaymentOperation::PayInvoice => {
                let pay_result = match nwc_outcome.response.clone().to_pay_invoice() {
                    Ok(result) => result,
                    Err(error) => {
                        let message = format!("{error:#}");
                        persist_failed_payment_request(
                            pool,
                            payment_request.id,
                            "PAYMENT_NWC_RESPONSE_ERROR",
                            &message,
                        )
                        .await;
                        return Err(anyhow!(message));
                    }
                };
                json!({
                    "settlement_status": "settled",
                    "payment_preimage": pay_result.preimage,
                    "amount_msat": amount_msat_u64.unwrap_or(0),
                    "fee_msat": pay_result.fees_paid.unwrap_or(0),
                    "wallet": parsed_destination.target,
                    "rail": "nwc_nip47",
                    "nwc": {
                        "relay": nwc_outcome.relay,
                        "request_event_id": nwc_outcome.request_event_id,
                        "response_event_id": nwc_outcome.response_event_id,
                        "route": route_meta,
                    },
                })
            }
            PaymentOperation::MakeInvoice => {
                let invoice_result = match nwc_outcome.response.clone().to_make_invoice() {
                    Ok(result) => result,
                    Err(error) => {
                        let message = format!("{error:#}");
                        persist_failed_payment_request(
                            pool,
                            payment_request.id,
                            "PAYMENT_NWC_RESPONSE_ERROR",
                            &message,
                        )
                        .await;
                        return Err(anyhow!(message));
                    }
                };
                json!({
                    "invoice": invoice_result.invoice,
                    "payment_hash": invoice_result.payment_hash,
                    "amount_msat": invoice_result.amount.unwrap_or(amount_msat_u64.unwrap_or(0)),
                    "wallet": parsed_destination.target,
                    "rail": "nwc_nip47",
                    "nwc": {
                        "relay": nwc_outcome.relay,
                        "request_event_id": nwc_outcome.request_event_id,
                        "response_event_id": nwc_outcome.response_event_id,
                        "route": route_meta,
                    },
                })
            }
            PaymentOperation::GetBalance => {
                let balance_result: GetBalanceResponse =
                    match nwc_outcome.response.clone().to_get_balance() {
                        Ok(result) => result,
                        Err(error) => {
                            let message = format!("{error:#}");
                            persist_failed_payment_request(
                                pool,
                                payment_request.id,
                                "PAYMENT_NWC_RESPONSE_ERROR",
                                &message,
                            )
                            .await;
                            return Err(anyhow!(message));
                        }
                    };
                json!({
                    "balance_msat": balance_result.balance,
                    "wallet": parsed_destination.target,
                    "rail": "nwc_nip47",
                    "nwc": {
                        "relay": nwc_outcome.relay,
                        "request_event_id": nwc_outcome.request_event_id,
                        "response_event_id": nwc_outcome.response_event_id,
                        "route": route_meta,
                    },
                })
            }
        }
    } else {
        match operation {
            PaymentOperation::PayInvoice => json!({
                "settlement_status": "settled",
                "payment_hash": format!("mock-hash-{}", payment_request.id),
                "payment_preimage": format!("mock-preimage-{}", payment_request.id),
                "amount_msat": amount_msat_u64.unwrap_or(0),
                "fee_msat": 0,
                "wallet": parsed_destination.target,
                "rail": "nwc_mock",
            }),
            PaymentOperation::MakeInvoice => json!({
                "invoice": format!("lnbc{}n1pmock{}", amount_msat_u64.unwrap_or(0), payment_request.id.simple()),
                "amount_msat": amount_msat_u64.unwrap_or(0),
                "wallet": parsed_destination.target,
                "rail": "nwc_mock",
            }),
            PaymentOperation::GetBalance => json!({
                "balance_msat": config.payment_nwc_mock_balance_msat,
                "wallet": parsed_destination.target,
                "rail": "nwc_mock",
            }),
        }
    };

    let payment_result = create_payment_result_dual(
        pool,
        &NewPaymentResult {
            id: Uuid::new_v4(),
            payment_request_id: payment_request.id,
            status: "executed".to_string(),
            result_json: Some(execution_output.clone()),
            error_json: None,
        },
    )
    .await?;
    let _ = update_payment_request_status_dual(pool, payment_request.id, "executed").await;

    if matches!(operation, PaymentOperation::PayInvoice) {
        execution_context.payment_spend_msat = execution_context
            .payment_spend_msat
            .saturating_add(amount_msat_u64.unwrap_or(0));
    }

    let outbox_record = json!({
        "provider": parsed_destination.provider.as_str(),
        "target": parsed_destination.target,
        "operation": operation.as_str(),
        "request": request_payload,
        "result": execution_output,
    });
    let outbox_bytes = serde_json::to_vec_pretty(&outbox_record)
        .with_context(|| "failed serializing payment.send outbox payload")?;
    let relative_path = PathBuf::from("payments")
        .join(parsed_destination.provider.as_str())
        .join(format!("{}.json", Uuid::new_v4()));
    let tenant_root = tenant_scoped_artifact_root(&config.artifact_root, &run.tenant_id)?;
    let full_path = tenant_root.join(&relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create payment outbox dir {}", parent.display()))?;
    }
    fs::write(&full_path, &outbox_bytes)
        .with_context(|| format!("failed writing payment outbox {}", full_path.display()))?;
    let artifact = persist_artifact_metadata_dual(
        pool,
        &NewArtifact {
            id: Uuid::new_v4(),
            run_id: run.id,
            path: relative_path.to_string_lossy().to_string(),
            content_type: "application/json".to_string(),
            size_bytes: outbox_bytes.len() as i64,
            checksum: None,
            storage_ref: full_path.to_string_lossy().to_string(),
        },
    )
    .await?;

    Ok(json!({
        "provider": parsed_destination.provider.as_str(),
        "destination": destination,
        "operation": operation.as_str(),
        "status": "executed",
        "payment_request_id": payment_request.id,
        "payment_result_id": payment_result.id,
        "idempotency_key": payment_request.idempotency_key,
        "token_accounting": {
            "payment_spend_msat": execution_context.payment_spend_msat,
            "payment_budget_msat": config.payment_max_spend_msat_per_run,
            "payment_approval_threshold_msat": config.payment_approval_threshold_msat,
            "payment_budget_tenant_msat": config.payment_max_spend_msat_per_tenant,
            "payment_budget_agent_msat": config.payment_max_spend_msat_per_agent,
        },
        "result": execution_output,
        "artifact_id": artifact.id,
        "path": artifact.path,
        "storage_ref": artifact.storage_ref,
    }))
}

async fn execute_cashu_payment_scaffold(
    pool: &DbPool,
    parsed_destination: &ParsedPaymentDestination<'_>,
    operation: PaymentOperation,
    idempotency_key: &str,
    payment_request_id: Uuid,
    amount_msat: Option<u64>,
    args: &Value,
    config: &WorkerConfig,
) -> Result<Value> {
    if !config.payment_cashu_enabled {
        let error_message =
            "cashu rail is disabled; set PAYMENT_CASHU_ENABLED=1 to enable scaffold".to_string();
        persist_failed_payment_request(
            pool,
            payment_request_id,
            "PAYMENT_CASHU_DISABLED",
            &error_message,
        )
        .await;
        return Err(anyhow!(error_message));
    }

    if let (Some(limit), Some(amount)) = (config.payment_cashu_max_spend_msat_per_run, amount_msat)
    {
        if amount > limit {
            let message = format!(
                "cashu scaffold run spend budget exceeded (limit={}, requested={})",
                limit, amount
            );
            persist_failed_payment_request(
                pool,
                payment_request_id,
                "PAYMENT_CASHU_RUN_BUDGET_EXCEEDED",
                &message,
            )
            .await;
            return Err(anyhow!(message));
        }
    }

    if config.payment_cashu_mint_uris.is_empty() {
        let message =
            "cashu scaffold requires mint routing; set PAYMENT_CASHU_MINT_URIS/PAYMENT_CASHU_MINT_URIS_REF"
                .to_string();
        persist_failed_payment_request(
            pool,
            payment_request_id,
            "PAYMENT_CASHU_MINTS_NOT_CONFIGURED",
            &message,
        )
        .await;
        return Err(anyhow!(message));
    }

    let resolved_routes =
        resolve_cashu_mint_uris(config, parsed_destination.target, idempotency_key);
    let candidate_mint_uris = resolved_routes.candidates;
    if candidate_mint_uris.is_empty() {
        let message = format!(
            "cashu mint `{}` is not configured; set PAYMENT_CASHU_MINT_URIS/PAYMENT_CASHU_MINT_URIS_REF or PAYMENT_CASHU_DEFAULT_MINT",
            parsed_destination.target
        );
        persist_failed_payment_request(
            pool,
            payment_request_id,
            "PAYMENT_CASHU_MINT_NOT_CONFIGURED",
            &message,
        )
        .await;
        return Err(anyhow!(message));
    }

    let build_route_meta = |selected_route: Option<usize>,
                            attempted_routes: usize,
                            skipped_unhealthy_routes: usize,
                            route_errors: Vec<String>| {
        json!({
            "strategy": config.payment_cashu_route_strategy.as_str(),
            "fallback_enabled": config.payment_cashu_route_fallback_enabled,
            "rollout_percent": config.payment_cashu_route_rollout_percent,
            "rollout_limited": resolved_routes.rollout_limited,
            "candidate_count": candidate_mint_uris.len(),
            "attempted_count": attempted_routes,
            "skipped_unhealthy_count": skipped_unhealthy_routes,
            "selected_route_index": selected_route,
            "error_count": route_errors.len(),
            "health_fail_threshold": config.payment_cashu_route_health_fail_threshold,
            "health_cooldown_secs": config.payment_cashu_route_health_cooldown.as_secs(),
            "errors": route_errors,
        })
    };

    let mint_uri = candidate_mint_uris
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("no cashu mint URIs were resolved for scaffolding"))?;

    if config.payment_cashu_mock_enabled {
        let route_meta = build_route_meta(Some(1), 1, 0, Vec::new());
        let mock_result = match operation {
            PaymentOperation::PayInvoice => json!({
                "settlement_status": "settled",
                "payment_hash": format!("cashu-mock-hash-{}", payment_request_id.simple()),
                "payment_preimage": format!("cashu-mock-preimage-{}", payment_request_id.simple()),
                "amount_msat": amount_msat.unwrap_or(0),
                "fee_msat": 0,
                "mint_id": parsed_destination.target,
                "mint_uri": mint_uri,
                "rail": "cashu_mock",
                "route": route_meta,
                "mock_mode": true,
            }),
            PaymentOperation::MakeInvoice => json!({
                "invoice": format!("cashu-invoice-{}", payment_request_id.simple()),
                "amount_msat": amount_msat.unwrap_or(0),
                "mint_id": parsed_destination.target,
                "mint_uri": mint_uri,
                "rail": "cashu_mock",
                "route": route_meta,
                "mock_mode": true,
            }),
            PaymentOperation::GetBalance => json!({
                "balance_msat": config.payment_cashu_mock_balance_msat,
                "mint_id": parsed_destination.target,
                "mint_uri": mint_uri,
                "rail": "cashu_mock",
                "route": route_meta,
                "mock_mode": true,
            }),
        };
        return Ok(mock_result);
    }

    if !config.payment_cashu_http_enabled {
        let message = format!(
            "cashu rail live transport is disabled; set PAYMENT_CASHU_HTTP_ENABLED=1 (operation={}, mint={})",
            operation.as_str(),
            parsed_destination.target
        );
        let details = json!({
            "provider": parsed_destination.provider.as_str(),
            "rail": "cashu_http",
            "mint_id": parsed_destination.target,
            "mint_uri": mint_uri,
            "operation": operation.as_str(),
            "amount_msat": amount_msat,
            "timeout_ms": config.payment_cashu_timeout.as_millis(),
            "route": build_route_meta(Some(1), 0, 0, Vec::new()),
        });
        persist_failed_payment_request(
            pool,
            payment_request_id,
            "PAYMENT_CASHU_HTTP_DISABLED",
            &message,
        )
        .await;

        return Err(anyhow!("{} ({})", message, details));
    }

    let mut route_errors = Vec::new();
    let mut selected_route = None;
    let mut attempted_routes = 0usize;
    let mut skipped_unhealthy_routes = 0usize;
    let mut execution_output = None;
    for (route_index, mint_uri) in candidate_mint_uris.iter().enumerate() {
        if !should_attempt_payment_route(
            config,
            "cashu",
            parsed_destination.target,
            mint_uri,
            config.payment_cashu_route_health_fail_threshold,
            config.payment_cashu_route_health_cooldown,
        ) {
            skipped_unhealthy_routes = skipped_unhealthy_routes.saturating_add(1);
            continue;
        }

        attempted_routes = attempted_routes.saturating_add(1);
        match execute_cashu_payment_http(
            parsed_destination,
            operation,
            payment_request_id,
            amount_msat,
            args,
            mint_uri.as_str(),
            config,
        )
        .await
        {
            Ok(result) => {
                mark_payment_route_success(config, "cashu", parsed_destination.target, mint_uri);
                selected_route = Some(route_index + 1);
                execution_output = Some(result);
                break;
            }
            Err(error) => {
                mark_payment_route_failure(config, "cashu", parsed_destination.target, mint_uri);
                route_errors.push(format!("route {}: {:#}", route_index + 1, error));
                if !config.payment_cashu_route_fallback_enabled {
                    break;
                }
            }
        }
    }

    let mut execution_output = match execution_output {
        Some(result) => result,
        None => {
            let message = if route_errors.is_empty() && skipped_unhealthy_routes > 0 {
                format!(
                    "cashu payment all candidate routes are temporarily unhealthy (skipped={})",
                    skipped_unhealthy_routes
                )
            } else if skipped_unhealthy_routes > 0 {
                format!(
                    "{} | skipped_unhealthy_routes={}",
                    route_errors.join(" | "),
                    skipped_unhealthy_routes
                )
            } else {
                route_errors.join(" | ")
            };
            let message = redact_text(message.as_str());
            persist_failed_payment_request(
                pool,
                payment_request_id,
                "PAYMENT_CASHU_HTTP_FAILED",
                &message,
            )
            .await;
            return Err(anyhow!(message));
        }
    };
    let route_meta = build_route_meta(
        selected_route,
        attempted_routes,
        skipped_unhealthy_routes,
        route_errors,
    );
    if let Value::Object(ref mut map) = execution_output {
        map.insert("route".to_string(), route_meta);
    }
    Ok(execution_output)
}

async fn execute_cashu_payment_http(
    parsed_destination: &ParsedPaymentDestination<'_>,
    operation: PaymentOperation,
    payment_request_id: Uuid,
    amount_msat: Option<u64>,
    args: &Value,
    mint_uri: &str,
    config: &WorkerConfig,
) -> Result<Value> {
    let endpoint_path = match operation {
        PaymentOperation::PayInvoice => "v1/pay_invoice",
        PaymentOperation::MakeInvoice => "v1/make_invoice",
        PaymentOperation::GetBalance => "v1/balance",
    };
    let endpoint = build_cashu_endpoint_url(
        mint_uri,
        endpoint_path,
        config.payment_cashu_http_allow_insecure,
    )?;
    let client = reqwest::Client::builder()
        .timeout(config.payment_cashu_timeout)
        .build()
        .context("failed building Cashu HTTP client")?;

    let request_builder = match operation {
        PaymentOperation::GetBalance => client
            .get(endpoint.as_str())
            .header("accept", "application/json"),
        PaymentOperation::PayInvoice | PaymentOperation::MakeInvoice => client
            .post(endpoint.as_str())
            .header("content-type", "application/json"),
    }
    .header("x-secureagnt-payment-rail", "cashu_http")
    .header("x-secureagnt-mint-id", parsed_destination.target);
    let request_builder = with_optional_auth_header(
        request_builder,
        config.payment_cashu_auth_header.as_str(),
        config.payment_cashu_auth_token.as_deref(),
    );

    let request_body = match operation {
        PaymentOperation::PayInvoice => {
            let invoice = args
                .get("invoice")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("payment.send args.invoice is required for pay_invoice"))?;
            Some(json!({
                "operation": operation.as_str(),
                "invoice": invoice,
                "amount_msat": amount_msat.unwrap_or(0),
                "idempotency_key": payment_request_id.to_string(),
                "mint_id": parsed_destination.target,
            }))
        }
        PaymentOperation::MakeInvoice => Some(json!({
            "operation": operation.as_str(),
            "amount_msat": amount_msat.unwrap_or(0),
            "description": args.get("description"),
            "idempotency_key": payment_request_id.to_string(),
            "mint_id": parsed_destination.target,
        })),
        PaymentOperation::GetBalance => None,
    };

    let response = match request_body {
        Some(ref payload) => request_builder
            .body(payload.to_string())
            .send()
            .await
            .context("cashu HTTP request failed")?,
        None => request_builder
            .send()
            .await
            .context("cashu HTTP request failed")?,
    };
    let http_status = response.status().as_u16() as i32;
    let response_text = response
        .text()
        .await
        .context("failed reading cashu HTTP response body")?;
    if !(200..=299).contains(&http_status) {
        let truncated = response_text.chars().take(512).collect::<String>();
        return Err(anyhow!(
            "cashu HTTP request failed: status={} body={}",
            http_status,
            truncated
        ));
    }

    let upstream_json =
        serde_json::from_str::<Value>(response_text.as_str()).unwrap_or_else(|_| {
            json!({
                "raw_body": response_text,
            })
        });

    let normalized_result = match operation {
        PaymentOperation::PayInvoice => json!({
            "settlement_status": extract_string_field(&upstream_json, &["settlement_status", "status"])
                .unwrap_or_else(|| "settled".to_string()),
            "payment_hash": extract_string_field(&upstream_json, &["payment_hash", "hash"]),
            "payment_preimage": extract_string_field(&upstream_json, &["payment_preimage", "preimage"]),
            "amount_msat": amount_msat.unwrap_or(0),
            "fee_msat": extract_u64_field(&upstream_json, &["fee_msat", "fees_paid"]).unwrap_or(0),
        }),
        PaymentOperation::MakeInvoice => {
            let invoice = extract_string_field(&upstream_json, &["invoice", "bolt11"])
                .ok_or_else(|| anyhow!("cashu HTTP make_invoice response missing invoice"))?;
            json!({
                "invoice": invoice,
                "payment_hash": extract_string_field(&upstream_json, &["payment_hash", "hash"]),
                "amount_msat": extract_u64_field(&upstream_json, &["amount_msat", "amount"])
                    .or(amount_msat)
                    .unwrap_or(0),
            })
        }
        PaymentOperation::GetBalance => {
            let balance_msat = extract_u64_field(&upstream_json, &["balance_msat", "balance"])
                .ok_or_else(|| anyhow!("cashu HTTP get_balance response missing balance"))?;
            json!({
                "balance_msat": balance_msat,
            })
        }
    };

    Ok(json!({
        "rail": "cashu_http",
        "mint_id": parsed_destination.target,
        "mint_uri": mint_uri,
        "operation": operation.as_str(),
        "amount_msat": amount_msat,
        "http_status": http_status,
        "upstream": upstream_json,
        "result": normalized_result,
    }))
}

fn extract_string_field(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        payload
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn extract_u64_field(payload: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        payload.get(*key).and_then(|value| {
            value
                .as_u64()
                .or_else(|| {
                    value
                        .as_i64()
                        .and_then(|signed| (signed >= 0).then_some(signed as u64))
                })
                .or_else(|| {
                    value
                        .as_str()
                        .map(str::trim)
                        .filter(|raw| !raw.is_empty())
                        .and_then(|raw| raw.parse::<u64>().ok())
                })
        })
    })
}

fn build_cashu_endpoint_url(
    mint_uri: &str,
    endpoint_path: &str,
    allow_insecure_http: bool,
) -> Result<String> {
    let normalized_base = if mint_uri.ends_with('/') {
        mint_uri.to_string()
    } else {
        format!("{mint_uri}/")
    };
    let base_url = reqwest::Url::parse(normalized_base.as_str())
        .with_context(|| format!("invalid Cashu mint URI `{mint_uri}`"))?;
    match base_url.scheme() {
        "https" => {}
        "http" if allow_insecure_http => {}
        "http" => {
            return Err(anyhow!(
                "Cashu mint URI `{mint_uri}` is insecure; set PAYMENT_CASHU_HTTP_ALLOW_INSECURE=1 only for local/dev usage"
            ));
        }
        other => {
            return Err(anyhow!(
                "Cashu mint URI `{mint_uri}` uses unsupported scheme `{other}` (expected https, or http when PAYMENT_CASHU_HTTP_ALLOW_INSECURE=1)"
            ));
        }
    }
    let endpoint = base_url
        .join(endpoint_path)
        .with_context(|| format!("failed joining endpoint `{endpoint_path}` to `{mint_uri}`"))?;
    Ok(endpoint.to_string())
}

async fn persist_failed_payment_request(
    pool: &DbPool,
    payment_request_id: Uuid,
    code: &str,
    message: &str,
) {
    let _ = create_payment_result_dual(
        pool,
        &NewPaymentResult {
            id: Uuid::new_v4(),
            payment_request_id,
            status: "failed".to_string(),
            result_json: None,
            error_json: Some(json!({
                "code": code,
                "message": message,
            })),
        },
    )
    .await;
    let _ = update_payment_request_status_dual(pool, payment_request_id, "failed").await;
}

async fn execute_local_exec_action(args: &Value, config: &WorkerConfig) -> Result<Value> {
    let result = execute_local_exec(args, &config.local_exec).await?;
    Ok(json!({
        "template_id": result.template_id,
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
    }))
}

fn infer_run_llm_channel(input: &Value) -> Option<String> {
    let direct = input
        .get("llm_channel")
        .or_else(|| input.get("channel"))
        .and_then(Value::as_str);
    if let Some(channel) = direct.and_then(llm::normalize_channel_name) {
        return Some(channel);
    }

    let trigger_channel = input
        .get("_trigger")
        .and_then(Value::as_object)
        .and_then(|trigger| trigger.get("channel"))
        .and_then(Value::as_str);
    if let Some(channel) = trigger_channel.and_then(llm::normalize_channel_name) {
        return Some(channel);
    }

    let event_channel = input
        .get("event_payload")
        .and_then(Value::as_object)
        .and_then(|payload| payload.get("channel"))
        .and_then(Value::as_str);
    event_channel.and_then(llm::normalize_channel_name)
}

async fn execute_llm_infer_action(
    pool: &DbPool,
    run: &agent_core::RunLeaseRecord,
    action_request_id: Uuid,
    args: &Value,
    config: &WorkerConfig,
    execution_context: &mut ActionExecutionContext,
) -> Result<Value> {
    let mut effective_args = args.clone();
    if effective_args.get("request_class").is_none() {
        if let Some(queue_class) = run
            .input_json
            .get("queue_class")
            .or_else(|| run.input_json.get("llm_queue_class"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(obj) = effective_args.as_object_mut() {
                obj.insert(
                    "request_class".to_string(),
                    Value::String(queue_class.to_string()),
                );
            }
        }
    }
    if effective_args.get("channel").is_none() {
        if let Some(channel) = infer_run_llm_channel(&run.input_json) {
            if let Some(obj) = effective_args.as_object_mut() {
                obj.insert("channel".to_string(), Value::String(channel));
            }
        }
    }

    let scope = llm_policy_scope_for_action(&effective_args, &config.llm)?;
    let is_remote = scope.starts_with("remote:");
    let estimated_tokens = effective_args
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(512);
    let budget_window_seconds = config.llm.remote_token_budget_window_secs.max(1);
    let budget_window_start =
        OffsetDateTime::now_utc() - time::Duration::seconds(budget_window_seconds as i64);
    let mut remote_budget_remaining_tenant: Option<u64> = None;
    let mut remote_budget_remaining_agent: Option<u64> = None;
    let mut remote_budget_remaining_model: Option<u64> = None;
    let mut soft_alerts: Vec<Value> = Vec::new();
    if is_remote {
        if let Some(remaining) = execution_context.remote_llm_tokens_remaining {
            if estimated_tokens > remaining {
                return Err(anyhow!(
                    "llm.infer remote token budget exceeded (remaining={}, requested_estimate={})",
                    remaining,
                    estimated_tokens
                ));
            }
        }
        if let Some(limit) = config.llm.remote_token_budget_per_tenant {
            let spent = sum_llm_consumed_tokens_for_tenant_since_dual(
                pool,
                &run.tenant_id,
                budget_window_start,
            )
            .await?
            .max(0) as u64;
            if spent.saturating_add(estimated_tokens) > limit {
                return Err(anyhow!(
                    "llm.infer tenant token budget exceeded (remaining={}, requested_estimate={}, window_secs={})",
                    limit.saturating_sub(spent),
                    estimated_tokens,
                    budget_window_seconds
                ));
            }
            remote_budget_remaining_tenant = Some(limit.saturating_sub(spent));
        }
        if let Some(limit) = config.llm.remote_token_budget_per_agent {
            let spent = sum_llm_consumed_tokens_for_agent_since_dual(
                pool,
                &run.tenant_id,
                run.agent_id,
                budget_window_start,
            )
            .await?
            .max(0) as u64;
            if spent.saturating_add(estimated_tokens) > limit {
                return Err(anyhow!(
                    "llm.infer agent token budget exceeded (remaining={}, requested_estimate={}, window_secs={})",
                    limit.saturating_sub(spent),
                    estimated_tokens,
                    budget_window_seconds
                ));
            }
            remote_budget_remaining_agent = Some(limit.saturating_sub(spent));
        }
        if let Some(limit) = config.llm.remote_token_budget_per_model {
            let spent = sum_llm_consumed_tokens_for_model_since_dual(
                pool,
                &run.tenant_id,
                &scope,
                budget_window_start,
            )
            .await?
            .max(0) as u64;
            if spent.saturating_add(estimated_tokens) > limit {
                return Err(anyhow!(
                    "llm.infer model token budget exceeded (remaining={}, requested_estimate={}, model_scope={}, window_secs={})",
                    limit.saturating_sub(spent),
                    estimated_tokens,
                    scope,
                    budget_window_seconds
                ));
            }
            remote_budget_remaining_model = Some(limit.saturating_sub(spent));
        }
    }

    let cache_namespace = format!("tenant:{}:agent:{}", run.tenant_id, run.agent_id);
    let result = execute_llm_infer(
        &effective_args,
        &config.llm,
        Some(cache_namespace.as_str()),
        match pool {
            DbPool::Postgres(pg_pool) => Some(pg_pool),
            DbPool::Sqlite(_) => None,
        },
    )
    .await?;
    let consumed_tokens = result
        .total_tokens
        .map(u64::from)
        .unwrap_or(estimated_tokens);
    let mut estimated_cost_usd = None;
    if result.route == "remote" {
        if let Some(remaining) = execution_context.remote_llm_tokens_remaining.as_mut() {
            *remaining = remaining.saturating_sub(consumed_tokens);
        }
        if let Some(remaining) = remote_budget_remaining_tenant.as_mut() {
            *remaining = remaining.saturating_sub(consumed_tokens);
        }
        if let Some(remaining) = remote_budget_remaining_agent.as_mut() {
            *remaining = remaining.saturating_sub(consumed_tokens);
        }
        if let Some(remaining) = remote_budget_remaining_model.as_mut() {
            *remaining = remaining.saturating_sub(consumed_tokens);
        }
        if config.llm.remote_cost_per_1k_tokens_usd > 0.0 {
            estimated_cost_usd =
                Some((consumed_tokens as f64 / 1000.0) * config.llm.remote_cost_per_1k_tokens_usd);
        }
        create_llm_token_usage_record_dual(
            pool,
            &NewLlmTokenUsageRecord {
                id: Uuid::new_v4(),
                run_id: run.id,
                action_request_id,
                tenant_id: run.tenant_id.clone(),
                agent_id: run.agent_id,
                route: "remote".to_string(),
                model_key: format!("remote:{}", result.model),
                consumed_tokens: consumed_tokens as i64,
                estimated_cost_usd,
                window_started_at: budget_window_start,
                window_duration_seconds: budget_window_seconds as i64,
            },
        )
        .await?;

        if let Some(threshold_pct) = config.llm.remote_token_budget_soft_alert_threshold_pct {
            maybe_push_llm_budget_soft_alert(
                &mut soft_alerts,
                "run",
                config.llm.remote_token_budget_per_run,
                execution_context.remote_llm_tokens_remaining,
                threshold_pct,
                None,
                None,
            );
            maybe_push_llm_budget_soft_alert(
                &mut soft_alerts,
                "tenant",
                config.llm.remote_token_budget_per_tenant,
                remote_budget_remaining_tenant,
                threshold_pct,
                Some(budget_window_seconds),
                None,
            );
            maybe_push_llm_budget_soft_alert(
                &mut soft_alerts,
                "agent",
                config.llm.remote_token_budget_per_agent,
                remote_budget_remaining_agent,
                threshold_pct,
                Some(budget_window_seconds),
                None,
            );
            maybe_push_llm_budget_soft_alert(
                &mut soft_alerts,
                "model",
                config.llm.remote_token_budget_per_model,
                remote_budget_remaining_model,
                threshold_pct,
                Some(budget_window_seconds),
                Some(scope.as_str()),
            );
        }
    }

    execution_context.last_llm_response_text = Some(result.response_text.clone());

    Ok(json!({
        "route": result.route,
        "model": result.model,
        "gateway": result.gateway,
        "response_text": result.response_text,
        "prompt_tokens": result.prompt_tokens,
        "completion_tokens": result.completion_tokens,
        "total_tokens": result.total_tokens,
        "token_accounting": {
            "estimated_tokens": estimated_tokens,
            "consumed_tokens": consumed_tokens,
            "remote_token_budget_remaining": execution_context.remote_llm_tokens_remaining,
            "remote_budget_window_secs": budget_window_seconds,
            "remote_budget_window_started_at": budget_window_start,
            "remote_token_budget_remaining_tenant": remote_budget_remaining_tenant,
            "remote_token_budget_remaining_agent": remote_budget_remaining_agent,
            "remote_token_budget_remaining_model": remote_budget_remaining_model,
            "soft_alert_threshold_pct": config.llm.remote_token_budget_soft_alert_threshold_pct,
            "soft_alerts": soft_alerts,
            "estimated_cost_usd": estimated_cost_usd,
        }
    }))
}

fn maybe_push_llm_budget_soft_alert(
    alerts: &mut Vec<Value>,
    scope: &str,
    limit: Option<u64>,
    remaining: Option<u64>,
    threshold_pct: u8,
    window_secs: Option<u64>,
    model_scope: Option<&str>,
) {
    let (Some(limit), Some(remaining)) = (limit, remaining) else {
        return;
    };
    if limit == 0 {
        return;
    }

    let used = limit.saturating_sub(remaining);
    let usage_pct = used.saturating_mul(100) / limit;
    if usage_pct < threshold_pct as u64 {
        return;
    }

    alerts.push(json!({
        "scope": scope,
        "threshold_pct": threshold_pct,
        "usage_pct": usage_pct,
        "used_tokens": used,
        "remaining_tokens": remaining,
        "limit_tokens": limit,
        "window_secs": window_secs,
        "model_scope": model_scope,
    }));
}

fn extract_llm_budget_soft_alerts(result_json: &Value) -> Vec<Value> {
    result_json
        .get("token_accounting")
        .and_then(|value| value.get("soft_alerts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn extract_llm_slo_alerts(result_json: &Value) -> Vec<Value> {
    let gateway = result_json.get("gateway");
    let status = gateway
        .and_then(|value| value.get("slo_status"))
        .and_then(Value::as_str)
        .unwrap_or("not_configured");
    if status != "warn" && status != "breach" {
        return Vec::new();
    }

    vec![json!({
        "status": status,
        "request_class": gateway
            .and_then(|value| value.get("request_class"))
            .and_then(Value::as_str),
        "threshold_ms": gateway
            .and_then(|value| value.get("slo_threshold_ms"))
            .and_then(Value::as_u64),
        "latency_ms": gateway
            .and_then(|value| value.get("slo_latency_ms"))
            .and_then(Value::as_u64),
        "reason_code": gateway
            .and_then(|value| value.get("slo_reason_code"))
            .and_then(Value::as_str),
    })]
}

async fn attempt_whitenoise_publish(
    config: &WorkerConfig,
    signer_identity: &signer::NostrSignerIdentity,
    recipient: &str,
    content: &str,
) -> (
    Option<nostr_transport::NostrPublishResult>,
    Option<String>,
    Option<Value>,
) {
    let recipient_pubkey = match PublicKey::parse(recipient)
        .with_context(|| "message.send destination target must be npub/hex for whitenoise")
    {
        Ok(pubkey) => pubkey,
        Err(error) => return (None, Some(format!("{error:#}")), None),
    };

    match config.nostr_signer.mode {
        NostrSignerMode::LocalKey => match resolve_local_secret_key_for_publish(config) {
            Ok(local_secret_key) => match publish_text_note(
                &local_secret_key,
                recipient,
                content,
                &config.nostr_relays,
                config.nostr_publish_timeout,
            )
            .await
            {
                Ok(result) => (Some(result), None, None),
                Err(error) => (None, Some(format!("{error:#}")), None),
            },
            Err(error) => (None, Some(format!("{error:#}")), None),
        },
        NostrSignerMode::Nip46Signer => {
            let signer_pubkey = match PublicKey::parse(signer_identity.public_key.as_str()) {
                Ok(pubkey) => pubkey,
                Err(error) => return (None, Some(format!("{error:#}")), None),
            };
            let unsigned = match build_text_note_unsigned(signer_pubkey, recipient_pubkey, content)
            {
                Ok(event) => event,
                Err(error) => return (None, Some(format!("{error:#}")), None),
            };

            let Some(bunker_uri) = config.nostr_signer.nip46_bunker_uri.as_deref() else {
                return (
                    None,
                    Some("NOSTR_NIP46_BUNKER_URI is required for NIP-46 publish".to_string()),
                    None,
                );
            };
            let signed_outcome = match sign_event_with_bunker(
                &unsigned,
                bunker_uri,
                config.nostr_signer.nip46_client_secret_key.as_deref(),
                config.nostr_publish_timeout,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(error) => return (None, Some(format!("{error:#}")), None),
            };

            match publish_signed_event(
                &signed_outcome.signed_event,
                &config.nostr_relays,
                config.nostr_publish_timeout,
            )
            .await
            {
                Ok(result) => (
                    Some(result),
                    None,
                    Some(json!({
                        "nip46_signer_relay": signed_outcome.signer_relay,
                        "nip46_client_public_key": signed_outcome.app_public_key,
                    })),
                ),
                Err(error) => (None, Some(format!("{error:#}")), None),
            }
        }
    }
}

async fn attempt_slack_send(
    config: &WorkerConfig,
    channel: &str,
    content: &str,
) -> (&'static str, Option<Value>, Option<String>, Option<Value>) {
    let Some(webhook_url) = config.slack_webhook_url.as_deref() else {
        return (
            "queued_local_outbox",
            None,
            None,
            Some(json!({
                "transport":"outbox_only",
                "reason":"SLACK_WEBHOOK_URL is not configured",
                "status":"queued_without_transport",
            })),
        );
    };

    let max_attempts = config.slack_max_attempts.max(1);
    let mut attempt = 1_u32;
    let mut errors = Vec::<String>::new();

    loop {
        match send_webhook_message(webhook_url, channel, content, config.slack_send_timeout).await {
            Ok(result) => {
                return (
                    "delivered_slack",
                    Some(json!({
                        "channel": result.channel,
                        "status_code": result.status_code,
                        "response": result.response,
                        "attempts": attempt,
                    })),
                    None,
                    Some(json!({
                        "transport":"slack_webhook",
                        "status":"delivered",
                        "attempts": attempt,
                        "max_attempts": max_attempts,
                        "retry_backoff_ms": config.slack_retry_backoff.as_millis(),
                    })),
                );
            }
            Err(error) => {
                let error_text = format!("{error:#}");
                errors.push(error_text.clone());
                if attempt >= max_attempts {
                    return (
                        "dead_lettered_local_outbox",
                        None,
                        Some(error_text),
                        Some(json!({
                            "transport":"slack_webhook",
                            "status":"dead_lettered",
                            "attempts": attempt,
                            "max_attempts": max_attempts,
                            "retry_backoff_ms": config.slack_retry_backoff.as_millis(),
                            "errors": errors,
                        })),
                    );
                }
                let exponent = attempt.saturating_sub(1).min(6);
                let backoff_multiplier = 1_u64 << exponent;
                let backoff_ms = (config.slack_retry_backoff.as_millis() as u64)
                    .saturating_mul(backoff_multiplier);
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageProvider {
    WhiteNoise,
    Slack,
}

impl MessageProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::WhiteNoise => "whitenoise",
            Self::Slack => "slack", // Placeholder connector path; transport to be wired separately.
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedMessageDestination<'a> {
    provider: MessageProvider,
    target: &'a str,
}

impl<'a> ParsedMessageDestination<'a> {
    fn parse(action_type: &'a str, raw: &'a str) -> Result<Self> {
        let (provider_raw, target_raw) = raw
            .split_once(':')
            .ok_or_else(|| anyhow!("{action_type} destination must be provider-scoped"))?;
        let provider = match provider_raw.trim().to_ascii_lowercase().as_str() {
            "whitenoise" => MessageProvider::WhiteNoise,
            "slack" => MessageProvider::Slack,
            other => {
                return Err(anyhow!(
                    "{action_type} provider `{}` is unsupported (expected whitenoise or slack)",
                    other
                ));
            }
        };
        let target = target_raw.trim();
        if target.is_empty() {
            return Err(anyhow!(
                "{action_type} destination target must not be empty: {}",
                raw
            ));
        }
        Ok(Self { provider, target })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaymentProvider {
    Nwc,
    Cashu,
}

impl PaymentProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Nwc => "nwc",
            Self::Cashu => "cashu",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPaymentDestination<'a> {
    provider: PaymentProvider,
    target: &'a str,
}

impl<'a> ParsedPaymentDestination<'a> {
    fn parse(raw: &'a str) -> Result<Self> {
        let (provider_raw, target_raw) = raw
            .split_once(':')
            .ok_or_else(|| anyhow!("payment.send destination must be provider-scoped"))?;
        let provider = match provider_raw.trim().to_ascii_lowercase().as_str() {
            "nwc" => PaymentProvider::Nwc,
            "cashu" => PaymentProvider::Cashu,
            other => {
                return Err(anyhow!(
                    "payment.send provider `{}` is unsupported (expected nwc or cashu)",
                    other
                ));
            }
        };
        let target = target_raw.trim();
        if target.is_empty() {
            return Err(anyhow!(
                "payment.send destination target must not be empty: {}",
                raw
            ));
        }
        Ok(Self { provider, target })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaymentOperation {
    PayInvoice,
    MakeInvoice,
    GetBalance,
}

impl PaymentOperation {
    fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pay_invoice" => Ok(Self::PayInvoice),
            "make_invoice" => Ok(Self::MakeInvoice),
            "get_balance" => Ok(Self::GetBalance),
            other => Err(anyhow!(
                "unsupported payment.send operation `{}` (expected pay_invoice, make_invoice, get_balance)",
                other
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::PayInvoice => "pay_invoice",
            Self::MakeInvoice => "make_invoice",
            Self::GetBalance => "get_balance",
        }
    }

    fn requires_amount(self) -> bool {
        matches!(self, Self::PayInvoice | Self::MakeInvoice)
    }
}

fn build_nwc_request(
    operation: PaymentOperation,
    args: &Value,
    amount_msat: Option<u64>,
) -> Result<NwcRequest> {
    match operation {
        PaymentOperation::PayInvoice => {
            let invoice = args
                .get("invoice")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("payment.send args.invoice is required for pay_invoice"))?;
            let mut request = PayInvoiceRequest::new(invoice);
            request.amount = amount_msat;
            Ok(NwcRequest::pay_invoice(request))
        }
        PaymentOperation::MakeInvoice => {
            let amount = amount_msat.ok_or_else(|| {
                anyhow!("payment.send args.amount_msat is required for make_invoice")
            })?;
            let description = args
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            Ok(NwcRequest::make_invoice(MakeInvoiceRequest {
                amount,
                description,
                description_hash: None,
                expiry: None,
            }))
        }
        PaymentOperation::GetBalance => Ok(NwcRequest::get_balance()),
    }
}

#[derive(Debug, Clone)]
struct ResolvedNwcRoutes {
    candidates: Vec<String>,
    rollout_limited: bool,
}

#[derive(Debug, Clone)]
struct ResolvedCashuRoutes {
    candidates: Vec<String>,
    rollout_limited: bool,
}

#[derive(Debug, Clone, Copy)]
struct PaymentRouteHealthEntry {
    consecutive_failures: u32,
    unhealthy_until: Option<Instant>,
}

fn resolve_nwc_uris_for_wallet(
    config: &WorkerConfig,
    wallet_id: &str,
    idempotency_key: &str,
) -> ResolvedNwcRoutes {
    let mut candidates: Vec<String> = config
        .payment_nwc_wallet_uris
        .get(wallet_id)
        .map(|value| split_route_value(value.as_str()))
        .or_else(|| {
            config
                .payment_nwc_wallet_uris
                .get("*")
                .map(|value| split_route_value(value.as_str()))
        })
        .unwrap_or_default();

    if candidates.is_empty() {
        if let Some(default_uri) = config
            .payment_nwc_uri
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            candidates.push(default_uri.to_string());
        }
    }

    let mut rollout_limited = false;
    if candidates.len() <= 1 {
        return ResolvedNwcRoutes {
            candidates,
            rollout_limited,
        };
    }

    if matches!(
        config.payment_nwc_route_strategy,
        PaymentNwcRouteStrategy::DeterministicHash
    ) {
        let offset = deterministic_route_offset(wallet_id, idempotency_key, candidates.len());
        candidates.rotate_left(offset);
    }

    if config.payment_nwc_route_rollout_percent < 100
        && !is_payment_route_rollout_enabled(
            wallet_id,
            idempotency_key,
            config.payment_nwc_route_rollout_percent,
        )
    {
        candidates.truncate(1);
        rollout_limited = true;
    }

    if !config.payment_nwc_route_fallback_enabled {
        candidates.truncate(1);
    }

    ResolvedNwcRoutes {
        candidates,
        rollout_limited,
    }
}

fn resolve_cashu_mint_uris(
    config: &WorkerConfig,
    mint_id: &str,
    idempotency_key: &str,
) -> ResolvedCashuRoutes {
    let mut candidates: Vec<String> = config
        .payment_cashu_mint_uris
        .get(mint_id)
        .map(|value| split_route_value(value.as_str()))
        .or_else(|| {
            config
                .payment_cashu_default_mint
                .as_deref()
                .and_then(|default_mint| config.payment_cashu_mint_uris.get(default_mint))
                .map(|value| split_route_value(value.as_str()))
        })
        .or_else(|| {
            config
                .payment_cashu_mint_uris
                .get("*")
                .map(|value| split_route_value(value.as_str()))
        })
        .unwrap_or_default();

    let mut rollout_limited = false;
    if candidates.len() <= 1 {
        return ResolvedCashuRoutes {
            candidates,
            rollout_limited,
        };
    }

    if matches!(
        config.payment_cashu_route_strategy,
        PaymentNwcRouteStrategy::DeterministicHash
    ) {
        let offset = deterministic_route_offset(mint_id, idempotency_key, candidates.len());
        candidates.rotate_left(offset);
    }

    if config.payment_cashu_route_rollout_percent < 100
        && !is_payment_route_rollout_enabled(
            mint_id,
            idempotency_key,
            config.payment_cashu_route_rollout_percent,
        )
    {
        candidates.truncate(1);
        rollout_limited = true;
    }

    if !config.payment_cashu_route_fallback_enabled {
        candidates.truncate(1);
    }

    ResolvedCashuRoutes {
        candidates,
        rollout_limited,
    }
}

fn split_route_value(raw: &str) -> Vec<String> {
    raw.split('|')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn deterministic_route_offset(wallet_id: &str, idempotency_key: &str, route_count: usize) -> usize {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    if route_count == 0 {
        return 0;
    }
    let mut hasher = DefaultHasher::new();
    wallet_id.hash(&mut hasher);
    idempotency_key.hash(&mut hasher);
    (hasher.finish() as usize) % route_count
}

fn is_payment_route_rollout_enabled(
    wallet_id: &str,
    idempotency_key: &str,
    rollout_pct: u8,
) -> bool {
    if rollout_pct >= 100 {
        return true;
    }
    if rollout_pct == 0 {
        return false;
    }
    let bucket = deterministic_route_offset(wallet_id, idempotency_key, 100);
    bucket < rollout_pct as usize
}

fn payment_route_health_state() -> &'static Mutex<HashMap<String, PaymentRouteHealthEntry>> {
    static ROUTE_HEALTH: OnceLock<Mutex<HashMap<String, PaymentRouteHealthEntry>>> =
        OnceLock::new();
    ROUTE_HEALTH.get_or_init(|| Mutex::new(HashMap::new()))
}

fn payment_route_health_key(
    config: &WorkerConfig,
    route_namespace: &str,
    wallet_id: &str,
    route_uri: &str,
) -> String {
    format!(
        "{}|{}|{}|{}",
        config.worker_id, route_namespace, wallet_id, route_uri
    )
}

fn should_attempt_payment_route(
    config: &WorkerConfig,
    route_namespace: &str,
    wallet_id: &str,
    route_uri: &str,
    fail_threshold: u32,
    cooldown: Duration,
) -> bool {
    if fail_threshold == 0 || cooldown.is_zero() {
        return true;
    }
    let key = payment_route_health_key(config, route_namespace, wallet_id, route_uri);
    let now = Instant::now();
    let Ok(mut guard) = payment_route_health_state().lock() else {
        return true;
    };
    if let Some(entry) = guard.get_mut(&key) {
        if let Some(unhealthy_until) = entry.unhealthy_until {
            if now < unhealthy_until {
                return false;
            }
        }
        entry.unhealthy_until = None;
        entry.consecutive_failures = 0;
    }
    true
}

fn mark_payment_route_success(
    config: &WorkerConfig,
    route_namespace: &str,
    wallet_id: &str,
    route_uri: &str,
) {
    let fail_threshold = if route_namespace == "cashu" {
        config.payment_cashu_route_health_fail_threshold
    } else {
        config.payment_nwc_route_health_fail_threshold
    };
    if fail_threshold == 0 {
        return;
    }
    let key = payment_route_health_key(config, route_namespace, wallet_id, route_uri);
    if let Ok(mut guard) = payment_route_health_state().lock() {
        guard.remove(&key);
    }
}

fn mark_payment_route_failure(
    config: &WorkerConfig,
    route_namespace: &str,
    wallet_id: &str,
    route_uri: &str,
) {
    let (fail_threshold, cooldown) = if route_namespace == "cashu" {
        (
            config.payment_cashu_route_health_fail_threshold,
            config.payment_cashu_route_health_cooldown,
        )
    } else {
        (
            config.payment_nwc_route_health_fail_threshold,
            config.payment_nwc_route_health_cooldown,
        )
    };
    if fail_threshold == 0 || cooldown.is_zero() {
        return;
    }
    let key = payment_route_health_key(config, route_namespace, wallet_id, route_uri);
    let now = Instant::now();
    if let Ok(mut guard) = payment_route_health_state().lock() {
        let entry = guard.entry(key).or_insert(PaymentRouteHealthEntry {
            consecutive_failures: 0,
            unhealthy_until: None,
        });
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        if entry.consecutive_failures >= fail_threshold {
            entry.unhealthy_until = Some(now + cooldown);
            entry.consecutive_failures = 0;
        }
    }
}

async fn append_audit_event(pool: &DbPool, new_event: &NewAuditEvent) -> Result<()> {
    let mut event = new_event.clone();
    event.payload_json = redact_json(&event.payload_json);
    append_audit_event_dual(pool, &event).await?;
    Ok(())
}

fn to_policy_request(
    action: &skillrunner::ActionRequest,
    config: &WorkerConfig,
) -> Result<PolicyActionRequest, ContractValidationError> {
    validate_action_contract(action)?;

    let payload_bytes = serde_json::to_vec(&action.args)
        .map_err(|error| {
            ContractValidationError::invalid(format!(
                "failed serializing action args for payload sizing: {error}"
            ))
        })?
        .len() as u64;

    let scope = match action.action_type.as_str() {
        "object.write" => required_trimmed_arg(action, "path", "object.write args.path")?,
        "message.send" => {
            required_trimmed_arg(action, "destination", "message.send args.destination")?
        }
        "message.receive" => {
            required_trimmed_arg(action, "source", "message.receive args.source")?
        }
        "payment.send" => {
            required_trimmed_arg(action, "destination", "payment.send args.destination")?
        }
        "memory.read" | "memory.write" => {
            required_trimmed_arg(action, "scope", "memory action args.scope")?
        }
        "llm.infer" => llm_policy_scope_for_action(&action.args, &config.llm)
            .map_err(|error| ContractValidationError::invalid(error.to_string()))?,
        "local.exec" => {
            let template_id = required_trimmed_arg(action, "template_id", "local.exec args.template_id")?;
            format!("local.exec:{template_id}")
        }
        _ => String::new(),
    };

    Ok(PolicyActionRequest::new(
        action.action_type.clone(),
        scope,
        payload_bytes,
    ))
}

fn extract_trace_id_from_run_input(input_json: &Value) -> Option<String> {
    input_json
        .get("_trace")
        .and_then(Value::as_str)
        .map(std::string::ToString::to_string)
}

fn append_trace_to_audit_payload(payload_json: Value, trace_id: &str) -> Value {
    match payload_json {
        Value::Object(mut map) => {
            map.insert("trace_id".to_string(), Value::String(trace_id.to_string()));
            Value::Object(map)
        }
        other => json!({
            "payload": other,
            "trace_id": trace_id,
        }),
    }
}

async fn append_audit_event_with_trace(
    pool: &DbPool,
    new_event: &NewAuditEvent,
    run_trace_id: Option<&str>,
) -> Result<()> {
    let mut event = new_event.clone();
    if let Some(trace_id) = run_trace_id {
        event.payload_json = append_trace_to_audit_payload(event.payload_json, trace_id);
    }
    append_audit_event(pool, &event).await?;
    Ok(())
}

#[derive(Debug)]
struct ContractValidationError {
    reason: DenyReason,
    detail: String,
}

impl ContractValidationError {
    fn invalid(detail: impl Into<String>) -> Self {
        Self {
            reason: DenyReason::InvalidActionContract,
            detail: detail.into(),
        }
    }
}

fn validate_action_contract(action: &skillrunner::ActionRequest) -> Result<(), ContractValidationError> {
    let action_type = normalized_action_type(action.action_type.as_str());
    let action_contract_version = action
        .action_contract_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let action_schema_id = action
        .action_schema_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let has_version = action_contract_version.is_some();
    let has_schema = action_schema_id.is_some();

    if has_version != has_schema {
        return Err(ContractValidationError::invalid(
            "action_contract_version and action_schema_id must be provided together",
        ));
    }

    let Some(version) = action_contract_version else {
        return Ok(());
    };

    let normalized_version = normalize_contract_version(version).ok_or_else(|| {
        ContractValidationError::invalid(format!(
            "unsupported action_contract_version `{version}`"
        ))
    })?;

    let Some(schema_id) = action_schema_id else {
        return Err(ContractValidationError::invalid(
            "action_schema_id must be a non-empty string",
        ));
    };

    if !is_supported_action_type_for_contract_check(&action_type) {
        return Ok(());
    }

    let (schema_action_type, schema_version) = schema_id
        .split_once(':')
        .map(|(schema_action_type, schema_version)| {
            (
                schema_action_type.trim(),
                normalize_contract_version(schema_version),
            )
        })
        .ok_or_else(|| {
            ContractValidationError::invalid(
                "action_schema_id must use `<action_type>:<version>` format",
            )
        })?;

    let Some(schema_version) = schema_version else {
        return Err(ContractValidationError::invalid(format!(
            "unsupported action_contract_version `{schema_id}`",
        )));
    };

    if normalized_action_type(schema_action_type) != action_type {
        return Err(ContractValidationError::invalid(
            "action_schema_id action type does not match action_type",
        ));
    }
    if schema_version != normalized_version {
        return Err(ContractValidationError::invalid(
            "action_schema_id version does not match action_contract_version",
        ));
    }

    if schema_id.contains("..") {
        return Err(ContractValidationError::invalid(
            "action_schema_id contains disallowed path segments",
        ));
    }

    Ok(())
}

fn required_trimmed_arg(
    action: &skillrunner::ActionRequest,
    field: &str,
    label: &str,
) -> Result<String, ContractValidationError> {
    let value = action
        .args
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ContractValidationError::invalid(format!("{label} is required and must not be empty"))
        })?;
    Ok(value.to_string())
}

fn normalize_skill_action(action: &mut skillrunner::ActionRequest) {
    action.action_type = normalized_action_type(action.action_type.as_str());
    if let Some(version) = action.action_contract_version.as_mut() {
        let trimmed = version.trim().to_string();
        if trimmed.is_empty() {
            action.action_contract_version = None;
        } else {
            action.action_contract_version = Some(trimmed);
        }
    }
    if let Some(schema_id) = action.action_schema_id.as_mut() {
        let trimmed = schema_id.trim().to_string();
        if trimmed.is_empty() {
            action.action_schema_id = None;
        } else {
            action.action_schema_id = Some(trimmed);
        }
    }

    if let Some(map) = action.args.as_object_mut() {
        canonicalize_action_arg_aliases(map);
        for field in ["path", "destination", "source", "scope", "template_id"] {
            if let Some(Value::String(value)) = map.get_mut(field) {
                *value = value.trim().to_string();
            }
        }
    }
}

fn is_supported_action_type_for_contract_check(action_type: &str) -> bool {
    matches!(
        action_type,
            "object.read"
            | "memory.read"
            | "memory.write"
            | "object.write"
            | "message.send"
            | "message.receive"
            | "payment.send"
            | "llm.infer"
            | "local.exec"
    )
}

fn normalize_contract_version(raw_version: &str) -> Option<&'static str> {
    match raw_version.trim() {
        "1" | "v1" | "V1" => Some("1"),
        _ => None,
    }
}

fn canonicalize_action_arg_aliases(map: &mut serde_json::Map<String, Value>) {
    canonicalize_action_arg_alias(map, "path", &["file_path", "filepath", "source_path"]);
    canonicalize_action_arg_alias(
        map,
        "destination",
        &["to", "recipient", "recipient_id"],
    );
    canonicalize_action_arg_alias(
        map,
        "source",
        &["from", "sender", "sender_id"],
    );
    canonicalize_action_arg_alias(map, "scope", &["scope_name", "scope-name"]);
    canonicalize_action_arg_alias(
        map,
        "template_id",
        &["template", "template-id", "template_name"],
    );

    if let Some(Value::String(value)) = map.get_mut("path") {
        *value = value.trim().to_string();
    }
    if let Some(Value::String(value)) = map.get_mut("destination") {
        *value = value.trim().to_string();
    }
    if let Some(Value::String(value)) = map.get_mut("scope") {
        *value = value.trim().to_string();
    }
    if let Some(Value::String(value)) = map.get_mut("template_id") {
        *value = value.trim().to_string();
    }
}

fn canonicalize_action_arg_alias(
    map: &mut serde_json::Map<String, Value>,
    canonical: &str,
    aliases: &[&str],
) {
    let mut canonical_value = map.remove(canonical);
    if canonical_value.is_none() {
        for alias in aliases {
            if canonical_value.is_none() {
                canonical_value = map.remove(*alias);
            } else {
                map.remove(*alias);
            }
        }
    } else {
        for alias in aliases {
            map.remove(*alias);
        }
    }

    if let Some(value) = canonical_value {
        map.insert(canonical.to_string(), value);
    }
}

fn parse_grant_set(raw: &Value) -> GrantSet {
    let Some(items) = raw.as_array() else {
        return GrantSet::default();
    };

    let grants = items
        .iter()
        .filter_map(parse_capability_grant)
        .collect::<Vec<_>>();

    GrantSet::new(grants)
}

// --- Parsing and configuration helpers ---

fn parse_capability_grant(value: &Value) -> Option<PolicyCapabilityGrant> {
    let capability = value.get("capability")?.as_str()?;
    let scope = value.get("scope")?.as_str()?.to_string();

    let kind = parse_capability_kind(capability)?;
    let mut grant = PolicyCapabilityGrant::new(kind, scope);

    if let Some(max_payload_bytes) = value
        .get("limits")
        .and_then(|limits| limits.get("max_payload_bytes"))
        .and_then(Value::as_u64)
    {
        grant = grant.with_max_payload_bytes(max_payload_bytes);
    }

    Some(grant)
}

fn parse_capability_kind(value: &str) -> Option<PolicyCapabilityKind> {
    match value {
        "object.read" | "object_read" => Some(PolicyCapabilityKind::ObjectRead),
        "object.write" | "object_write" => Some(PolicyCapabilityKind::ObjectWrite),
        "memory.read" | "memory_read" => Some(PolicyCapabilityKind::MemoryRead),
        "memory.write" | "memory_write" => Some(PolicyCapabilityKind::MemoryWrite),
        "message.send" | "message_send" => Some(PolicyCapabilityKind::MessageSend),
        "message.receive" | "message_receive" => Some(PolicyCapabilityKind::MessageReceive),
        "payment.send" | "payment_send" => Some(PolicyCapabilityKind::PaymentSend),
        "llm.infer" | "llm_infer" => Some(PolicyCapabilityKind::LlmInfer),
        "local.exec" | "local_exec" => Some(PolicyCapabilityKind::LocalExec),
        "db.query" | "db_query" => Some(PolicyCapabilityKind::DbQuery),
        "http.request" | "http_request" => Some(PolicyCapabilityKind::HttpRequest),
        _ => None,
    }
}

fn capability_kind_to_action_type(kind: &PolicyCapabilityKind) -> &'static str {
    match kind {
        PolicyCapabilityKind::ObjectRead => "object.read",
        PolicyCapabilityKind::ObjectWrite => "object.write",
        PolicyCapabilityKind::MemoryRead => "memory.read",
        PolicyCapabilityKind::MemoryWrite => "memory.write",
        PolicyCapabilityKind::MessageSend => "message.send",
        PolicyCapabilityKind::MessageReceive => "message.receive",
        PolicyCapabilityKind::PaymentSend => "payment.send",
        PolicyCapabilityKind::LlmInfer => "llm.infer",
        PolicyCapabilityKind::LocalExec => "local.exec",
        PolicyCapabilityKind::DbQuery => "db.query",
        PolicyCapabilityKind::HttpRequest => "http.request",
    }
}

fn sanitize_relative_path(path: &str) -> Result<PathBuf> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(anyhow!("absolute paths are not allowed: {}", path));
    }

    let mut cleaned = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => cleaned.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("unsafe path component in {}", path));
            }
        }
    }

    if cleaned.as_os_str().is_empty() {
        return Err(anyhow!("empty artifact path is not allowed"));
    }

    Ok(cleaned)
}

fn tenant_scoped_artifact_root(artifact_root: &Path, tenant_id: &str) -> Result<PathBuf> {
    let safe_tenant_path = sanitize_relative_path(tenant_id)?;
    Ok(artifact_root.join("tenants").join(safe_tenant_path))
}

fn read_env_u64(key: &str, default: u64) -> Result<u64> {
    match env::var(key) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("invalid integer for {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn read_env_u8(key: &str, default: u8, min: u8, max: u8) -> Result<u8> {
    if min > max {
        return Err(anyhow!("invalid bounds for {} ({} > {})", key, min, max));
    }
    let value = match env::var(key) {
        Ok(raw) => raw
            .parse::<u8>()
            .with_context(|| format!("invalid integer for {key}: {raw}"))?,
        Err(_) => default,
    };
    if value < min || value > max {
        return Err(anyhow!(
            "{} must be between {} and {} (got {})",
            key,
            min,
            max,
            value
        ));
    }
    Ok(value)
}

fn read_env_optional_u64(key: &str) -> Result<Option<u64>> {
    match env::var(key) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                let parsed = trimmed
                    .parse::<u64>()
                    .with_context(|| format!("invalid integer for {key}: {value}"))?;
                Ok(Some(parsed))
            }
        }
        Err(_) => Ok(None),
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

fn read_env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        ),
        Err(_) => default,
    }
}

fn read_env_non_empty_string(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn read_env_optional_sha256(key: &str) -> Result<Option<String>> {
    let Some(raw) = env::var(key).ok() else {
        return Ok(None);
    };
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "{} must be a 64-character lowercase/uppercase sha256 hex string",
            key
        ));
    }
    Ok(Some(value))
}

fn read_env_secret(value_key: &str, reference_key: &str) -> Result<Option<String>> {
    let resolver = shared_secret_resolver();
    resolve_secret_value(
        env::var(value_key).ok(),
        env::var(reference_key).ok(),
        resolver,
    )
}

fn read_env_secret_map(value_key: &str, reference_key: &str) -> Result<BTreeMap<String, String>> {
    let resolver = shared_secret_resolver();
    let resolved = resolve_secret_value(
        env::var(value_key).ok(),
        env::var(reference_key).ok(),
        resolver,
    )?;
    parse_wallet_endpoint_map(resolved.as_deref().unwrap_or_default(), value_key)
}

fn shared_secret_resolver() -> &'static CachedSecretResolver<CliSecretResolver> {
    static RESOLVER: OnceLock<CachedSecretResolver<CliSecretResolver>> = OnceLock::new();
    RESOLVER.get_or_init(|| CachedSecretResolver::from_env_with(CliSecretResolver::from_env()))
}

fn parse_wallet_endpoint_map(raw: &str, source_key: &str) -> Result<BTreeMap<String, String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(BTreeMap::new());
    }

    if trimmed.starts_with('{') {
        let decoded: Value = serde_json::from_str(trimmed).with_context(|| {
            format!("{source_key} must be valid JSON object when using JSON map syntax")
        })?;
        let object = decoded.as_object().ok_or_else(|| {
            anyhow!("{source_key} JSON map must be an object of wallet_id -> endpoint_uri")
        })?;
        let mut mapped = BTreeMap::new();
        for (wallet_id_raw, uri_value) in object {
            let wallet_id = wallet_id_raw.trim();
            if !is_valid_wallet_id(wallet_id) {
                return Err(anyhow!(
                    "{source_key} contains invalid wallet id `{wallet_id}`"
                ));
            }
            let uri = uri_value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    anyhow!("{source_key} wallet `{wallet_id}` must map to non-empty URI string")
                })?;
            if mapped
                .insert(wallet_id.to_string(), uri.to_string())
                .is_some()
            {
                return Err(anyhow!(
                    "{source_key} contains duplicate wallet id `{wallet_id}`"
                ));
            }
        }
        return Ok(mapped);
    }

    let mut mapped = BTreeMap::new();
    for entry in trimmed
        .split(['\n', ','])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (wallet_id_raw, uri_raw) = entry.split_once('=').ok_or_else(|| {
            anyhow!("{source_key} entry must be `wallet_id=endpoint_uri`: {entry}")
        })?;
        let wallet_id = wallet_id_raw.trim();
        if !is_valid_wallet_id(wallet_id) {
            return Err(anyhow!(
                "{source_key} contains invalid wallet id `{wallet_id}`"
            ));
        }
        let uri = uri_raw.trim();
        if uri.is_empty() {
            return Err(anyhow!(
                "{source_key} wallet `{wallet_id}` has empty URI value"
            ));
        }
        if mapped
            .insert(wallet_id.to_string(), uri.to_string())
            .is_some()
        {
            return Err(anyhow!(
                "{source_key} contains duplicate wallet id `{wallet_id}`"
            ));
        }
    }
    Ok(mapped)
}

fn read_env_skill_recipe_commands(
    value_key: &str,
) -> Result<BTreeMap<String, Vec<String>>> {
    let Ok(raw) = env::var(value_key) else {
        return Ok(BTreeMap::new());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(BTreeMap::new());
    }

    if trimmed.starts_with('{') {
        let decoded: Value = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "{value_key} must be a JSON object mapping recipe_id -> command args list"
            )
        })?;
        let object = decoded
            .as_object()
            .ok_or_else(|| anyhow!("{value_key} must be a JSON object when using JSON map syntax"))?;
        let mut mapped = BTreeMap::new();
        for (recipe_id_raw, raw_cmd) in object {
            let recipe_id = recipe_id_raw.trim();
            if recipe_id.is_empty() {
                return Err(anyhow!("{value_key} contains an empty recipe id"));
            }
            let command = parse_skill_recipe_command_value(recipe_id, raw_cmd, value_key)?;
            if mapped
                .insert(recipe_id.to_string(), command)
                .is_some()
            {
                return Err(anyhow!(
                    "{value_key} contains duplicate recipe id `{recipe_id}`"
                ));
            }
        }
        return Ok(mapped);
    }

    let mut mapped = BTreeMap::new();
    for raw_entry in trimmed
        .split([';', '\n'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let Some((recipe_id, raw_cmd)) = raw_entry.split_once('=') else {
            return Err(anyhow!(
                "{value_key} must be JSON object or key=value entries separated by ';' or newlines"
            ));
        };
        let recipe_id = recipe_id.trim();
        if recipe_id.is_empty() {
            return Err(anyhow!("{value_key} contains an empty recipe id"));
        }
        let command = parse_skill_recipe_command_str(recipe_id, raw_cmd, value_key)?;
        if mapped
            .insert(recipe_id.to_string(), command)
            .is_some()
        {
            return Err(anyhow!(
                "{value_key} contains duplicate recipe id `{recipe_id}`"
            ));
        }
    }
    Ok(mapped)
}

fn parse_skill_recipe_command_value(
    recipe_id: &str,
    raw_cmd: &Value,
    value_key: &str,
) -> Result<Vec<String>> {
    match raw_cmd {
        Value::String(raw) => parse_skill_recipe_command_str(recipe_id, raw, value_key),
        Value::Array(values) => {
            let command = values
                .into_iter()
                .map(|value| {
                    let value = value.as_str().ok_or_else(|| {
                        anyhow!(
                            "{value_key} recipe `{recipe_id}` command list contains non-string value"
                        )
                    })?;
                    let arg = value.trim();
                    if arg.is_empty() {
                        return Err(anyhow!(
                            "{value_key} recipe `{recipe_id}` has an empty command argument"
                        ));
                    }
                    Ok(arg.to_string())
                })
                .collect::<Result<Vec<_>>>()?;
            if command.is_empty() {
                return Err(anyhow!(
                    "{value_key} recipe `{recipe_id}` command list is empty"
                ));
            }
            Ok(command)
        }
        _ => Err(anyhow!(
            "{value_key} recipe `{recipe_id}` must be a string command or command array"
        )),
    }
}

fn parse_skill_recipe_command_str(
    recipe_id: &str,
    raw_cmd: &str,
    value_key: &str,
) -> Result<Vec<String>> {
    let command = raw_cmd
        .split_whitespace()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if command.is_empty() {
        return Err(anyhow!(
            "{value_key} recipe `{recipe_id}` command is empty"
        ));
    }
    Ok(command)
}

fn is_valid_wallet_id(raw: &str) -> bool {
    if raw == "*" {
        return true;
    }
    !raw.is_empty()
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn read_env_csv(key: &str) -> Vec<String> {
    let Ok(raw) = env::var(key) else {
        return Vec::new();
    };
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn normalized_action_type(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_action_type_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| normalized_action_type(value.as_str()))
        .filter(|value| !value.is_empty())
        .collect()
}

fn resolve_local_secret_key_for_publish(config: &WorkerConfig) -> Result<SecretKey> {
    if let Some(secret) = config
        .nostr_signer
        .local_secret_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return SecretKey::parse(secret)
            .with_context(|| "failed to parse local Nostr secret key for publish");
    }

    if let Some(path) = &config.nostr_signer.local_secret_key_file {
        let secret = fs::read_to_string(path).with_context(|| {
            format!(
                "failed to read local Nostr secret key file for publish: {}",
                path.display()
            )
        })?;
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(anyhow!(
                "local Nostr secret key file is empty: {}",
                path.display()
            ));
        }
        return SecretKey::parse(secret)
            .with_context(|| "failed to parse local Nostr secret key from file for publish");
    }

    Err(anyhow!(
        "Nostr relay publish requires local key material (NOSTR_SECRET_KEY or NOSTR_SECRET_KEY_FILE)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use serde_json::json;

    #[test]
    fn worker_config_from_env_respects_inflight_caps_defaults_and_minimum()
    -> anyhow::Result<()> {
        let original_dispatch: Option<OsString> = env::var_os("WORKER_TRIGGER_DISPATCH_MAX_INFLIGHT_RUNS");
        let original_claim: Option<OsString> = env::var_os("WORKER_CLAIM_MAX_INFLIGHT_RUNS");

        env::remove_var("WORKER_TRIGGER_DISPATCH_MAX_INFLIGHT_RUNS");
        env::remove_var("WORKER_CLAIM_MAX_INFLIGHT_RUNS");

        let defaults = WorkerConfig::from_env()?;
        assert_eq!(defaults.trigger_dispatch_max_inflight_runs, 1_000);
        assert_eq!(defaults.claim_max_inflight_runs, 1_000);

        env::set_var("WORKER_TRIGGER_DISPATCH_MAX_INFLIGHT_RUNS", "7");
        env::set_var("WORKER_CLAIM_MAX_INFLIGHT_RUNS", "8");
        let explicit = WorkerConfig::from_env()?;
        assert_eq!(explicit.trigger_dispatch_max_inflight_runs, 7);
        assert_eq!(explicit.claim_max_inflight_runs, 8);

        env::set_var("WORKER_TRIGGER_DISPATCH_MAX_INFLIGHT_RUNS", "0");
        env::set_var("WORKER_CLAIM_MAX_INFLIGHT_RUNS", "0");
        let clamped = WorkerConfig::from_env()?;
        assert_eq!(clamped.trigger_dispatch_max_inflight_runs, 1);
        assert_eq!(clamped.claim_max_inflight_runs, 1);

        match original_dispatch {
            Some(value) => env::set_var("WORKER_TRIGGER_DISPATCH_MAX_INFLIGHT_RUNS", value),
            None => env::remove_var("WORKER_TRIGGER_DISPATCH_MAX_INFLIGHT_RUNS"),
        }
        match original_claim {
            Some(value) => env::set_var("WORKER_CLAIM_MAX_INFLIGHT_RUNS", value),
            None => env::remove_var("WORKER_CLAIM_MAX_INFLIGHT_RUNS"),
        }
        Ok(())
    }

    #[test]
    fn worker_config_from_env_reads_skill_recipe_commands() -> anyhow::Result<()> {
        let original_recipe_commands: Option<OsString> =
            env::var_os("WORKER_SKILL_RECIPE_COMMANDS");

        env::set_var(
            "WORKER_SKILL_RECIPE_COMMANDS",
            r#"{"show_notes_v1":["python3","skills/python/summarize_transcript/main.py"],"audit_chain_verifier_v1":["skills/rust/audit_chain_verifier/target/release/audit_chain_verifier"]}"#,
        );

        let config = WorkerConfig::from_env()?;
        let summarize = config
            .skill_recipe_commands
            .get("show_notes_v1")
            .ok_or_else(|| anyhow::anyhow!("missing show_notes_v1 command"))?;
        let verifier = config
            .skill_recipe_commands
            .get("audit_chain_verifier_v1")
            .ok_or_else(|| anyhow::anyhow!("missing audit_chain_verifier_v1 command"))?;

        assert_eq!(
            summarize,
            &vec![
                "python3".to_string(),
                "skills/python/summarize_transcript/main.py".to_string()
            ]
        );
        assert_eq!(
            verifier,
            &vec!["skills/rust/audit_chain_verifier/target/release/audit_chain_verifier".to_string()]
        );

        match original_recipe_commands {
            Some(value) => env::set_var("WORKER_SKILL_RECIPE_COMMANDS", value),
            None => env::remove_var("WORKER_SKILL_RECIPE_COMMANDS"),
        }
        Ok(())
    }

    #[test]
    fn normalize_skill_action_canonicalizes_action_fields_and_aliases() -> anyhow::Result<()> {
        let mut action = skillrunner::ActionRequest {
            action_id: "action-1".to_string(),
            action_type: "  Object.Write ".to_string(),
            args: json!({
                "file_path": " shownotes/ep245.md ",
                "recipient": " podcast-dest ",
                "template": " ignored ",
                "to": "ignored-too",
            }),
            justification: "canonicalize".to_string(),
            action_contract_version: Some(" v1 ".to_string()),
            action_schema_id: Some(" Object.Write:V1 ".to_string()),
        };

        normalize_skill_action(&mut action);
        assert_eq!(action.action_type, "object.write");
        assert_eq!(action.action_contract_version.as_deref(), Some("v1"));
        assert_eq!(
            action.action_schema_id.as_deref(),
            Some("Object.Write:V1")
        );
        assert_eq!(
            action
                .args
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("expected path arg"))?,
            "shownotes/ep245.md"
        );
        assert!(action.args.get("template").is_none());
        assert!(action.args.get("to").is_none());
        assert!(action.args.get("recipient").is_none());
        Ok(())
    }

    #[test]
    fn worker_config_from_env_falls_back_to_artifact_script_when_configured_script_missing(
    ) -> anyhow::Result<()> {
        let original_skill_script: Option<OsString> = env::var_os("WORKER_SKILL_SCRIPT");
        let original_artifact_root: Option<OsString> = env::var_os("WORKER_ARTIFACT_ROOT");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let staged_root =
            std::env::temp_dir().join(format!("secureagnt-worker-skill-test-{stamp}"));
        let staged_script = staged_root
            .join("skills")
            .join("python")
            .join("summarize_transcript")
            .join("main.py");
        std::fs::create_dir_all(staged_script.parent().unwrap())?;
        std::fs::write(&staged_script, "staged")?;

        env::set_var("WORKER_ARTIFACT_ROOT", staged_root.to_string_lossy().as_ref());
        env::set_var("WORKER_SKILL_SCRIPT", "/tmp/does-not-exist-should-fallback");

        let config = WorkerConfig::from_env()?;
        assert_eq!(config.skill_args[0], staged_script.to_string_lossy());

        match original_skill_script {
            Some(value) => env::set_var("WORKER_SKILL_SCRIPT", value),
            None => env::remove_var("WORKER_SKILL_SCRIPT"),
        }
        match original_artifact_root {
            Some(value) => env::set_var("WORKER_ARTIFACT_ROOT", value),
            None => env::remove_var("WORKER_ARTIFACT_ROOT"),
        }
        let _ = std::fs::remove_dir_all(&staged_root);
        Ok(())
    }

    #[test]
    fn worker_config_from_env_prefers_explicit_script_when_present() -> anyhow::Result<()> {
        let original_skill_script: Option<OsString> = env::var_os("WORKER_SKILL_SCRIPT");
        let original_artifact_root: Option<OsString> = env::var_os("WORKER_ARTIFACT_ROOT");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!("secureagnt-worker-skill-test-explicit-{stamp}"));
        let explicit_root = root.join("explicit");
        let explicit_script = explicit_root.join("explicit.py");
        std::fs::create_dir_all(explicit_root)?;
        std::fs::write(&explicit_script, "explicit")?;

        let staged_root = std::env::temp_dir()
            .join(format!("secureagnt-worker-skill-test-fallback-{stamp}"));
        let staged_script = staged_root
            .join("skills")
            .join("python")
            .join("summarize_transcript")
            .join("main.py");
        std::fs::create_dir_all(staged_script.parent().unwrap())?;
        std::fs::write(&staged_script, "staged")?;

        env::set_var("WORKER_SKILL_SCRIPT", explicit_script.to_string_lossy().as_ref());
        env::set_var("WORKER_ARTIFACT_ROOT", staged_root.to_string_lossy().as_ref());

        let config = WorkerConfig::from_env()?;
        assert_eq!(config.skill_args[0], explicit_script.to_string_lossy());

        match original_skill_script {
            Some(value) => env::set_var("WORKER_SKILL_SCRIPT", value),
            None => env::remove_var("WORKER_SKILL_SCRIPT"),
        }
        match original_artifact_root {
            Some(value) => env::set_var("WORKER_ARTIFACT_ROOT", value),
            None => env::remove_var("WORKER_ARTIFACT_ROOT"),
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&staged_root);
        Ok(())
    }

    #[test]
    fn validate_action_contract_requires_schema_action_type_match() -> anyhow::Result<()> {
        let action = skillrunner::ActionRequest {
            action_id: "action-2".to_string(),
            action_type: "object.write".to_string(),
            args: json!({"path": "shownotes/ep245.md", "content": "# Summary"}),
            justification: "validate".to_string(),
            action_contract_version: Some("v1".to_string()),
            action_schema_id: Some("llm.infer:v1".to_string()),
        };

        let error = validate_action_contract(&action)
            .expect_err("expected schema mismatch to be rejected");
        assert_eq!(error.reason, DenyReason::InvalidActionContract);
        assert!(
            error
                .detail
                .contains("action_schema_id action type does not match action_type")
        );
        Ok(())
    }
}
