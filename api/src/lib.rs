use agent_core::{
    append_audit_event, append_audit_event_dual, append_trigger_audit_event,
    classify_agent_context_mutability, compile_agent_heartbeat_markdown,
    count_inflight_runs_dual, count_tenant_inflight_runs_dual, count_tenant_triggers,
    create_compliance_siem_delivery_record,
    compute_trigger_event_semantic_dedupe_key, create_cron_trigger, create_interval_trigger,
    create_memory_record,
    create_run_with_semantic_dedupe_key_dual, create_webhook_trigger,
    get_active_run_id_by_semantic_dedupe_key_dual, default_agent_context_required_files,
    enqueue_trigger_event,
    fire_trigger_manually, get_llm_usage_totals_since, get_run_status, get_run_status_dual,
    get_tenant_action_latency_summary, get_tenant_action_latency_traces,
    get_tenant_compliance_audit_policy, get_tenant_compliance_siem_delivery_slo,
    get_tenant_compliance_siem_delivery_summary, get_tenant_llm_gateway_lane_summary,
    get_tenant_memory_compaction_stats, get_tenant_ops_summary_dual, get_tenant_payment_summary,
    get_tenant_run_latency_histogram, get_tenant_run_latency_traces, get_trigger,
    list_run_audit_events, list_run_audit_events_dual, list_tenant_compliance_audit_events,
    list_tenant_compliance_siem_delivery_alert_acks, list_tenant_compliance_siem_delivery_records,
    list_tenant_compliance_siem_delivery_target_summaries, list_tenant_handoff_memory_records,
    list_tenant_memory_records, list_tenant_payment_ledger, load_agent_context_snapshot,
    normalize_agent_context_required_files, purge_expired_tenant_compliance_audit_events,
    purge_expired_tenant_memory_records, redact_memory_content,
    requeue_dead_letter_compliance_siem_delivery_record, requeue_dead_letter_trigger_event,
    resolve_secret_value, update_trigger_config, update_trigger_status,
    upsert_tenant_compliance_audit_policy, upsert_tenant_compliance_siem_delivery_alert_ack,
    verify_tenant_compliance_audit_chain, AgentContextLoadError, AgentContextLoaderConfig,
    AgentContextMutability, CachedSecretResolver, CliSecretResolver, DbPool,
    ManualTriggerFireOutcome, NewAuditEvent, NewComplianceSiemDeliveryAlertAckRecord,
    NewComplianceSiemDeliveryRecord, NewCronTrigger, NewIntervalTrigger, NewMemoryRecord, NewRun,
    NewTriggerAuditEvent, NewWebhookTrigger, TriggerEventEnqueueOutcome,
    TriggerEventEnqueueUnavailableReason, TriggerEventReplayOutcome, UpdateTriggerParams,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use core as agent_core;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, SqlitePool};
use std::{
    collections::HashMap,
    env, fs,
    io::Write,
    path::{Component, Path as StdPath, PathBuf},
    sync::OnceLock,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

const TENANT_HEADER: &str = "x-tenant-id";
const ROLE_HEADER: &str = "x-user-role";
const USER_ID_HEADER: &str = "x-user-id";
const TRIGGER_SECRET_HEADER: &str = "x-trigger-secret";
const AUTH_PROXY_TOKEN_HEADER: &str = "x-auth-proxy-token";
const MAX_OBJECT_WRITE_PAYLOAD_BYTES: u64 = 500_000;
const MAX_MESSAGE_SEND_PAYLOAD_BYTES: u64 = 20_000;
const MAX_OBJECT_READ_PAYLOAD_BYTES: u64 = 128_000;
const MAX_LOCAL_EXEC_PAYLOAD_BYTES: u64 = 4_096;
const MAX_LLM_INFER_PAYLOAD_BYTES: u64 = 32_000;
const MAX_PAYMENT_SEND_PAYLOAD_BYTES: u64 = 16_000;
const MAX_MEMORY_READ_PAYLOAD_BYTES: u64 = 64_000;
const MAX_MEMORY_WRITE_PAYLOAD_BYTES: u64 = 64_000;
const CONSOLE_INDEX_HTML: &str = include_str!("../static/console.html");
const BOOTSTRAP_FILE_NAME: &str = "BOOTSTRAP.md";
const BOOTSTRAP_STATUS_FILE_PATH: &str = "sessions/bootstrap.status.jsonl";

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub db_pool: DbPool,
    pub tenant_max_inflight_runs: Option<i64>,
    pub tenant_max_triggers: Option<i64>,
    pub tenant_max_memory_records: Option<i64>,
    pub agent_context_loader: AgentContextLoaderConfig,
    pub agent_context_mutation_enabled: bool,
    pub agent_bootstrap_enabled: bool,
    pub trusted_proxy_auth_enabled: bool,
    pub trusted_proxy_auth_secret: Option<String>,
    pub trusted_proxy_auth_error: Option<String>,
}

#[derive(Clone)]
struct SqliteAppState {
    pub db_pool: DbPool,
    pub tenant_max_inflight_runs: Option<i64>,
    pub agent_context_loader: AgentContextLoaderConfig,
    pub agent_context_mutation_enabled: bool,
    pub agent_bootstrap_enabled: bool,
    pub trusted_proxy_auth_enabled: bool,
    pub trusted_proxy_auth_secret: Option<String>,
    pub trusted_proxy_auth_error: Option<String>,
}

pub fn app_router(pool: PgPool) -> Router {
    let tenant_max_inflight_runs = parse_positive_i64_env("API_TENANT_MAX_INFLIGHT_RUNS");
    let tenant_max_triggers = parse_positive_i64_env("API_TENANT_MAX_TRIGGERS");
    let tenant_max_memory_records = parse_positive_i64_env("API_TENANT_MAX_MEMORY_RECORDS");
    let agent_context_loader = default_api_agent_context_loader_from_env();
    let agent_context_mutation_enabled =
        parse_bool_env("API_AGENT_CONTEXT_MUTATION_ENABLED", false);
    let agent_bootstrap_enabled = parse_bool_env("API_AGENT_BOOTSTRAP_ENABLED", true);
    let (trusted_proxy_auth_enabled, trusted_proxy_auth_secret, trusted_proxy_auth_error) =
        default_trusted_proxy_auth_config_from_env();
    app_router_with_all_limits(
        pool,
        tenant_max_inflight_runs,
        tenant_max_triggers,
        tenant_max_memory_records,
        agent_context_loader,
        agent_context_mutation_enabled,
        agent_bootstrap_enabled,
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        trusted_proxy_auth_error,
    )
}

pub fn app_router_sqlite(db_pool: DbPool) -> Router {
    if !matches!(db_pool, DbPool::Sqlite(_)) {
        panic!("app_router_sqlite requires DbPool::Sqlite");
    }
    let tenant_max_inflight_runs = parse_positive_i64_env("API_TENANT_MAX_INFLIGHT_RUNS");
    let agent_context_loader = default_api_agent_context_loader_from_env();
    let agent_context_mutation_enabled =
        parse_bool_env("API_AGENT_CONTEXT_MUTATION_ENABLED", false);
    let agent_bootstrap_enabled = parse_bool_env("API_AGENT_BOOTSTRAP_ENABLED", true);
    let (trusted_proxy_auth_enabled, trusted_proxy_auth_secret, trusted_proxy_auth_error) =
        default_trusted_proxy_auth_config_from_env();

    app_router_sqlite_with_all_limits(
        db_pool,
        tenant_max_inflight_runs,
        agent_context_loader,
        agent_context_mutation_enabled,
        agent_bootstrap_enabled,
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        trusted_proxy_auth_error,
    )
}

pub fn app_router_sqlite_with_agent_context_config(
    db_pool: DbPool,
    agent_context_loader: AgentContextLoaderConfig,
    agent_context_mutation_enabled: bool,
) -> Router {
    app_router_sqlite_with_agent_context_and_bootstrap_config(
        db_pool,
        agent_context_loader,
        agent_context_mutation_enabled,
        true,
    )
}

pub fn app_router_sqlite_with_agent_context_and_bootstrap_config(
    db_pool: DbPool,
    agent_context_loader: AgentContextLoaderConfig,
    agent_context_mutation_enabled: bool,
    agent_bootstrap_enabled: bool,
) -> Router {
    if !matches!(db_pool, DbPool::Sqlite(_)) {
        panic!("app_router_sqlite_with_agent_context_and_bootstrap_config requires DbPool::Sqlite");
    }
    let tenant_max_inflight_runs = parse_positive_i64_env("API_TENANT_MAX_INFLIGHT_RUNS");
    let (trusted_proxy_auth_enabled, trusted_proxy_auth_secret, trusted_proxy_auth_error) =
        default_trusted_proxy_auth_config_from_env();
    app_router_sqlite_with_all_limits(
        db_pool,
        tenant_max_inflight_runs,
        agent_context_loader,
        agent_context_mutation_enabled,
        agent_bootstrap_enabled,
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        trusted_proxy_auth_error,
    )
}

fn app_router_sqlite_with_all_limits(
    db_pool: DbPool,
    tenant_max_inflight_runs: Option<i64>,
    agent_context_loader: AgentContextLoaderConfig,
    agent_context_mutation_enabled: bool,
    agent_bootstrap_enabled: bool,
    trusted_proxy_auth_enabled: bool,
    trusted_proxy_auth_secret: Option<String>,
    trusted_proxy_auth_error: Option<String>,
) -> Router {
    Router::new()
        .route("/console", get(console_index_handler))
        .route("/v1/runs", post(create_run_sqlite_handler))
        .route("/v1/triggers", post(create_trigger_sqlite_handler))
        .route(
            "/v1/triggers/cron",
            post(create_cron_trigger_sqlite_handler),
        )
        .route(
            "/v1/triggers/webhook",
            post(create_webhook_trigger_sqlite_handler),
        )
        .route("/v1/triggers/:id", patch(update_trigger_sqlite_handler))
        .route(
            "/v1/triggers/:id/enable",
            post(enable_trigger_sqlite_handler),
        )
        .route(
            "/v1/triggers/:id/disable",
            post(disable_trigger_sqlite_handler),
        )
        .route(
            "/v1/triggers/:id/events",
            post(ingest_trigger_event_sqlite_handler),
        )
        .route(
            "/v1/triggers/:id/events/:event_id/replay",
            post(replay_trigger_event_sqlite_handler),
        )
        .route("/v1/triggers/:id/fire", post(fire_trigger_sqlite_handler))
        .route("/v1/runs/:id", get(get_run_sqlite_handler))
        .route("/v1/runs/:id/audit", get(get_run_audit_sqlite_handler))
        .route(
            "/v1/agents/:id/context",
            get(get_agent_context_sqlite_handler).post(mutate_agent_context_sqlite_handler),
        )
        .route(
            "/v1/agents/:id/bootstrap",
            get(get_agent_bootstrap_sqlite_handler),
        )
        .route(
            "/v1/agents/:id/bootstrap/complete",
            post(complete_agent_bootstrap_sqlite_handler),
        )
        .route(
            "/v1/agents/:id/heartbeat/compile",
            post(compile_agent_heartbeat_sqlite_handler),
        )
        .route(
            "/v1/agents/:id/heartbeat/materialize",
            post(materialize_agent_heartbeat_sqlite_handler),
        )
        .route(
            "/v1/memory/records",
            get(list_memory_records_sqlite_handler).post(create_memory_record_sqlite_handler),
        )
        .route(
            "/v1/memory/handoff-packets",
            get(list_handoff_packets_sqlite_handler).post(create_handoff_packet_sqlite_handler),
        )
        .route("/v1/memory/retrieve", get(retrieve_memory_sqlite_handler))
        .route(
            "/v1/memory/compactions/stats",
            get(get_memory_compaction_stats_sqlite_handler),
        )
        .route(
            "/v1/memory/records/purge-expired",
            post(purge_memory_records_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance",
            get(get_compliance_audit_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/export",
            get(get_compliance_audit_export_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/export",
            get(get_compliance_audit_siem_export_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries",
            get(get_compliance_audit_siem_deliveries_sqlite_handler)
                .post(post_compliance_audit_siem_delivery_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/summary",
            get(get_compliance_audit_siem_deliveries_summary_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/slo",
            get(get_compliance_audit_siem_deliveries_slo_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/targets",
            get(get_compliance_audit_siem_delivery_targets_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/alerts",
            get(get_compliance_audit_siem_delivery_alerts_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/alerts/ack",
            post(ack_compliance_audit_siem_delivery_alert_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/:id/replay",
            post(replay_compliance_audit_siem_delivery_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/policy",
            get(get_compliance_audit_policy_sqlite_handler)
                .put(put_compliance_audit_policy_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/purge",
            post(post_compliance_audit_purge_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/verify",
            get(get_compliance_audit_verify_sqlite_handler),
        )
        .route(
            "/v1/audit/compliance/replay-package",
            get(get_compliance_audit_replay_package_sqlite_handler),
        )
        .route(
            "/v1/payments/summary",
            get(get_payment_summary_sqlite_handler),
        )
        .route("/v1/payments", get(get_payments_sqlite_handler))
        .route(
            "/v1/usage/llm/tokens",
            get(get_llm_usage_tokens_sqlite_handler),
        )
        .route("/v1/ops/summary", get(get_ops_summary_sqlite_handler))
        .route(
            "/v1/ops/llm-gateway",
            get(get_ops_llm_gateway_sqlite_handler),
        )
        .route(
            "/v1/ops/action-latency",
            get(get_ops_action_latency_sqlite_handler),
        )
        .route(
            "/v1/ops/action-latency-traces",
            get(get_ops_action_latency_traces_sqlite_handler),
        )
        .route(
            "/v1/ops/latency-histogram",
            get(get_ops_latency_histogram_sqlite_handler),
        )
        .route(
            "/v1/ops/latency-traces",
            get(get_ops_latency_traces_sqlite_handler),
        )
        .fallback(sqlite_profile_unavailable_handler)
        .with_state(SqliteAppState {
            db_pool,
            tenant_max_inflight_runs,
            agent_context_loader,
            agent_context_mutation_enabled,
            agent_bootstrap_enabled,
            trusted_proxy_auth_enabled,
            trusted_proxy_auth_secret,
            trusted_proxy_auth_error,
        })
}

pub fn app_router_with_tenant_limit(pool: PgPool, tenant_max_inflight_runs: Option<i64>) -> Router {
    let (trusted_proxy_auth_enabled, trusted_proxy_auth_secret, trusted_proxy_auth_error) =
        default_trusted_proxy_auth_config_from_env();
    app_router_with_all_limits(
        pool,
        tenant_max_inflight_runs,
        None,
        None,
        default_api_agent_context_loader_from_env(),
        parse_bool_env("API_AGENT_CONTEXT_MUTATION_ENABLED", false),
        parse_bool_env("API_AGENT_BOOTSTRAP_ENABLED", true),
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        trusted_proxy_auth_error,
    )
}

pub fn app_router_with_limits(
    pool: PgPool,
    tenant_max_inflight_runs: Option<i64>,
    tenant_max_triggers: Option<i64>,
) -> Router {
    let (trusted_proxy_auth_enabled, trusted_proxy_auth_secret, trusted_proxy_auth_error) =
        default_trusted_proxy_auth_config_from_env();
    app_router_with_all_limits(
        pool,
        tenant_max_inflight_runs,
        tenant_max_triggers,
        None,
        default_api_agent_context_loader_from_env(),
        parse_bool_env("API_AGENT_CONTEXT_MUTATION_ENABLED", false),
        parse_bool_env("API_AGENT_BOOTSTRAP_ENABLED", true),
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        trusted_proxy_auth_error,
    )
}

pub fn app_router_with_memory_limit(
    pool: PgPool,
    tenant_max_memory_records: Option<i64>,
) -> Router {
    let (trusted_proxy_auth_enabled, trusted_proxy_auth_secret, trusted_proxy_auth_error) =
        default_trusted_proxy_auth_config_from_env();
    app_router_with_all_limits(
        pool,
        None,
        None,
        tenant_max_memory_records,
        default_api_agent_context_loader_from_env(),
        parse_bool_env("API_AGENT_CONTEXT_MUTATION_ENABLED", false),
        parse_bool_env("API_AGENT_BOOTSTRAP_ENABLED", true),
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        trusted_proxy_auth_error,
    )
}

pub fn app_router_with_trusted_proxy_auth(
    pool: PgPool,
    trusted_proxy_auth_enabled: bool,
    trusted_proxy_auth_secret: Option<String>,
) -> Router {
    app_router_with_all_limits(
        pool,
        None,
        None,
        None,
        default_api_agent_context_loader_from_env(),
        parse_bool_env("API_AGENT_CONTEXT_MUTATION_ENABLED", false),
        parse_bool_env("API_AGENT_BOOTSTRAP_ENABLED", true),
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        None,
    )
}

pub fn app_router_with_agent_context_config(
    pool: PgPool,
    agent_context_loader: AgentContextLoaderConfig,
    agent_context_mutation_enabled: bool,
) -> Router {
    app_router_with_agent_context_and_bootstrap_config(
        pool,
        agent_context_loader,
        agent_context_mutation_enabled,
        true,
    )
}

pub fn app_router_with_agent_context_and_bootstrap_config(
    pool: PgPool,
    agent_context_loader: AgentContextLoaderConfig,
    agent_context_mutation_enabled: bool,
    agent_bootstrap_enabled: bool,
) -> Router {
    let (trusted_proxy_auth_enabled, trusted_proxy_auth_secret, trusted_proxy_auth_error) =
        default_trusted_proxy_auth_config_from_env();
    app_router_with_all_limits(
        pool,
        None,
        None,
        None,
        agent_context_loader,
        agent_context_mutation_enabled,
        agent_bootstrap_enabled,
        trusted_proxy_auth_enabled,
        trusted_proxy_auth_secret,
        trusted_proxy_auth_error,
    )
}

fn app_router_with_all_limits(
    pool: PgPool,
    tenant_max_inflight_runs: Option<i64>,
    tenant_max_triggers: Option<i64>,
    tenant_max_memory_records: Option<i64>,
    agent_context_loader: AgentContextLoaderConfig,
    agent_context_mutation_enabled: bool,
    agent_bootstrap_enabled: bool,
    trusted_proxy_auth_enabled: bool,
    trusted_proxy_auth_secret: Option<String>,
    trusted_proxy_auth_error: Option<String>,
) -> Router {
    Router::new()
        .route("/console", get(console_index_handler))
        .route("/v1/runs", post(create_run_handler))
        .route("/v1/triggers", post(create_trigger_handler))
        .route("/v1/triggers/cron", post(create_cron_trigger_handler))
        .route("/v1/triggers/webhook", post(create_webhook_trigger_handler))
        .route("/v1/triggers/:id", patch(update_trigger_handler))
        .route("/v1/triggers/:id/enable", post(enable_trigger_handler))
        .route("/v1/triggers/:id/disable", post(disable_trigger_handler))
        .route(
            "/v1/triggers/:id/events",
            post(ingest_trigger_event_handler),
        )
        .route(
            "/v1/triggers/:id/events/:event_id/replay",
            post(replay_trigger_event_handler),
        )
        .route("/v1/triggers/:id/fire", post(fire_trigger_handler))
        .route("/v1/runs/:id", get(get_run_handler))
        .route("/v1/runs/:id/audit", get(get_run_audit_handler))
        .route(
            "/v1/agents/:id/context",
            get(get_agent_context_handler).post(mutate_agent_context_handler),
        )
        .route("/v1/agents/:id/bootstrap", get(get_agent_bootstrap_handler))
        .route(
            "/v1/agents/:id/bootstrap/complete",
            post(complete_agent_bootstrap_handler),
        )
        .route(
            "/v1/agents/:id/heartbeat/compile",
            post(compile_agent_heartbeat_handler),
        )
        .route(
            "/v1/agents/:id/heartbeat/materialize",
            post(materialize_agent_heartbeat_handler),
        )
        .route(
            "/v1/memory/records",
            get(list_memory_records_handler).post(create_memory_record_handler),
        )
        .route(
            "/v1/memory/handoff-packets",
            get(list_handoff_packets_handler).post(create_handoff_packet_handler),
        )
        .route("/v1/memory/retrieve", get(retrieve_memory_handler))
        .route(
            "/v1/memory/compactions/stats",
            get(get_memory_compaction_stats_handler),
        )
        .route(
            "/v1/memory/records/purge-expired",
            post(purge_memory_records_handler),
        )
        .route("/v1/audit/compliance", get(get_compliance_audit_handler))
        .route(
            "/v1/audit/compliance/siem/export",
            get(get_compliance_audit_siem_export_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries",
            get(get_compliance_audit_siem_deliveries_handler)
                .post(post_compliance_audit_siem_delivery_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/summary",
            get(get_compliance_audit_siem_deliveries_summary_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/slo",
            get(get_compliance_audit_siem_deliveries_slo_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/targets",
            get(get_compliance_audit_siem_delivery_targets_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/alerts",
            get(get_compliance_audit_siem_delivery_alerts_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/alerts/ack",
            post(ack_compliance_audit_siem_delivery_alert_handler),
        )
        .route(
            "/v1/audit/compliance/siem/deliveries/:id/replay",
            post(replay_compliance_audit_siem_delivery_handler),
        )
        .route(
            "/v1/audit/compliance/policy",
            get(get_compliance_audit_policy_handler).put(put_compliance_audit_policy_handler),
        )
        .route(
            "/v1/audit/compliance/verify",
            get(get_compliance_audit_verify_handler),
        )
        .route(
            "/v1/audit/compliance/purge",
            post(post_compliance_audit_purge_handler),
        )
        .route(
            "/v1/audit/compliance/export",
            get(get_compliance_audit_export_handler),
        )
        .route(
            "/v1/audit/compliance/replay-package",
            get(get_compliance_audit_replay_package_handler),
        )
        .route("/v1/payments/summary", get(get_payment_summary_handler))
        .route("/v1/payments", get(get_payments_handler))
        .route("/v1/usage/llm/tokens", get(get_llm_usage_tokens_handler))
        .route("/v1/ops/summary", get(get_ops_summary_handler))
        .route("/v1/ops/llm-gateway", get(get_ops_llm_gateway_handler))
        .route(
            "/v1/ops/action-latency",
            get(get_ops_action_latency_handler),
        )
        .route(
            "/v1/ops/action-latency-traces",
            get(get_ops_action_latency_traces_handler),
        )
        .route(
            "/v1/ops/latency-histogram",
            get(get_ops_latency_histogram_handler),
        )
        .route(
            "/v1/ops/latency-traces",
            get(get_ops_latency_traces_handler),
        )
        .with_state(AppState {
            db_pool: DbPool::Postgres(pool.clone()),
            pool,
            tenant_max_inflight_runs,
            tenant_max_triggers,
            tenant_max_memory_records,
            agent_context_loader,
            agent_context_mutation_enabled,
            agent_bootstrap_enabled,
            trusted_proxy_auth_enabled,
            trusted_proxy_auth_secret,
            trusted_proxy_auth_error,
        })
}

async fn console_index_handler() -> impl IntoResponse {
    Html(CONSOLE_INDEX_HTML)
}

async fn sqlite_profile_unavailable_handler(uri: Uri) -> ApiError {
    ApiError {
        status: StatusCode::NOT_IMPLEMENTED,
        code: "SQLITE_PROFILE_ENDPOINT_UNAVAILABLE",
        message: format!(
            "endpoint `{}` is not enabled in the current sqlite API profile",
            uri.path()
        ),
    }
}

fn parse_positive_i64_env(key: &str) -> Option<i64> {
    env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
}

fn parse_positive_usize_env(key: &str) -> Option<usize> {
    env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}

fn parse_bool_env(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .and_then(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        })
        .unwrap_or(default)
}

fn normalize_optional_env_var(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_env_csv(key: &str) -> Vec<String> {
    env::var(key)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn default_api_agent_context_loader_from_env() -> AgentContextLoaderConfig {
    let configured =
        normalize_agent_context_required_files(&read_env_csv("API_AGENT_CONTEXT_REQUIRED_FILES"));
    let required_files = if configured.is_empty() {
        default_agent_context_required_files()
    } else {
        configured
    };
    AgentContextLoaderConfig {
        root_dir: PathBuf::from(
            env::var("API_AGENT_CONTEXT_ROOT").unwrap_or_else(|_| "agent_context".to_string()),
        ),
        required_files,
        max_file_bytes: parse_positive_usize_env("API_AGENT_CONTEXT_MAX_FILE_BYTES")
            .unwrap_or(64 * 1024),
        max_total_bytes: parse_positive_usize_env("API_AGENT_CONTEXT_MAX_TOTAL_BYTES")
            .unwrap_or(256 * 1024),
        max_dynamic_files_per_dir: parse_positive_usize_env(
            "API_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR",
        )
        .unwrap_or(8),
    }
}

fn resolve_trusted_proxy_auth_secret_from_env() -> (Option<String>, Option<String>) {
    let secret = normalize_optional_env_var("API_TRUSTED_PROXY_SHARED_SECRET");
    let secret_ref = normalize_optional_env_var("API_TRUSTED_PROXY_SHARED_SECRET_REF");
    if secret.is_none() && secret_ref.is_none() {
        return (None, None);
    }
    match resolve_secret_value(secret, secret_ref, shared_secret_resolver()) {
        Ok(value) => (
            value
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty()),
            None,
        ),
        Err(err) => (None, Some(err.to_string())),
    }
}

fn default_trusted_proxy_auth_config_from_env() -> (bool, Option<String>, Option<String>) {
    let enabled = parse_bool_env("API_TRUSTED_PROXY_AUTH_ENABLED", false);
    let (secret, error) = resolve_trusted_proxy_auth_secret_from_env();
    (enabled, secret, error)
}

#[derive(Debug, Deserialize)]
struct CreateRunRequest {
    agent_id: Uuid,
    #[serde(default)]
    triggered_by_user_id: Option<Uuid>,
    recipe_id: String,
    input: Value,
    #[serde(default = "default_json_array")]
    requested_capabilities: Value,
}

#[derive(Debug, Deserialize)]
struct CreateTriggerRequest {
    agent_id: Uuid,
    #[serde(default)]
    triggered_by_user_id: Option<Uuid>,
    recipe_id: String,
    input: Value,
    #[serde(default = "default_json_array")]
    requested_capabilities: Value,
    interval_seconds: i64,
    #[serde(default = "default_trigger_max_inflight_runs")]
    max_inflight_runs: i32,
    #[serde(default)]
    jitter_seconds: i32,
}

#[derive(Debug, Deserialize)]
struct CreateWebhookTriggerRequest {
    agent_id: Uuid,
    #[serde(default)]
    triggered_by_user_id: Option<Uuid>,
    recipe_id: String,
    input: Value,
    #[serde(default = "default_json_array")]
    requested_capabilities: Value,
    #[serde(default)]
    webhook_secret_ref: Option<String>,
    #[serde(default = "default_trigger_max_attempts")]
    max_attempts: i32,
    #[serde(default = "default_trigger_max_inflight_runs")]
    max_inflight_runs: i32,
    #[serde(default)]
    jitter_seconds: i32,
}

#[derive(Debug, Deserialize)]
struct CreateCronTriggerRequest {
    agent_id: Uuid,
    #[serde(default)]
    triggered_by_user_id: Option<Uuid>,
    recipe_id: String,
    input: Value,
    #[serde(default = "default_json_array")]
    requested_capabilities: Value,
    cron_expression: String,
    #[serde(default = "default_trigger_timezone")]
    schedule_timezone: String,
    #[serde(default = "default_trigger_max_attempts")]
    max_attempts: i32,
    #[serde(default = "default_trigger_max_inflight_runs")]
    max_inflight_runs: i32,
    #[serde(default)]
    jitter_seconds: i32,
}

#[derive(Debug, Deserialize)]
struct TriggerEventRequest {
    event_id: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct FireTriggerRequest {
    idempotency_key: String,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct UpdateTriggerRequest {
    #[serde(default)]
    interval_seconds: Option<i64>,
    #[serde(default)]
    cron_expression: Option<String>,
    #[serde(default)]
    schedule_timezone: Option<String>,
    #[serde(default)]
    misfire_policy: Option<String>,
    #[serde(default)]
    max_attempts: Option<i32>,
    #[serde(default)]
    max_inflight_runs: Option<i32>,
    #[serde(default)]
    jitter_seconds: Option<i32>,
    #[serde(default)]
    webhook_secret_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateMemoryRecordRequest {
    agent_id: Uuid,
    run_id: Option<Uuid>,
    step_id: Option<Uuid>,
    memory_kind: String,
    scope: String,
    content_json: Value,
    summary_text: Option<String>,
    source: Option<String>,
    redaction_applied: Option<bool>,
    expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize)]
struct CreateHandoffPacketRequest {
    to_agent_id: Uuid,
    from_agent_id: Option<Uuid>,
    run_id: Option<Uuid>,
    step_id: Option<Uuid>,
    title: String,
    payload_json: Value,
    source: Option<String>,
    expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize)]
struct MutateAgentContextRequest {
    relative_path: String,
    content: String,
    #[serde(default = "default_context_mutation_mode")]
    mode: String,
}

#[derive(Debug, Deserialize)]
struct CompleteBootstrapRequest {
    #[serde(default)]
    identity_markdown: Option<String>,
    #[serde(default)]
    soul_markdown: Option<String>,
    #[serde(default)]
    user_markdown: Option<String>,
    #[serde(default)]
    heartbeat_markdown: Option<String>,
    #[serde(default)]
    completion_note: Option<String>,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct CompileHeartbeatRequest {
    #[serde(default)]
    heartbeat_markdown: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MaterializeHeartbeatRequest {
    #[serde(default)]
    heartbeat_markdown: Option<String>,
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    approval_confirmed: bool,
    #[serde(default)]
    approval_note: Option<String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    requested_capabilities: Option<Value>,
    #[serde(default)]
    triggered_by_user_id: Option<Uuid>,
    #[serde(default)]
    cron_max_attempts: Option<i32>,
}

#[derive(Debug, Serialize)]
struct AgentContextFileResponse {
    slot: String,
    relative_path: String,
    sha256: String,
    bytes: usize,
    mutability: Option<AgentContextMutability>,
}

#[derive(Debug, Serialize)]
struct AgentContextInspectResponse {
    tenant_id: String,
    agent_id: Uuid,
    source_dir: String,
    loaded_at: OffsetDateTime,
    loaded_file_count: usize,
    total_loaded_bytes: usize,
    aggregate_sha256: String,
    summary_digest_sha256: String,
    missing_required_files: Vec<String>,
    warnings: Vec<String>,
    precedence_order: Vec<String>,
    required_files: Vec<AgentContextFileResponse>,
    memory_files: Vec<AgentContextFileResponse>,
    session_files: Vec<AgentContextFileResponse>,
}

#[derive(Debug, Serialize)]
struct AgentHeartbeatCompileResponse {
    tenant_id: String,
    agent_id: Uuid,
    source: String,
    source_path: Option<String>,
    context_aggregate_sha256: Option<String>,
    context_summary_digest_sha256: Option<String>,
    candidate_count: usize,
    issue_count: usize,
    candidates: Vec<agent_core::HeartbeatTriggerCandidate>,
    issues: Vec<agent_core::HeartbeatCompileIssue>,
}

#[derive(Debug, Serialize)]
struct AgentHeartbeatMaterializeItemResponse {
    line: usize,
    kind: String,
    recipe_id: String,
    interval_seconds: Option<i64>,
    cron_expression: Option<String>,
    timezone: Option<String>,
    max_inflight_runs: i32,
    jitter_seconds: i32,
    status: String,
    trigger_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct AgentHeartbeatMaterializeResponse {
    tenant_id: String,
    agent_id: Uuid,
    source: String,
    source_path: Option<String>,
    context_aggregate_sha256: Option<String>,
    context_summary_digest_sha256: Option<String>,
    apply_requested: bool,
    approval_confirmed: bool,
    approval_note: Option<String>,
    approved_by_user_id: Option<Uuid>,
    cron_max_attempts: i32,
    candidate_count: usize,
    issue_count: usize,
    planned_count: usize,
    created_count: usize,
    existing_count: usize,
    candidates: Vec<AgentHeartbeatMaterializeItemResponse>,
    issues: Vec<agent_core::HeartbeatCompileIssue>,
}

#[derive(Debug, Serialize)]
struct AgentContextMutationResponse {
    tenant_id: String,
    agent_id: Uuid,
    relative_path: String,
    mode: String,
    mutability: AgentContextMutability,
    sha256: String,
    bytes: usize,
}

#[derive(Debug, Serialize)]
struct AgentBootstrapInspectResponse {
    tenant_id: String,
    agent_id: Uuid,
    enabled: bool,
    status: String,
    source_dir: String,
    bootstrap_present: bool,
    bootstrap_path: Option<String>,
    bootstrap_sha256: Option<String>,
    bootstrap_bytes: Option<usize>,
    bootstrap_markdown: Option<String>,
    completed_at: Option<OffsetDateTime>,
    completed_by_user_id: Option<Uuid>,
    completion_note: Option<String>,
    updated_files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BootstrapFileWriteResponse {
    relative_path: String,
    sha256: String,
    bytes: usize,
}

#[derive(Debug, Serialize)]
struct AgentBootstrapCompleteResponse {
    tenant_id: String,
    agent_id: Uuid,
    status: String,
    source_dir: String,
    completed_at: OffsetDateTime,
    completed_by_user_id: Uuid,
    completion_note: Option<String>,
    force: bool,
    updated_files: Vec<BootstrapFileWriteResponse>,
    status_record_relative_path: String,
}

#[derive(Debug, Serialize)]
struct RunResponse {
    id: Uuid,
    tenant_id: String,
    agent_id: Uuid,
    triggered_by_user_id: Option<Uuid>,
    recipe_id: String,
    status: String,
    trace_id: Option<String>,
    requested_capabilities: Value,
    granted_capabilities: Value,
    created_at: OffsetDateTime,
    started_at: Option<OffsetDateTime>,
    finished_at: Option<OffsetDateTime>,
    error_json: Option<Value>,
    attempts: i32,
    lease_owner: Option<String>,
    lease_expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct TriggerResponse {
    id: Uuid,
    tenant_id: String,
    agent_id: Uuid,
    triggered_by_user_id: Option<Uuid>,
    recipe_id: String,
    status: String,
    trigger_type: String,
    interval_seconds: Option<i64>,
    cron_expression: Option<String>,
    schedule_timezone: String,
    misfire_policy: String,
    max_attempts: i32,
    max_inflight_runs: i32,
    jitter_seconds: i32,
    consecutive_failures: i32,
    dead_lettered_at: Option<OffsetDateTime>,
    dead_letter_reason: Option<String>,
    webhook_secret_configured: bool,
    input_json: Value,
    requested_capabilities: Value,
    granted_capabilities: Value,
    next_fire_at: OffsetDateTime,
    last_fired_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct TriggerEventIngestResponse {
    trigger_id: Uuid,
    event_id: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct TriggerFireResponse {
    trigger_id: Uuid,
    run_id: Option<Uuid>,
    idempotency_key: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct AuditEventResponse {
    id: Uuid,
    run_id: Uuid,
    step_id: Option<Uuid>,
    actor: String,
    event_type: String,
    payload_json: Value,
    created_at: OffsetDateTime,
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditQuery {
    limit: Option<i64>,
    run_id: Option<Uuid>,
    event_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditExportQuery {
    limit: Option<i64>,
    run_id: Option<Uuid>,
    event_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditSiemExportQuery {
    limit: Option<i64>,
    run_id: Option<Uuid>,
    event_type: Option<String>,
    adapter: Option<String>,
    elastic_index: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditSiemDeliveriesQuery {
    limit: Option<i64>,
    run_id: Option<Uuid>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditSiemDeliverySummaryQuery {
    run_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditSiemDeliverySloQuery {
    run_id: Option<Uuid>,
    window_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditSiemDeliveryTargetsQuery {
    run_id: Option<Uuid>,
    window_secs: Option<u64>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditSiemDeliveryAlertsQuery {
    run_id: Option<Uuid>,
    window_secs: Option<u64>,
    limit: Option<i64>,
    max_hard_failure_rate_pct: Option<f64>,
    max_dead_letter_rate_pct: Option<f64>,
    max_pending_count: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditSiemDeliveryRequest {
    limit: Option<i64>,
    run_id: Option<Uuid>,
    event_type: Option<String>,
    adapter: Option<String>,
    elastic_index: Option<String>,
    delivery_target: String,
    max_attempts: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ReplayComplianceAuditSiemDeliveryRequest {
    delay_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AckComplianceAuditSiemDeliveryAlertRequest {
    run_id: Option<Uuid>,
    delivery_target: String,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ComplianceAuditReplayPackageQuery {
    run_id: Uuid,
    audit_limit: Option<i64>,
    compliance_limit: Option<i64>,
    payment_limit: Option<i64>,
    include_payments: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateComplianceAuditPolicyRequest {
    compliance_hot_retention_days: Option<i32>,
    compliance_archive_retention_days: Option<i32>,
    legal_hold: Option<bool>,
    legal_hold_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlmUsageQuery {
    window_secs: Option<u64>,
    agent_id: Option<Uuid>,
    model_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PaymentLedgerQuery {
    limit: Option<i64>,
    run_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    status: Option<String>,
    destination: Option<String>,
    idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PaymentSummaryQuery {
    window_secs: Option<u64>,
    agent_id: Option<Uuid>,
    operation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryRecordQuery {
    limit: Option<i64>,
    agent_id: Option<Uuid>,
    memory_kind: Option<String>,
    scope_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HandoffPacketQuery {
    limit: Option<i64>,
    to_agent_id: Option<Uuid>,
    from_agent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct MemoryRetrieveQuery {
    limit: Option<i64>,
    agent_id: Option<Uuid>,
    memory_kind: Option<String>,
    scope_prefix: Option<String>,
    query_text: Option<String>,
    min_score: Option<f64>,
    source_prefix: Option<String>,
    require_summary: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MemoryCompactionStatsQuery {
    window_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PurgeMemoryRecordsRequest {
    as_of: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize)]
struct OpsSummaryQuery {
    window_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpsLlmGatewayQuery {
    window_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpsLatencyHistogramQuery {
    window_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpsActionLatencyQuery {
    window_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OpsActionLatencyTracesQuery {
    window_secs: Option<u64>,
    limit: Option<i64>,
    action_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpsLatencyTracesQuery {
    window_secs: Option<u64>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct LlmUsageResponse {
    tenant_id: String,
    window_secs: u64,
    since: OffsetDateTime,
    tokens: i64,
    estimated_cost_usd: f64,
    agent_id: Option<Uuid>,
    model_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct PaymentLedgerResponse {
    id: Uuid,
    action_request_id: Uuid,
    run_id: Uuid,
    tenant_id: String,
    agent_id: Uuid,
    provider: String,
    operation: String,
    destination: String,
    idempotency_key: String,
    amount_msat: Option<i64>,
    status: String,
    request_json: Value,
    latest_result_status: Option<String>,
    latest_result_json: Option<Value>,
    latest_error_json: Option<Value>,
    settlement_status: Option<String>,
    settlement_rail: Option<String>,
    normalized_outcome: String,
    normalized_error_code: Option<String>,
    normalized_error_class: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    latest_result_created_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct PaymentSummaryResponse {
    tenant_id: String,
    window_secs: Option<u64>,
    since: Option<OffsetDateTime>,
    agent_id: Option<Uuid>,
    operation: Option<String>,
    total_requests: i64,
    requested_count: i64,
    executed_count: i64,
    failed_count: i64,
    duplicate_count: i64,
    executed_spend_msat: i64,
}

#[derive(Debug, Serialize)]
struct OpsSummaryResponse {
    tenant_id: String,
    window_secs: u64,
    since: OffsetDateTime,
    queued_runs: i64,
    running_runs: i64,
    tenant_inflight_runs: i64,
    tenant_inflight_pressure: Option<f64>,
    tenant_inflight_cap: Option<i64>,
    global_inflight_runs: i64,
    succeeded_runs_window: i64,
    failed_runs_window: i64,
    dead_letter_trigger_events_window: i64,
    avg_run_duration_ms: Option<f64>,
    p95_run_duration_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct OpsLlmGatewayLaneResponse {
    request_class: String,
    total_count: i64,
    avg_duration_ms: Option<f64>,
    p95_duration_ms: Option<f64>,
    cache_hit_count: i64,
    distributed_cache_hit_count: i64,
    cache_hit_rate_pct: Option<f64>,
    verifier_escalated_count: i64,
    verifier_escalated_rate_pct: Option<f64>,
    slo_warn_count: i64,
    slo_breach_count: i64,
    distributed_fail_open_count: i64,
}

#[derive(Debug, Serialize)]
struct OpsLlmGatewayResponse {
    tenant_id: String,
    window_secs: u64,
    since: OffsetDateTime,
    total_count: i64,
    lanes: Vec<OpsLlmGatewayLaneResponse>,
}

#[derive(Debug, Serialize)]
struct OpsLatencyHistogramBucketResponse {
    bucket_label: String,
    lower_bound_ms: i64,
    upper_bound_exclusive_ms: Option<i64>,
    run_count: i64,
}

#[derive(Debug, Serialize)]
struct OpsLatencyHistogramResponse {
    tenant_id: String,
    window_secs: u64,
    since: OffsetDateTime,
    buckets: Vec<OpsLatencyHistogramBucketResponse>,
}

#[derive(Debug, Serialize)]
struct OpsActionLatencyEntryResponse {
    action_type: String,
    total_count: i64,
    avg_duration_ms: Option<f64>,
    p95_duration_ms: Option<f64>,
    max_duration_ms: Option<i64>,
    failed_count: i64,
    denied_count: i64,
}

#[derive(Debug, Serialize)]
struct OpsActionLatencyResponse {
    tenant_id: String,
    window_secs: u64,
    since: OffsetDateTime,
    actions: Vec<OpsActionLatencyEntryResponse>,
}

#[derive(Debug, Serialize)]
struct OpsActionLatencyTraceEntryResponse {
    action_request_id: Uuid,
    run_id: Uuid,
    step_id: Uuid,
    action_type: String,
    status: String,
    duration_ms: i64,
    created_at: OffsetDateTime,
    executed_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct OpsActionLatencyTracesResponse {
    tenant_id: String,
    window_secs: u64,
    since: OffsetDateTime,
    limit: i64,
    action_type: Option<String>,
    traces: Vec<OpsActionLatencyTraceEntryResponse>,
}

#[derive(Debug, Serialize)]
struct OpsLatencyTraceResponse {
    run_id: Uuid,
    status: String,
    duration_ms: i64,
    started_at: OffsetDateTime,
    finished_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct OpsLatencyTracesResponse {
    tenant_id: String,
    window_secs: u64,
    since: OffsetDateTime,
    limit: i64,
    traces: Vec<OpsLatencyTraceResponse>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditEventResponse {
    id: Uuid,
    source_audit_event_id: Uuid,
    tamper_chain_seq: i64,
    tamper_prev_hash: Option<String>,
    tamper_hash: String,
    run_id: Uuid,
    step_id: Option<Uuid>,
    tenant_id: String,
    agent_id: Option<Uuid>,
    user_id: Option<Uuid>,
    actor: String,
    event_type: String,
    payload_json: Value,
    request_id: Option<String>,
    session_id: Option<String>,
    action_request_id: Option<Uuid>,
    payment_request_id: Option<Uuid>,
    created_at: OffsetDateTime,
    recorded_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditVerifyResponse {
    tenant_id: String,
    checked_events: i64,
    verified: bool,
    first_invalid_event_id: Option<Uuid>,
    latest_chain_seq: Option<i64>,
    latest_tamper_hash: Option<String>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditPolicyResponse {
    tenant_id: String,
    compliance_hot_retention_days: i32,
    compliance_archive_retention_days: i32,
    legal_hold: bool,
    legal_hold_reason: Option<String>,
    updated_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditPurgeResponse {
    tenant_id: String,
    deleted_count: i64,
    legal_hold: bool,
    cutoff_at: OffsetDateTime,
    compliance_hot_retention_days: i32,
    compliance_archive_retention_days: i32,
}

#[derive(Debug, Serialize)]
struct MemoryRecordResponse {
    id: Uuid,
    tenant_id: String,
    agent_id: Uuid,
    run_id: Option<Uuid>,
    step_id: Option<Uuid>,
    memory_kind: String,
    scope: String,
    content_json: Value,
    summary_text: Option<String>,
    source: String,
    redaction_applied: bool,
    expires_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct HandoffPacketResponse {
    packet_id: Uuid,
    memory_id: Uuid,
    tenant_id: String,
    from_agent_id: Uuid,
    to_agent_id: Uuid,
    run_id: Option<Uuid>,
    step_id: Option<Uuid>,
    scope: String,
    title: String,
    payload_json: Value,
    source: String,
    redaction_applied: bool,
    expires_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct MemoryPurgeResponse {
    tenant_id: String,
    deleted_count: i64,
    as_of: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct MemoryCitation {
    memory_id: Uuid,
    created_at: OffsetDateTime,
    source: String,
    memory_kind: String,
    scope: String,
}

#[derive(Debug, Serialize)]
struct MemoryRetrievalItem {
    rank: i64,
    score: f64,
    citation: MemoryCitation,
    content_json: Value,
    summary_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct MemoryRetrieveResponse {
    tenant_id: String,
    limit: i64,
    retrieved_count: i64,
    agent_id: Option<Uuid>,
    memory_kind: Option<String>,
    scope_prefix: Option<String>,
    query_text: Option<String>,
    min_score: Option<f64>,
    source_prefix: Option<String>,
    require_summary: bool,
    items: Vec<MemoryRetrievalItem>,
}

#[derive(Debug, Serialize)]
struct MemoryCompactionStatsResponse {
    tenant_id: String,
    window_secs: Option<u64>,
    since: Option<OffsetDateTime>,
    compacted_groups_window: i64,
    compacted_source_records_window: i64,
    pending_uncompacted_records: i64,
    last_compacted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct ComplianceReplayCorrelationSummary {
    run_audit_event_count: usize,
    compliance_event_count: usize,
    payment_event_count: usize,
    first_event_at: Option<OffsetDateTime>,
    last_event_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct ComplianceReplayPackageManifest {
    version: String,
    digest_sha256: String,
    signing_mode: String,
    signature: Option<String>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditReplayPackageResponse {
    tenant_id: String,
    run: RunResponse,
    generated_at: OffsetDateTime,
    run_audit_events: Vec<AuditEventResponse>,
    compliance_audit_events: Vec<ComplianceAuditEventResponse>,
    payment_ledger: Vec<PaymentLedgerResponse>,
    correlation: ComplianceReplayCorrelationSummary,
    manifest: ComplianceReplayPackageManifest,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliveryResponse {
    id: Uuid,
    tenant_id: String,
    run_id: Option<Uuid>,
    adapter: String,
    delivery_target: String,
    status: String,
    attempts: i32,
    max_attempts: i32,
    next_attempt_at: OffsetDateTime,
    created_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliveryItemResponse {
    id: Uuid,
    tenant_id: String,
    run_id: Option<Uuid>,
    adapter: String,
    delivery_target: String,
    status: String,
    attempts: i32,
    max_attempts: i32,
    next_attempt_at: OffsetDateTime,
    leased_by: Option<String>,
    lease_expires_at: Option<OffsetDateTime>,
    last_error: Option<String>,
    last_http_status: Option<i32>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    delivered_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliverySummaryResponse {
    tenant_id: String,
    run_id: Option<Uuid>,
    pending_count: i64,
    processing_count: i64,
    failed_count: i64,
    delivered_count: i64,
    dead_lettered_count: i64,
    oldest_pending_age_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliverySloResponse {
    tenant_id: String,
    run_id: Option<Uuid>,
    window_secs: u64,
    since: OffsetDateTime,
    total_count: i64,
    pending_count: i64,
    processing_count: i64,
    failed_count: i64,
    delivered_count: i64,
    dead_lettered_count: i64,
    delivery_success_rate_pct: Option<f64>,
    hard_failure_rate_pct: Option<f64>,
    dead_letter_rate_pct: Option<f64>,
    oldest_pending_age_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliveryTargetSummaryResponse {
    delivery_target: String,
    total_count: i64,
    pending_count: i64,
    processing_count: i64,
    failed_count: i64,
    delivered_count: i64,
    dead_lettered_count: i64,
    last_error: Option<String>,
    last_http_status: Option<i32>,
    last_attempt_at: Option<OffsetDateTime>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliveryAlertResponse {
    tenant_id: String,
    run_id: Option<Uuid>,
    window_secs: u64,
    since: OffsetDateTime,
    thresholds: ComplianceAuditSiemDeliveryAlertThresholdsResponse,
    alerts: Vec<ComplianceAuditSiemDeliveryAlertItemResponse>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliveryAlertThresholdsResponse {
    max_hard_failure_rate_pct: Option<f64>,
    max_dead_letter_rate_pct: Option<f64>,
    max_pending_count: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliveryAlertItemResponse {
    delivery_target: String,
    total_count: i64,
    pending_count: i64,
    processing_count: i64,
    failed_count: i64,
    delivered_count: i64,
    dead_lettered_count: i64,
    hard_failure_rate_pct: Option<f64>,
    dead_letter_rate_pct: Option<f64>,
    triggered_rules: Vec<String>,
    severity: String,
    last_error: Option<String>,
    last_http_status: Option<i32>,
    last_attempt_at: Option<OffsetDateTime>,
    acknowledged: bool,
    acknowledged_at: Option<OffsetDateTime>,
    acknowledged_by_user_id: Option<Uuid>,
    acknowledged_by_role: Option<String>,
    acknowledgement_note: Option<String>,
}

#[derive(Debug, Serialize)]
struct ComplianceAuditSiemDeliveryAlertAckResponse {
    tenant_id: String,
    run_scope: String,
    run_id: Option<Uuid>,
    delivery_target: String,
    acknowledged_by_user_id: Uuid,
    acknowledged_by_role: String,
    acknowledgement_note: Option<String>,
    created_at: OffsetDateTime,
    acknowledged_at: OffsetDateTime,
}

fn default_context_mutation_mode() -> String {
    "replace".to_string()
}

fn normalize_requested_capabilities_payload(payload: Option<Value>) -> ApiResult<Value> {
    let normalized = payload.unwrap_or_else(|| Value::Array(Vec::new()));
    if normalized.is_null() {
        return Ok(Value::Array(Vec::new()));
    }
    if !normalized.is_array() {
        return Err(ApiError::bad_request(
            "requested_capabilities must be an array",
        ));
    }
    Ok(normalized)
}

fn normalize_materialization_input_payload(payload: Option<Value>) -> Value {
    let value = payload.unwrap_or_else(|| Value::Object(Default::default()));
    if value.is_null() {
        return Value::Object(Default::default());
    }
    value
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "BAD_REQUEST",
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHORIZED",
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN",
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "CONFLICT",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL",
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

async fn create_run_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateRunRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    if let Some(limit) = state.tenant_max_inflight_runs {
        let inflight = count_tenant_inflight_runs_dual(&state.db_pool, tenant_id.as_str())
            .await
            .map_err(|err| {
                ApiError::internal(format!("failed counting tenant inflight runs: {err}"))
            })?;
        if inflight >= limit {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "TENANT_INFLIGHT_LIMITED",
                message: format!(
                    "tenant is at max inflight run capacity (limit={}, inflight={})",
                    limit, inflight
                ),
            });
        }
    }
    create_run_common(
        &state.db_pool,
        tenant_id,
        role_preset,
        req,
    )
    .await
}

async fn create_run_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<CreateRunRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    if let Some(limit) = state.tenant_max_inflight_runs {
        let inflight = count_tenant_inflight_runs_dual(&state.db_pool, tenant_id.as_str())
            .await
            .map_err(|err| {
                ApiError::internal(format!("failed counting tenant inflight runs: {err}"))
            })?;
        if inflight >= limit {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "TENANT_INFLIGHT_LIMITED",
                message: format!(
                    "tenant is at max inflight run capacity (limit={}, inflight={})",
                    limit, inflight
                ),
            });
        }
    }
    create_run_common(
        &state.db_pool,
        tenant_id,
        role_preset,
        req,
    )
    .await
}

async fn create_run_common(
    db_pool: &DbPool,
    tenant_id: String,
    role_preset: RolePreset,
    req: CreateRunRequest,
) -> ApiResult<(StatusCode, Json<RunResponse>)> {
    let CreateRunRequest {
        agent_id,
        triggered_by_user_id,
        recipe_id,
        input,
        requested_capabilities,
    } = req;

    let granted_capabilities =
        resolve_granted_capabilities(recipe_id.as_str(), role_preset, &requested_capabilities)?;
    let semantic_dedupe_key = compute_run_semantic_dedupe_key(
        &tenant_id,
        agent_id,
        triggered_by_user_id,
        recipe_id.as_str(),
        &input,
        &requested_capabilities,
        role_preset,
    )?;

    let make_new_run = |run_id: Uuid| -> NewRun {
        let run_trace_id = run_id.to_string();
        NewRun {
            id: run_id,
            tenant_id: tenant_id.clone(),
            agent_id,
            triggered_by_user_id,
            recipe_id: recipe_id.clone(),
            status: "queued".to_string(),
            input_json: inject_trace_id_into_input(input.clone(), &run_trace_id),
            requested_capabilities: requested_capabilities.clone(),
            granted_capabilities: granted_capabilities.clone(),
            error_json: None,
        }
    };

    let run_id = Uuid::new_v4();
    let created = create_run_with_semantic_dedupe_key_dual(
        db_pool,
        &make_new_run(run_id),
        &semantic_dedupe_key,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed creating run: {err}")))?;

    let (run_id, status_code, created_new_run) = match created {
        Some(created) => (created.id, StatusCode::CREATED, true),
        None => {
            let existing_run_id = get_active_run_id_by_semantic_dedupe_key_dual(
                db_pool,
                tenant_id.as_str(),
                &semantic_dedupe_key,
            )
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed checking active run for semantic dedupe key: {err}"
                ))
            })?
            .ok_or_else(|| {
                ApiError::conflict("run already exists in race with completion; retry request")
            })?;
            (existing_run_id, StatusCode::OK, false)
        }
    };

    if created_new_run {
        append_audit_event_dual(
            db_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: None,
                tenant_id: tenant_id.clone(),
                agent_id: Some(agent_id),
                user_id: triggered_by_user_id,
                actor: "api".to_string(),
                event_type: "run.created".to_string(),
                payload_json: json!({
                    "recipe_id": recipe_id,
                    "role_preset": role_preset.as_str(),
                    "trace_id": run_id.to_string(),
                    "requested_capability_count": requested_capabilities.as_array().map_or(0, |v| v.len()),
                    "granted_capability_count": granted_capabilities.as_array().map_or(0, |v| v.len()),
                }),
            },
        )
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed appending run.created audit event: {err}"))
        })?;
    }

    let run = get_run_status_dual(db_pool, &tenant_id, run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading created run: {err}")))?
        .ok_or_else(|| ApiError::internal("created run could not be reloaded"))?;

    Ok((status_code, Json(run_to_response(run))))
}

fn compute_run_semantic_dedupe_key(
    tenant_id: &str,
    agent_id: Uuid,
    triggered_by_user_id: Option<Uuid>,
    recipe_id: &str,
    input: &Value,
    requested_capabilities: &Value,
    role_preset: RolePreset,
) -> ApiResult<String> {
    let dedupe_payload = json!({
        "tenant_id": tenant_id,
        "agent_id": agent_id.to_string(),
        "triggered_by_user_id": triggered_by_user_id.map(|value| value.to_string()),
        "recipe_id": recipe_id,
        "role_preset": role_preset.as_str(),
        "input": canonicalize_json_for_semantic_dedupe(input),
        "requested_capabilities": canonicalize_json_for_semantic_dedupe(requested_capabilities),
    });
    let dedupe_bytes = serde_json::to_vec(&dedupe_payload)
        .map_err(|err| {
            ApiError::internal(format!("failed serializing run semantic dedupe payload: {err}"))
        })?;
    Ok(digest_sha256_hex(&dedupe_bytes))
}

fn canonicalize_json_for_semantic_dedupe(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut fields: Vec<(String, Value)> = map
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_json_for_semantic_dedupe(value)))
                .collect();
            fields.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut canonical = Map::new();
            for (key, value) in fields {
                canonical.insert(key, value);
            }
            Value::Object(canonical)
        }
        Value::Array(values) => {
            Value::Array(values.iter().map(canonicalize_json_for_semantic_dedupe).collect())
        }
        _ => value.clone(),
    }
}

async fn ensure_tenant_trigger_capacity(state: &AppState, tenant_id: &str) -> ApiResult<()> {
    if let Some(limit) = state.tenant_max_triggers {
        let trigger_count = count_tenant_triggers(&state.pool, tenant_id)
            .await
            .map_err(|err| ApiError::internal(format!("failed counting tenant triggers: {err}")))?;
        if trigger_count >= limit {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "TENANT_TRIGGER_LIMITED",
                message: format!(
                    "tenant is at max trigger capacity (limit={}, triggers={})",
                    limit, trigger_count
                ),
            });
        }
    }

    Ok(())
}

fn inject_trace_id_into_input(input: Value, trace_id: &str) -> Value {
    match input {
        Value::Object(mut map) => {
            map.insert("_trace".to_string(), Value::String(trace_id.to_string()));
            Value::Object(map)
        }
        _ => json!({
            "input": input,
            "_trace": trace_id,
        }),
    }
}

fn merge_json_objects_for_api(primary: Value, overlay: Value) -> Value {
    let mut merged = match primary {
        Value::Object(map) => map,
        other => {
            return json!({
                "input": other,
                "_trigger": overlay,
            })
        }
    };

    if let Value::Object(overlay_map) = overlay {
        for (key, value) in overlay_map {
            merged.insert(key, value);
        }
    }
    Value::Object(merged)
}

async fn ensure_tenant_memory_capacity(state: &AppState, tenant_id: &str) -> ApiResult<()> {
    if let Some(limit) = state.tenant_max_memory_records {
        let memory_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM memory_records
            WHERE tenant_id = $1
              AND compacted_at IS NULL
              AND (expires_at IS NULL OR expires_at > now())
            "#,
        )
        .bind(tenant_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed counting tenant memory records: {err}"))
        })?;
        if memory_count >= limit {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "TENANT_MEMORY_LIMITED",
                message: format!(
                    "tenant is at max memory record capacity (limit={}, active_records={})",
                    limit, memory_count
                ),
            });
        }
    }
    Ok(())
}

async fn get_run_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;

    let Some(run) = get_run_status_dual(&state.db_pool, &tenant_id, run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed fetching run: {err}")))?
    else {
        return Err(ApiError::not_found("run not found"));
    };

    Ok((StatusCode::OK, Json(run_to_response(run))))
}

async fn get_run_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;

    let Some(run) = get_run_status_dual(&state.db_pool, &tenant_id, run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed fetching run: {err}")))?
    else {
        return Err(ApiError::not_found("run not found"));
    };

    Ok((StatusCode::OK, Json(run_to_response(run))))
}

fn sqlite_pool_from_db_pool(db_pool: &DbPool) -> ApiResult<&SqlitePool> {
    match db_pool {
        DbPool::Sqlite(pool) => Ok(pool),
        DbPool::Postgres(_) => Err(ApiError::internal(
            "sqlite profile received non-sqlite database pool",
        )),
    }
}

async fn ensure_tenant_trigger_capacity_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
) -> ApiResult<()> {
    if let Some(limit) = parse_positive_i64_env("API_TENANT_MAX_TRIGGERS") {
        let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
        let trigger_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM triggers
            WHERE tenant_id = ?1
            "#,
        )
        .bind(tenant_id)
        .fetch_one(sqlite)
        .await
        .map_err(|err| ApiError::internal(format!("failed counting tenant triggers: {err}")))?;
        if trigger_count >= limit {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "TENANT_TRIGGER_LIMITED",
                message: format!(
                    "tenant is at max trigger capacity (limit={}, triggers={})",
                    limit, trigger_count
                ),
            });
        }
    }

    Ok(())
}

async fn ensure_tenant_memory_capacity_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
) -> ApiResult<()> {
    if let Some(limit) = parse_positive_i64_env("API_TENANT_MAX_MEMORY_RECORDS") {
        let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
        let memory_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM memory_records
            WHERE tenant_id = ?1
              AND compacted_at IS NULL
              AND (expires_at IS NULL OR datetime(expires_at) > datetime('now'))
            "#,
        )
        .bind(tenant_id)
        .fetch_one(sqlite)
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed counting tenant memory records: {err}"))
        })?;
        if memory_count >= limit {
            return Err(ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "TENANT_MEMORY_LIMITED",
                message: format!(
                    "tenant is at max memory record capacity (limit={}, active_records={})",
                    limit, memory_count
                ),
            });
        }
    }

    Ok(())
}

fn user_id_from_headers_sqlite(
    state: &SqliteAppState,
    headers: &HeaderMap,
) -> ApiResult<Option<Uuid>> {
    enforce_trusted_proxy_auth_sqlite(state, headers)?;
    let Some(raw) = headers.get(USER_ID_HEADER) else {
        return Ok(None);
    };
    let value = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("x-user-id header is not valid UTF-8"))?;
    let parsed = Uuid::parse_str(value.trim())
        .map_err(|_| ApiError::bad_request("x-user-id header must be a valid UUID"))?;
    Ok(Some(parsed))
}

async fn append_trigger_audit_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
    trigger_id: Uuid,
    role_preset: RolePreset,
    event_type: &str,
    payload_json: Value,
) -> ApiResult<()> {
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    sqlx::query(
        r#"
        INSERT INTO trigger_audit_events (
            id, trigger_id, tenant_id, actor, event_type, payload_json
        )
        VALUES (?1, ?2, ?3, 'api', ?4, ?5)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(trigger_id.to_string())
    .bind(tenant_id)
    .bind(event_type)
    .bind(
        json!({
            "role_preset": role_preset.as_str(),
            "details": payload_json,
        })
        .to_string(),
    )
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed appending trigger audit event: {err}")))?;
    Ok(())
}

async fn get_trigger_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
    trigger_id: Uuid,
) -> ApiResult<Option<agent_core::TriggerRecord>> {
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let row = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               agent_id,
               triggered_by_user_id,
               recipe_id,
               status,
               trigger_type,
               interval_seconds,
               cron_expression,
               schedule_timezone,
               misfire_policy,
               max_attempts,
               max_inflight_runs,
               jitter_seconds,
               consecutive_failures,
               dead_lettered_at,
               dead_letter_reason,
               webhook_secret_ref,
               input_json,
               requested_capabilities,
               granted_capabilities,
               next_fire_at,
               last_fired_at,
               created_at,
               updated_at
        FROM triggers
        WHERE tenant_id = ?1
          AND id = ?2
        "#,
    )
    .bind(tenant_id)
    .bind(trigger_id.to_string())
    .fetch_optional(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed loading trigger: {err}")))?;

    row.map(trigger_record_from_sqlite_row).transpose()
}

fn trigger_record_from_sqlite_row(
    row: sqlx::sqlite::SqliteRow,
) -> ApiResult<agent_core::TriggerRecord> {
    Ok(agent_core::TriggerRecord {
        id: parse_sqlite_uuid_required(&row, "id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_sqlite_uuid_required(&row, "agent_id")?,
        triggered_by_user_id: parse_sqlite_uuid_optional(&row, "triggered_by_user_id")?,
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        trigger_type: row.get("trigger_type"),
        interval_seconds: row.get("interval_seconds"),
        cron_expression: row.get("cron_expression"),
        schedule_timezone: row.get("schedule_timezone"),
        misfire_policy: row.get("misfire_policy"),
        max_attempts: row.get("max_attempts"),
        max_inflight_runs: row.get("max_inflight_runs"),
        jitter_seconds: row.get("jitter_seconds"),
        consecutive_failures: row.get("consecutive_failures"),
        dead_lettered_at: parse_sqlite_datetime_optional(&row, "dead_lettered_at")?,
        dead_letter_reason: row.get("dead_letter_reason"),
        webhook_secret_ref: row.get("webhook_secret_ref"),
        input_json: parse_sqlite_json_required(&row, "input_json")?,
        requested_capabilities: parse_sqlite_json_required(&row, "requested_capabilities")?,
        granted_capabilities: parse_sqlite_json_required(&row, "granted_capabilities")?,
        next_fire_at: parse_sqlite_datetime_required(&row, "next_fire_at")?,
        last_fired_at: parse_sqlite_datetime_optional(&row, "last_fired_at")?,
        created_at: parse_sqlite_datetime_required(&row, "created_at")?,
        updated_at: parse_sqlite_datetime_required(&row, "updated_at")?,
    })
}

fn parse_sqlite_uuid_required(row: &sqlx::sqlite::SqliteRow, column: &str) -> ApiResult<Uuid> {
    let raw: String = row.get(column);
    Uuid::parse_str(raw.as_str()).map_err(|err| {
        ApiError::internal(format!(
            "invalid uuid in column `{column}`: {err} (value={raw})"
        ))
    })
}

fn parse_sqlite_uuid_optional(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> ApiResult<Option<Uuid>> {
    let raw: Option<String> = row.get(column);
    raw.map(|value| {
        Uuid::parse_str(value.as_str()).map_err(|err| {
            ApiError::internal(format!(
                "invalid uuid in column `{column}`: {err} (value={value})"
            ))
        })
    })
    .transpose()
}

fn parse_sqlite_json_required(row: &sqlx::sqlite::SqliteRow, column: &str) -> ApiResult<Value> {
    let raw: String = row.get(column);
    serde_json::from_str(raw.as_str()).map_err(|err| {
        ApiError::internal(format!(
            "invalid json in column `{column}`: {err} (value={raw})"
        ))
    })
}

fn parse_sqlite_json_optional(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> ApiResult<Option<Value>> {
    let raw: Option<String> = row.get(column);
    raw.map(|value| {
        serde_json::from_str(value.as_str()).map_err(|err| {
            ApiError::internal(format!(
                "invalid json in column `{column}`: {err} (value={value})"
            ))
        })
    })
    .transpose()
}

fn parse_sqlite_datetime_required(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> ApiResult<OffsetDateTime> {
    let raw: String = row.get(column);
    parse_sqlite_datetime_str(raw.as_str()).map_err(|err| {
        ApiError::internal(format!(
            "invalid datetime in column `{column}`: {err} (value={raw})"
        ))
    })
}

fn parse_sqlite_datetime_optional(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> ApiResult<Option<OffsetDateTime>> {
    let raw: Option<String> = row.get(column);
    raw.map(|value| {
        parse_sqlite_datetime_str(value.as_str()).map_err(|err| {
            ApiError::internal(format!(
                "invalid datetime in column `{column}`: {err} (value={value})"
            ))
        })
    })
    .transpose()
}

fn parse_sqlite_datetime_str(raw: &str) -> Result<OffsetDateTime, time::error::Parse> {
    if let Ok(parsed) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Ok(parsed);
    }

    const SQLITE_FORMAT_NO_SUBSECOND: &[time::format_description::FormatItem<'_>] =
        time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    if let Ok(primitive) = PrimitiveDateTime::parse(raw, SQLITE_FORMAT_NO_SUBSECOND) {
        return Ok(primitive.assume_utc());
    }

    const SQLITE_FORMAT_WITH_SUBSECOND: &[time::format_description::FormatItem<'_>] = time::macros::format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond]"
    );
    PrimitiveDateTime::parse(raw, SQLITE_FORMAT_WITH_SUBSECOND).map(|value| value.assume_utc())
}

async fn create_trigger_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<CreateTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    if req.interval_seconds <= 0 {
        return Err(ApiError::bad_request(
            "interval_seconds must be greater than zero",
        ));
    }
    if req.interval_seconds > 31_536_000 {
        return Err(ApiError::bad_request(
            "interval_seconds exceeds maximum of 31536000",
        ));
    }
    if req.max_inflight_runs <= 0 || req.max_inflight_runs > 1000 {
        return Err(ApiError::bad_request(
            "max_inflight_runs must be between 1 and 1000",
        ));
    }
    if req.jitter_seconds < 0 || req.jitter_seconds > 3600 {
        return Err(ApiError::bad_request(
            "jitter_seconds must be between 0 and 3600",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let effective_triggered_by_user_id =
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?;
    let requested_capabilities = req.requested_capabilities;
    let granted_capabilities =
        resolve_granted_capabilities(req.recipe_id.as_str(), role_preset, &requested_capabilities)?;
    ensure_tenant_trigger_capacity_sqlite(&state, tenant_id.as_str()).await?;

    let trigger_id = Uuid::new_v4();
    let now = OffsetDateTime::now_utc();
    let next_fire_at = now + time::Duration::seconds(req.interval_seconds);
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    sqlx::query(
        r#"
        INSERT INTO triggers (
            id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
            interval_seconds, schedule_timezone, misfire_policy, max_attempts, max_inflight_runs,
            jitter_seconds, input_json, requested_capabilities, granted_capabilities, next_fire_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', 'interval', ?6, 'UTC', 'fire_now', 3, ?7, ?8, ?9, ?10, ?11, ?12)
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(&tenant_id)
    .bind(req.agent_id.to_string())
    .bind(effective_triggered_by_user_id.map(|id| id.to_string()))
    .bind(&req.recipe_id)
    .bind(req.interval_seconds)
    .bind(req.max_inflight_runs)
    .bind(req.jitter_seconds)
    .bind(req.input.to_string())
    .bind(requested_capabilities.to_string())
    .bind(granted_capabilities.to_string())
    .bind(next_fire_at.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting trigger next_fire_at: {err}"))
    })?)
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed creating trigger: {err}")))?;

    let created = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id)
        .await?
        .ok_or_else(|| ApiError::internal("created trigger could not be reloaded"))?;

    append_trigger_audit_sqlite(
        &state,
        &tenant_id,
        created.id,
        role_preset,
        "trigger.created",
        json!({
            "trigger_type": created.trigger_type,
            "interval_seconds": created.interval_seconds,
            "max_inflight_runs": created.max_inflight_runs,
            "jitter_seconds": created.jitter_seconds,
        }),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(trigger_to_response(created))))
}

async fn create_cron_trigger_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<CreateCronTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    if req.cron_expression.trim().is_empty() {
        return Err(ApiError::bad_request("cron_expression must not be empty"));
    }
    if req.schedule_timezone.trim().is_empty() {
        return Err(ApiError::bad_request("schedule_timezone must not be empty"));
    }
    if req.max_attempts <= 0 || req.max_attempts > 20 {
        return Err(ApiError::bad_request(
            "max_attempts must be between 1 and 20",
        ));
    }
    if req.max_inflight_runs <= 0 || req.max_inflight_runs > 1000 {
        return Err(ApiError::bad_request(
            "max_inflight_runs must be between 1 and 1000",
        ));
    }
    if req.jitter_seconds < 0 || req.jitter_seconds > 3600 {
        return Err(ApiError::bad_request(
            "jitter_seconds must be between 0 and 3600",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let effective_triggered_by_user_id =
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?;
    let requested_capabilities = req.requested_capabilities;
    let granted_capabilities =
        resolve_granted_capabilities(req.recipe_id.as_str(), role_preset, &requested_capabilities)?;
    ensure_tenant_trigger_capacity_sqlite(&state, tenant_id.as_str()).await?;

    let trigger_id = Uuid::new_v4();
    let now = OffsetDateTime::now_utc();
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    sqlx::query(
        r#"
        INSERT INTO triggers (
            id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
            cron_expression, schedule_timezone, misfire_policy, max_attempts, max_inflight_runs,
            jitter_seconds, input_json, requested_capabilities, granted_capabilities, next_fire_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', 'cron', ?6, ?7, 'fire_now', ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(&tenant_id)
    .bind(req.agent_id.to_string())
    .bind(effective_triggered_by_user_id.map(|id| id.to_string()))
    .bind(&req.recipe_id)
    .bind(req.cron_expression.trim())
    .bind(req.schedule_timezone.trim())
    .bind(req.max_attempts)
    .bind(req.max_inflight_runs)
    .bind(req.jitter_seconds)
    .bind(req.input.to_string())
    .bind(requested_capabilities.to_string())
    .bind(granted_capabilities.to_string())
    .bind(now.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting trigger next_fire_at: {err}"))
    })?)
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed creating cron trigger: {err}")))?;

    let created = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id)
        .await?
        .ok_or_else(|| ApiError::internal("created trigger could not be reloaded"))?;

    append_trigger_audit_sqlite(
        &state,
        &tenant_id,
        created.id,
        role_preset,
        "trigger.created",
        json!({
            "trigger_type": created.trigger_type,
            "cron_expression": created.cron_expression,
            "schedule_timezone": created.schedule_timezone,
            "max_inflight_runs": created.max_inflight_runs,
            "jitter_seconds": created.jitter_seconds,
        }),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(trigger_to_response(created))))
}

async fn create_webhook_trigger_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<CreateWebhookTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    if req.max_attempts <= 0 || req.max_attempts > 20 {
        return Err(ApiError::bad_request(
            "max_attempts must be between 1 and 20",
        ));
    }
    if req.max_inflight_runs <= 0 || req.max_inflight_runs > 1000 {
        return Err(ApiError::bad_request(
            "max_inflight_runs must be between 1 and 1000",
        ));
    }
    if req.jitter_seconds < 0 || req.jitter_seconds > 3600 {
        return Err(ApiError::bad_request(
            "jitter_seconds must be between 0 and 3600",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let effective_triggered_by_user_id =
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?;
    let requested_capabilities = req.requested_capabilities;
    let granted_capabilities =
        resolve_granted_capabilities(req.recipe_id.as_str(), role_preset, &requested_capabilities)?;
    ensure_tenant_trigger_capacity_sqlite(&state, tenant_id.as_str()).await?;

    let trigger_id = Uuid::new_v4();
    let now = OffsetDateTime::now_utc();
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    sqlx::query(
        r#"
        INSERT INTO triggers (
            id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
            schedule_timezone, misfire_policy, max_attempts, max_inflight_runs, jitter_seconds,
            webhook_secret_ref, input_json, requested_capabilities, granted_capabilities, next_fire_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', 'webhook', 'UTC', 'fire_now', ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(&tenant_id)
    .bind(req.agent_id.to_string())
    .bind(effective_triggered_by_user_id.map(|id| id.to_string()))
    .bind(&req.recipe_id)
    .bind(req.max_attempts)
    .bind(req.max_inflight_runs)
    .bind(req.jitter_seconds)
    .bind(req.webhook_secret_ref.as_deref().map(str::trim).filter(|v| !v.is_empty()))
    .bind(req.input.to_string())
    .bind(requested_capabilities.to_string())
    .bind(granted_capabilities.to_string())
    .bind(now.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting trigger next_fire_at: {err}"))
    })?)
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed creating webhook trigger: {err}")))?;

    let created = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id)
        .await?
        .ok_or_else(|| ApiError::internal("created trigger could not be reloaded"))?;

    append_trigger_audit_sqlite(
        &state,
        &tenant_id,
        created.id,
        role_preset,
        "trigger.created",
        json!({
            "trigger_type": created.trigger_type,
            "max_attempts": created.max_attempts,
            "max_inflight_runs": created.max_inflight_runs,
            "jitter_seconds": created.jitter_seconds,
            "webhook_secret_configured": created.webhook_secret_ref.is_some(),
        }),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(trigger_to_response(created))))
}

async fn update_trigger_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
    Json(req): Json<UpdateTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;

    if let Some(seconds) = req.interval_seconds {
        if seconds <= 0 || seconds > 31_536_000 {
            return Err(ApiError::bad_request(
                "interval_seconds must be between 1 and 31536000",
            ));
        }
    }
    if let Some(ref expression) = req.cron_expression {
        if expression.trim().is_empty() {
            return Err(ApiError::bad_request("cron_expression must not be empty"));
        }
    }
    if let Some(ref timezone) = req.schedule_timezone {
        if timezone.trim().is_empty() {
            return Err(ApiError::bad_request("schedule_timezone must not be empty"));
        }
    }
    if let Some(ref policy) = req.misfire_policy {
        if policy != "fire_now" && policy != "skip" {
            return Err(ApiError::bad_request(
                "misfire_policy must be one of: fire_now, skip",
            ));
        }
    }
    if let Some(attempts) = req.max_attempts {
        if attempts <= 0 || attempts > 20 {
            return Err(ApiError::bad_request(
                "max_attempts must be between 1 and 20",
            ));
        }
    }
    if let Some(max_inflight_runs) = req.max_inflight_runs {
        if max_inflight_runs <= 0 || max_inflight_runs > 1000 {
            return Err(ApiError::bad_request(
                "max_inflight_runs must be between 1 and 1000",
            ));
        }
    }
    if let Some(jitter_seconds) = req.jitter_seconds {
        if !(0..=3600).contains(&jitter_seconds) {
            return Err(ApiError::bad_request(
                "jitter_seconds must be between 0 and 3600",
            ));
        }
    }

    let Some(existing) = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id).await? else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, existing.triggered_by_user_id)?;

    match existing.trigger_type.as_str() {
        "interval" => {
            if req.cron_expression.is_some() || req.schedule_timezone.is_some() {
                return Err(ApiError::bad_request(
                    "interval trigger does not support cron schedule fields",
                ));
            }
        }
        "cron" => {
            if req.interval_seconds.is_some() {
                return Err(ApiError::bad_request(
                    "cron trigger does not support interval_seconds",
                ));
            }
        }
        "webhook" => {
            if req.interval_seconds.is_some()
                || req.cron_expression.is_some()
                || req.schedule_timezone.is_some()
            {
                return Err(ApiError::bad_request(
                    "webhook trigger does not support schedule fields",
                ));
            }
        }
        _ => {}
    }

    let interval_seconds = req.interval_seconds.or(existing.interval_seconds);
    let cron_expression = req
        .cron_expression
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or(existing.cron_expression.clone());
    let schedule_timezone = req
        .schedule_timezone
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or(existing.schedule_timezone.clone());
    let misfire_policy = req
        .misfire_policy
        .clone()
        .unwrap_or(existing.misfire_policy.clone());
    let max_attempts = req.max_attempts.unwrap_or(existing.max_attempts);
    let max_inflight_runs = req.max_inflight_runs.unwrap_or(existing.max_inflight_runs);
    let jitter_seconds = req.jitter_seconds.unwrap_or(existing.jitter_seconds);
    let webhook_secret_ref = req
        .webhook_secret_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or(existing.webhook_secret_ref.clone());

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    sqlx::query(
        r#"
        UPDATE triggers
        SET interval_seconds = ?3,
            cron_expression = ?4,
            schedule_timezone = ?5,
            misfire_policy = ?6,
            max_attempts = ?7,
            max_inflight_runs = ?8,
            jitter_seconds = ?9,
            webhook_secret_ref = ?10,
            updated_at = CURRENT_TIMESTAMP
        WHERE tenant_id = ?1
          AND id = ?2
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(trigger_id.to_string())
    .bind(interval_seconds)
    .bind(cron_expression)
    .bind(schedule_timezone)
    .bind(misfire_policy)
    .bind(max_attempts)
    .bind(max_inflight_runs)
    .bind(jitter_seconds)
    .bind(webhook_secret_ref)
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed updating trigger: {err}")))?;

    let updated = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id)
        .await?
        .ok_or_else(|| ApiError::not_found("trigger not found"))?;

    append_trigger_audit_sqlite(
        &state,
        &tenant_id,
        trigger_id,
        role_preset,
        "trigger.updated",
        json!({
            "trigger_type": updated.trigger_type,
            "interval_seconds": updated.interval_seconds,
            "cron_expression": updated.cron_expression,
            "schedule_timezone": updated.schedule_timezone,
            "misfire_policy": updated.misfire_policy,
            "max_attempts": updated.max_attempts,
            "max_inflight_runs": updated.max_inflight_runs,
            "jitter_seconds": updated.jitter_seconds,
            "webhook_secret_configured": updated.webhook_secret_ref.is_some(),
        }),
    )
    .await?;

    Ok((StatusCode::OK, Json(trigger_to_response(updated))))
}

async fn set_trigger_status_sqlite_handler(
    state: SqliteAppState,
    headers: HeaderMap,
    trigger_id: Uuid,
    status: &str,
    audit_event_type: &str,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;

    let Some(existing) = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id).await? else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, existing.triggered_by_user_id)?;

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let result = sqlx::query(
        r#"
        UPDATE triggers
        SET status = ?3,
            updated_at = CURRENT_TIMESTAMP
        WHERE tenant_id = ?1
          AND id = ?2
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(trigger_id.to_string())
    .bind(status)
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed updating trigger status: {err}")))?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("trigger not found"));
    }

    let updated = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id)
        .await?
        .ok_or_else(|| ApiError::not_found("trigger not found"))?;

    append_trigger_audit_sqlite(
        &state,
        &tenant_id,
        trigger_id,
        role_preset,
        audit_event_type,
        json!({
            "status": updated.status,
            "trigger_type": updated.trigger_type,
        }),
    )
    .await?;

    Ok((StatusCode::OK, Json(trigger_to_response(updated))))
}

async fn enable_trigger_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    set_trigger_status_sqlite_handler(state, headers, trigger_id, "enabled", "trigger.enabled")
        .await
}

async fn disable_trigger_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    set_trigger_status_sqlite_handler(state, headers, trigger_id, "disabled", "trigger.disabled")
        .await
}

async fn ingest_trigger_event_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
    Json(req): Json<TriggerEventRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    if req.event_id.trim().is_empty() {
        return Err(ApiError::bad_request("event_id must not be empty"));
    }
    if req.payload.as_object().is_none() {
        return Err(ApiError::bad_request(
            "trigger event payload must be a JSON object",
        ));
    }

    let Some(trigger) = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id).await? else {
        return Err(ApiError::not_found("trigger not found"));
    };
    if trigger.trigger_type != "webhook" {
        return Err(ApiError::bad_request(
            "trigger does not accept webhook events",
        ));
    }
    if trigger.status != "enabled" || trigger.dead_lettered_at.is_some() {
        return Err(ApiError::conflict("trigger is not enabled"));
    }
    if let Some(reference) = trigger.webhook_secret_ref {
        let provided = headers
            .get(TRIGGER_SECRET_HEADER)
            .ok_or_else(|| ApiError::bad_request("missing x-trigger-secret header"))?
            .to_str()
            .map_err(|_| ApiError::bad_request("x-trigger-secret must be valid UTF-8"))?;
        let expected = resolve_secret_value(None, Some(reference), shared_secret_resolver())
            .map_err(|err| ApiError::internal(format!("failed resolving trigger secret: {err}")))?
            .ok_or_else(|| ApiError::internal("trigger secret reference resolved to empty"))?;
        if provided != expected {
            return Err(ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: "UNAUTHORIZED",
                message: "trigger secret validation failed".to_string(),
            });
        }
    }

    let semantic_dedupe_key =
        compute_trigger_event_semantic_dedupe_key(tenant_id.as_str(), &trigger_id, &req.payload);

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let result = sqlx::query(
        r#"
        INSERT INTO trigger_events (
            id, trigger_id, tenant_id, event_id, semantic_dedupe_key, payload_json, status, attempts, next_attempt_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0, CURRENT_TIMESTAMP)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(trigger_id.to_string())
    .bind(tenant_id.as_str())
    .bind(req.event_id.as_str())
    .bind(semantic_dedupe_key)
    .bind(req.payload.to_string())
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed enqueueing trigger event: {err}")))?;

    let status = if result.rows_affected() == 0 {
        "duplicate"
    } else {
        "queued"
    };

    Ok((
        StatusCode::ACCEPTED,
        Json(TriggerEventIngestResponse {
            trigger_id,
            event_id: req.event_id,
            status,
        }),
    ))
}

async fn replay_trigger_event_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path((trigger_id, event_id)): Path<(Uuid, String)>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;

    let Some(trigger) = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id).await? else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, trigger.triggered_by_user_id)?;

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let result = sqlx::query(
        r#"
        UPDATE trigger_events
        SET status = 'pending',
            attempts = 0,
            next_attempt_at = CURRENT_TIMESTAMP,
            last_error_json = NULL,
            processed_at = NULL,
            dead_lettered_at = NULL
        WHERE trigger_id = ?1
          AND event_id = ?2
          AND status = 'dead_lettered'
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(event_id.as_str())
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed replaying trigger event: {err}")))?;

    if result.rows_affected() == 1 {
        append_trigger_audit_sqlite(
            &state,
            &tenant_id,
            trigger_id,
            role_preset,
            "trigger.event.replayed",
            json!({
                "event_id": event_id,
            }),
        )
        .await?;
        return Ok((
            StatusCode::ACCEPTED,
            Json(TriggerEventIngestResponse {
                trigger_id,
                event_id,
                status: "queued_for_replay",
            }),
        ));
    }

    let status: Option<String> = sqlx::query_scalar(
        r#"
        SELECT status
        FROM trigger_events
        WHERE trigger_id = ?1
          AND event_id = ?2
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(event_id.as_str())
    .fetch_optional(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed loading trigger event status: {err}")))?;

    match status {
        Some(status) => Err(ApiError {
            status: StatusCode::CONFLICT,
            code: "TRIGGER_EVENT_NOT_REPLAYABLE",
            message: format!("trigger event cannot be replayed from status `{status}`"),
        }),
        None => Err(ApiError::not_found("trigger event not found")),
    }
}

async fn fire_trigger_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
    Json(req): Json<FireTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;

    let idempotency_key = req.idempotency_key.trim();
    if idempotency_key.is_empty() {
        return Err(ApiError::bad_request("idempotency_key must not be empty"));
    }
    if idempotency_key.len() > 128 {
        return Err(ApiError::bad_request(
            "idempotency_key exceeds maximum length of 128",
        ));
    }

    let Some(trigger) = get_trigger_sqlite(&state, tenant_id.as_str(), trigger_id).await? else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, trigger.triggered_by_user_id)?;
    if trigger.status != "enabled" || trigger.dead_lettered_at.is_some() {
        return Err(ApiError::conflict("trigger is not enabled"));
    }

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let mut tx = sqlite
        .begin()
        .await
        .map_err(|err| ApiError::internal(format!("failed opening sqlite tx: {err}")))?;

    let existing_run_id: Option<String> = sqlx::query_scalar(
        r#"
        SELECT run_id
        FROM trigger_runs
        WHERE trigger_id = ?1
          AND dedupe_key = ?2
        LIMIT 1
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(idempotency_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| ApiError::internal(format!("failed checking trigger dedupe key: {err}")))?;

    if let Some(raw_run_id) = existing_run_id {
        tx.commit()
            .await
            .map_err(|err| ApiError::internal(format!("failed committing sqlite tx: {err}")))?;
        let run_id = Uuid::parse_str(raw_run_id.as_str()).ok();
        return Ok((
            StatusCode::OK,
            Json(TriggerFireResponse {
                trigger_id,
                run_id,
                idempotency_key: idempotency_key.to_string(),
                status: "duplicate",
            }),
        ));
    }

    let run_id = Uuid::new_v4();
    let trigger_envelope = json!({
        "_trigger": {
            "type": "manual",
            "trigger_id": trigger_id,
            "idempotency_key": idempotency_key,
        },
        "manual_payload": req.payload,
    });
    let run_input = merge_json_objects_for_api(trigger.input_json, trigger_envelope);
    sqlx::query(
        r#"
        INSERT INTO runs (
            id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
            input_json, requested_capabilities, granted_capabilities
        )
        VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8)
        "#,
    )
    .bind(run_id.to_string())
    .bind(tenant_id.as_str())
    .bind(trigger.agent_id.to_string())
    .bind(trigger.triggered_by_user_id.map(|id| id.to_string()))
    .bind(trigger.recipe_id.as_str())
    .bind(run_input.to_string())
    .bind(trigger.requested_capabilities.to_string())
    .bind(trigger.granted_capabilities.to_string())
    .execute(&mut *tx)
    .await
    .map_err(|err| ApiError::internal(format!("failed creating triggered run: {err}")))?;

    sqlx::query(
        r#"
        INSERT INTO trigger_runs (
            id, trigger_id, run_id, scheduled_for, status, dedupe_key
        )
        VALUES (?1, ?2, ?3, ?4, 'created', ?5)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(trigger_id.to_string())
    .bind(run_id.to_string())
    .bind(OffsetDateTime::now_utc().format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting trigger scheduled_for: {err}"))
    })?)
    .bind(idempotency_key)
    .execute(&mut *tx)
    .await
    .map_err(|err| ApiError::internal(format!("failed writing trigger run ledger: {err}")))?;

    sqlx::query(
        r#"
        UPDATE triggers
        SET last_fired_at = CURRENT_TIMESTAMP,
            consecutive_failures = 0,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
    )
    .bind(trigger_id.to_string())
    .execute(&mut *tx)
    .await
    .map_err(|err| ApiError::internal(format!("failed updating trigger fire timestamp: {err}")))?;

    tx.commit()
        .await
        .map_err(|err| ApiError::internal(format!("failed committing sqlite tx: {err}")))?;

    append_trigger_audit_sqlite(
        &state,
        &tenant_id,
        trigger_id,
        role_preset,
        "trigger.fired",
        json!({
            "run_id": run_id,
            "idempotency_key": idempotency_key,
        }),
    )
    .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(TriggerFireResponse {
            trigger_id,
            run_id: Some(run_id),
            idempotency_key: idempotency_key.to_string(),
            status: "created",
        }),
    ))
}

fn memory_record_from_sqlite_row(
    row: &sqlx::sqlite::SqliteRow,
) -> ApiResult<agent_core::MemoryRecord> {
    let redaction_applied_raw: i64 = row.get("redaction_applied");
    Ok(agent_core::MemoryRecord {
        id: parse_sqlite_uuid_required(row, "id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_sqlite_uuid_required(row, "agent_id")?,
        run_id: parse_sqlite_uuid_optional(row, "run_id")?,
        step_id: parse_sqlite_uuid_optional(row, "step_id")?,
        memory_kind: row.get("memory_kind"),
        scope: row.get("scope"),
        content_json: parse_sqlite_json_required(row, "content_json")?,
        summary_text: row.get("summary_text"),
        source: row.get("source"),
        redaction_applied: redaction_applied_raw != 0,
        expires_at: parse_sqlite_datetime_optional(row, "expires_at")?,
        compacted_at: parse_sqlite_datetime_optional(row, "compacted_at")?,
        created_at: parse_sqlite_datetime_required(row, "created_at")?,
        updated_at: parse_sqlite_datetime_required(row, "updated_at")?,
    })
}

async fn create_memory_record_sqlite(
    state: &SqliteAppState,
    new_record: &NewMemoryRecord,
) -> ApiResult<agent_core::MemoryRecord> {
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let row = sqlx::query(
        r#"
        INSERT INTO memory_records (
            id,
            tenant_id,
            agent_id,
            run_id,
            step_id,
            memory_kind,
            scope,
            content_json,
            summary_text,
            source,
            redaction_applied,
            expires_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        RETURNING id,
                  tenant_id,
                  agent_id,
                  run_id,
                  step_id,
                  memory_kind,
                  scope,
                  content_json,
                  summary_text,
                  source,
                  redaction_applied,
                  expires_at,
                  compacted_at,
                  created_at,
                  updated_at
        "#,
    )
    .bind(new_record.id.to_string())
    .bind(&new_record.tenant_id)
    .bind(new_record.agent_id.to_string())
    .bind(new_record.run_id.map(|value| value.to_string()))
    .bind(new_record.step_id.map(|value| value.to_string()))
    .bind(&new_record.memory_kind)
    .bind(&new_record.scope)
    .bind(new_record.content_json.to_string())
    .bind(&new_record.summary_text)
    .bind(&new_record.source)
    .bind(if new_record.redaction_applied {
        1_i64
    } else {
        0_i64
    })
    .bind(
        new_record
            .expires_at
            .map(|value| value.format(&Rfc3339))
            .transpose()
            .map_err(|err| {
                ApiError::internal(format!("failed formatting memory expires_at: {err}"))
            })?,
    )
    .fetch_one(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed creating memory record: {err}")))?;

    memory_record_from_sqlite_row(&row)
}

async fn validate_memory_run_and_step_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
    run_id: Option<Uuid>,
    step_id: Option<Uuid>,
) -> ApiResult<()> {
    if let Some(run_id) = run_id {
        let exists = get_run_status_dual(&state.db_pool, tenant_id, run_id)
            .await
            .map_err(|err| ApiError::internal(format!("failed validating run_id: {err}")))?
            .is_some();
        if !exists {
            return Err(ApiError::bad_request("run_id is not found for this tenant"));
        }
    }

    if let Some(step_id) = step_id {
        let Some(run_id) = run_id else {
            return Err(ApiError::bad_request(
                "step_id requires run_id for tenant validation",
            ));
        };

        let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
        let step_exists: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM steps
            WHERE id = ?1
              AND run_id = ?2
              AND tenant_id = ?3
            "#,
        )
        .bind(step_id.to_string())
        .bind(run_id.to_string())
        .bind(tenant_id)
        .fetch_one(sqlite)
        .await
        .map_err(|err| ApiError::internal(format!("failed validating step_id: {err}")))?;
        if step_exists == 0 {
            return Err(ApiError::bad_request(
                "step_id is not found for this tenant/run",
            ));
        }
    }

    Ok(())
}

async fn list_tenant_memory_records_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
    agent_id: Option<Uuid>,
    memory_kind: Option<&str>,
    scope_prefix: Option<&str>,
    limit: i64,
) -> ApiResult<Vec<agent_core::MemoryRecord>> {
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               agent_id,
               run_id,
               step_id,
               memory_kind,
               scope,
               content_json,
               summary_text,
               source,
               redaction_applied,
               expires_at,
               compacted_at,
               created_at,
               updated_at
        FROM memory_records
        WHERE tenant_id = ?1
          AND compacted_at IS NULL
          AND (expires_at IS NULL OR datetime(expires_at) > datetime('now'))
          AND (?2 IS NULL OR agent_id = ?2)
          AND (?3 IS NULL OR memory_kind = ?3)
          AND (?4 IS NULL OR scope LIKE (?4 || '%'))
        ORDER BY datetime(created_at) DESC, id DESC
        LIMIT ?5
        "#,
    )
    .bind(tenant_id)
    .bind(agent_id.map(|value| value.to_string()))
    .bind(memory_kind)
    .bind(scope_prefix)
    .bind(limit.clamp(1, 1000))
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed listing memory records: {err}")))?;

    rows.iter()
        .map(memory_record_from_sqlite_row)
        .collect::<ApiResult<Vec<_>>>()
}

async fn list_tenant_handoff_memory_records_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
    to_agent_id: Option<Uuid>,
    from_agent_id: Option<Uuid>,
    limit: i64,
) -> ApiResult<Vec<agent_core::MemoryRecord>> {
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               agent_id,
               run_id,
               step_id,
               memory_kind,
               scope,
               content_json,
               summary_text,
               source,
               redaction_applied,
               expires_at,
               compacted_at,
               created_at,
               updated_at
        FROM memory_records
        WHERE tenant_id = ?1
          AND memory_kind = 'handoff'
          AND scope LIKE 'memory:handoff/%'
          AND compacted_at IS NULL
          AND (expires_at IS NULL OR datetime(expires_at) > datetime('now'))
          AND (?2 IS NULL OR agent_id = ?2)
          AND (?3 IS NULL OR json_extract(content_json, '$.from_agent_id') = ?3)
        ORDER BY datetime(created_at) DESC, id DESC
        LIMIT ?4
        "#,
    )
    .bind(tenant_id)
    .bind(to_agent_id.map(|value| value.to_string()))
    .bind(from_agent_id.map(|value| value.to_string()))
    .bind(limit.clamp(1, 1000))
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed listing handoff packets: {err}")))?;

    rows.iter()
        .map(memory_record_from_sqlite_row)
        .collect::<ApiResult<Vec<_>>>()
}

async fn create_memory_record_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<CreateMemoryRecordRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_memory_write_role(role_preset)?;
    ensure_tenant_memory_capacity_sqlite(&state, tenant_id.as_str()).await?;

    let Some(memory_kind) = normalize_memory_kind(req.memory_kind.as_str()) else {
        return Err(ApiError::bad_request(
            "memory_kind must be one of: session, semantic, procedural, handoff",
        ));
    };
    let scope = req.scope.trim();
    if scope.is_empty() {
        return Err(ApiError::bad_request("scope must not be empty"));
    }
    if !is_scope_allowed_for_capability("memory.write", scope) {
        return Err(ApiError::bad_request(
            "scope must be memory-scoped (prefix `memory:`)",
        ));
    }

    let source = req
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("api")
        .to_string();

    validate_memory_run_and_step_sqlite(&state, tenant_id.as_str(), req.run_id, req.step_id)
        .await?;

    let (redacted_content_json, redacted_summary_text, redaction_auto_applied) =
        redact_memory_content(&req.content_json, req.summary_text.as_deref());
    let redaction_applied = req.redaction_applied.unwrap_or(false) || redaction_auto_applied;

    let created = create_memory_record_sqlite(
        &state,
        &NewMemoryRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            agent_id: req.agent_id,
            run_id: req.run_id,
            step_id: req.step_id,
            memory_kind: memory_kind.to_string(),
            scope: scope.to_string(),
            content_json: redacted_content_json,
            summary_text: redacted_summary_text,
            source,
            redaction_applied,
            expires_at: req.expires_at,
        },
    )
    .await?;

    if let Some(run_id) = created.run_id {
        append_audit_event_dual(
            &state.db_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: created.step_id,
                tenant_id: tenant_id.clone(),
                agent_id: Some(created.agent_id),
                user_id: None,
                actor: "api".to_string(),
                event_type: "memory.recorded".to_string(),
                payload_json: json!({
                    "memory_id": created.id,
                    "memory_kind": created.memory_kind,
                    "scope": created.scope,
                    "redaction_applied": created.redaction_applied,
                    "expires_at": created.expires_at,
                }),
            },
        )
        .await
        .map_err(|err| ApiError::internal(format!("failed appending memory audit event: {err}")))?;
    }

    Ok((StatusCode::CREATED, Json(memory_to_response(created))))
}

async fn create_handoff_packet_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<CreateHandoffPacketRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_memory_write_role(role_preset)?;
    ensure_tenant_memory_capacity_sqlite(&state, tenant_id.as_str()).await?;

    let title = req.title.trim();
    if title.is_empty() {
        return Err(ApiError::bad_request("title must not be empty"));
    }

    let source = req
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("api")
        .to_string();

    validate_memory_run_and_step_sqlite(&state, tenant_id.as_str(), req.run_id, req.step_id)
        .await?;

    let from_agent_id = req.from_agent_id.unwrap_or(req.to_agent_id);
    let packet_id = Uuid::new_v4();
    let scope = format!("memory:handoff/{}/{}", req.to_agent_id, packet_id);
    let (redacted_payload_json, redacted_title, redaction_auto_applied) =
        redact_memory_content(&req.payload_json, Some(title));
    let redaction_applied = redaction_auto_applied;
    let title_value = redacted_title.unwrap_or_else(|| title.to_string());

    let created = create_memory_record_sqlite(
        &state,
        &NewMemoryRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            agent_id: req.to_agent_id,
            run_id: req.run_id,
            step_id: req.step_id,
            memory_kind: "handoff".to_string(),
            scope,
            content_json: json!({
                "packet_id": packet_id,
                "from_agent_id": from_agent_id,
                "to_agent_id": req.to_agent_id,
                "title": title_value,
                "payload_json": redacted_payload_json,
            }),
            summary_text: Some(title_value),
            source,
            redaction_applied,
            expires_at: req.expires_at,
        },
    )
    .await?;

    if let Some(run_id) = created.run_id {
        append_audit_event_dual(
            &state.db_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: created.step_id,
                tenant_id: tenant_id.clone(),
                agent_id: Some(created.agent_id),
                user_id: None,
                actor: "api".to_string(),
                event_type: "memory.handoff.recorded".to_string(),
                payload_json: json!({
                    "memory_id": created.id,
                    "packet_id": packet_id,
                    "from_agent_id": from_agent_id,
                    "to_agent_id": req.to_agent_id,
                    "scope": created.scope,
                    "redaction_applied": created.redaction_applied,
                    "expires_at": created.expires_at,
                }),
            },
        )
        .await
        .map_err(|err| {
            ApiError::internal(format!(
                "failed appending handoff memory audit event: {err}"
            ))
        })?;
    }

    Ok((
        StatusCode::CREATED,
        Json(handoff_packet_from_memory_record(created)?),
    ))
}

async fn list_handoff_packets_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<HandoffPacketQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let rows = list_tenant_handoff_memory_records_sqlite(
        &state,
        tenant_id.as_str(),
        query.to_agent_id,
        query.from_agent_id,
        limit,
    )
    .await?;

    let body = rows
        .into_iter()
        .map(handoff_packet_from_memory_record)
        .collect::<ApiResult<Vec<HandoffPacketResponse>>>()?;

    Ok((StatusCode::OK, Json(body)))
}

async fn list_memory_records_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryRecordQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let memory_kind = match query.memory_kind.as_deref() {
        Some(value) => {
            let Some(normalized) = normalize_memory_kind(value) else {
                return Err(ApiError::bad_request(
                    "memory_kind must be one of: session, semantic, procedural, handoff",
                ));
            };
            Some(normalized)
        }
        None => None,
    };
    let scope_prefix = trim_non_empty(query.scope_prefix.as_deref());
    if let Some(prefix) = scope_prefix {
        if !is_scope_allowed_for_capability("memory.read", prefix) {
            return Err(ApiError::bad_request(
                "scope_prefix must use memory scope prefix (`memory:`)",
            ));
        }
    }

    let rows = list_tenant_memory_records_sqlite(
        &state,
        tenant_id.as_str(),
        query.agent_id,
        memory_kind,
        scope_prefix,
        limit,
    )
    .await?;

    Ok((
        StatusCode::OK,
        Json(
            rows.into_iter()
                .map(memory_to_response)
                .collect::<Vec<MemoryRecordResponse>>(),
        ),
    ))
}

async fn retrieve_memory_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryRetrieveQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(20).clamp(1, 200);
    let memory_kind = match query.memory_kind.as_deref() {
        Some(value) => {
            let Some(normalized) = normalize_memory_kind(value) else {
                return Err(ApiError::bad_request(
                    "memory_kind must be one of: session, semantic, procedural, handoff",
                ));
            };
            Some(normalized)
        }
        None => None,
    };
    let scope_prefix = trim_non_empty(query.scope_prefix.as_deref());
    if let Some(prefix) = scope_prefix {
        if !is_scope_allowed_for_capability("memory.read", prefix) {
            return Err(ApiError::bad_request(
                "scope_prefix must use memory scope prefix (`memory:`)",
            ));
        }
    }
    let query_text = trim_non_empty(query.query_text.as_deref());
    let min_score = query.min_score;
    if let Some(value) = min_score {
        if !(0.0..=2.0).contains(&value) {
            return Err(ApiError::bad_request(
                "min_score must be between 0.0 and 2.0",
            ));
        }
    }
    let source_prefix = trim_non_empty(query.source_prefix.as_deref());
    let require_summary = query.require_summary.unwrap_or(false);

    let candidate_limit = if query_text.is_some()
        || min_score.is_some()
        || source_prefix.is_some()
        || require_summary
    {
        (limit.saturating_mul(5)).clamp(1, 1000)
    } else {
        limit
    };

    let rows = list_tenant_memory_records_sqlite(
        &state,
        tenant_id.as_str(),
        query.agent_id,
        memory_kind,
        scope_prefix,
        candidate_limit,
    )
    .await?;

    let filtered_rows = rows
        .into_iter()
        .filter(|row| {
            source_prefix
                .map(|prefix| row.source.starts_with(prefix))
                .unwrap_or(true)
        })
        .filter(|row| {
            !require_summary
                || row
                    .summary_text
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
        })
        .collect::<Vec<_>>();

    let query_tokens = query_text.map(tokenize_retrieval_query).unwrap_or_default();
    let total_candidates = filtered_rows.len();
    let mut scored_rows = filtered_rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let score = compute_memory_retrieval_score(
                &row,
                query_tokens.as_slice(),
                index,
                total_candidates.max(1),
            );
            (row, score)
        })
        .collect::<Vec<_>>();
    scored_rows.sort_by(|(left_row, left_score), (right_row, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right_row.created_at.cmp(&left_row.created_at))
            .then_with(|| right_row.id.cmp(&left_row.id))
    });

    let min_score = min_score.map(|value| value.clamp(0.0, 2.0));
    let items = scored_rows
        .into_iter()
        .filter(|(_, score)| {
            min_score
                .map(|threshold| *score >= threshold)
                .unwrap_or(true)
        })
        .take(limit as usize)
        .enumerate()
        .map(|(index, (row, score))| MemoryRetrievalItem {
            rank: (index + 1) as i64,
            score,
            citation: MemoryCitation {
                memory_id: row.id,
                created_at: row.created_at,
                source: row.source.clone(),
                memory_kind: row.memory_kind.clone(),
                scope: row.scope.clone(),
            },
            content_json: row.content_json,
            summary_text: row.summary_text,
        })
        .collect::<Vec<_>>();

    Ok((
        StatusCode::OK,
        Json(MemoryRetrieveResponse {
            tenant_id,
            limit,
            retrieved_count: items.len() as i64,
            agent_id: query.agent_id,
            memory_kind: memory_kind.map(ToString::to_string),
            scope_prefix: scope_prefix.map(ToString::to_string),
            query_text: query_text.map(ToString::to_string),
            min_score,
            source_prefix: source_prefix.map(ToString::to_string),
            require_summary,
            items,
        }),
    ))
}

async fn get_memory_compaction_stats_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryCompactionStatsQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.map(|value| value.clamp(1, 31_536_000));
    let since = window_secs
        .map(|seconds| OffsetDateTime::now_utc() - time::Duration::seconds(seconds as i64));
    let since_text = since
        .map(|value| {
            value.format(&Rfc3339).map_err(|err| {
                ApiError::internal(format!(
                    "failed formatting memory compaction stats since: {err}"
                ))
            })
        })
        .transpose()?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let row = sqlx::query(
        r#"
        SELECT
          (SELECT COUNT(*)
           FROM memory_compactions
           WHERE tenant_id = ?1
             AND (?2 IS NULL OR datetime(created_at) >= datetime(?2))) AS compacted_groups_window,
          (SELECT COALESCE(SUM(source_count), 0)
           FROM memory_compactions
           WHERE tenant_id = ?1
             AND (?2 IS NULL OR datetime(created_at) >= datetime(?2))) AS compacted_source_records_window,
          (SELECT COUNT(*)
           FROM memory_records
           WHERE tenant_id = ?1
             AND compacted_at IS NULL) AS pending_uncompacted_records,
          (SELECT MAX(created_at)
           FROM memory_compactions
           WHERE tenant_id = ?1) AS last_compacted_at
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(since_text)
    .fetch_one(sqlite)
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed querying memory compaction stats: {err}"))
    })?;

    Ok((
        StatusCode::OK,
        Json(MemoryCompactionStatsResponse {
            tenant_id,
            window_secs,
            since,
            compacted_groups_window: row.get("compacted_groups_window"),
            compacted_source_records_window: row.get("compacted_source_records_window"),
            pending_uncompacted_records: row.get("pending_uncompacted_records"),
            last_compacted_at: parse_sqlite_datetime_optional(&row, "last_compacted_at")?,
        }),
    ))
}

async fn purge_memory_records_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<PurgeMemoryRecordsRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_owner_role(role_preset, "only owner can purge memory records")?;

    let as_of = req.as_of.unwrap_or_else(OffsetDateTime::now_utc);
    let as_of_text = as_of.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting memory purge as_of: {err}"))
    })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let run_impact_rows = sqlx::query(
        r#"
        SELECT run_id, COUNT(*) AS row_count
        FROM memory_records
        WHERE tenant_id = ?1
          AND expires_at IS NOT NULL
          AND datetime(expires_at) <= datetime(?2)
          AND run_id IS NOT NULL
        GROUP BY run_id
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(as_of_text.as_str())
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed loading memory purge run impacts: {err}")))?;

    let deleted_count = sqlx::query(
        r#"
        DELETE FROM memory_records
        WHERE tenant_id = ?1
          AND expires_at IS NOT NULL
          AND datetime(expires_at) <= datetime(?2)
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(as_of_text.as_str())
    .execute(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed purging memory records: {err}")))?
    .rows_affected() as i64;

    for row in run_impact_rows {
        let raw_run_id: String = row.get("run_id");
        let run_deleted_count: i64 = row.get("row_count");
        if run_deleted_count <= 0 {
            continue;
        }
        let run_id = Uuid::parse_str(raw_run_id.as_str()).map_err(|err| {
            ApiError::internal(format!(
                "invalid run_id in memory purge impact row: {err} (value={raw_run_id})"
            ))
        })?;

        let Some(run) = get_run_status_dual(&state.db_pool, tenant_id.as_str(), run_id)
            .await
            .map_err(|err| {
                ApiError::internal(format!("failed loading run for memory purge audit: {err}"))
            })?
        else {
            continue;
        };

        append_audit_event_dual(
            &state.db_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: None,
                tenant_id: tenant_id.clone(),
                agent_id: Some(run.agent_id),
                user_id: run.triggered_by_user_id,
                actor: "api".to_string(),
                event_type: "memory.purged".to_string(),
                payload_json: json!({
                    "as_of": as_of,
                    "run_deleted_count": run_deleted_count,
                    "tenant_deleted_count": deleted_count,
                }),
            },
        )
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed appending memory.purged audit event: {err}"))
        })?;
    }

    Ok((
        StatusCode::OK,
        Json(MemoryPurgeResponse {
            tenant_id,
            deleted_count,
            as_of,
        }),
    ))
}

async fn get_llm_usage_tokens_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<LlmUsageQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let since_text = since.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting usage since timestamp: {err}"))
    })?;
    let model_key = query
        .model_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;

    let row = sqlx::query(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0) AS tokens,
               COALESCE(SUM(estimated_cost_usd), 0.0) AS estimated_cost_usd
        FROM llm_token_usage
        WHERE tenant_id = ?1
          AND route = 'remote'
          AND datetime(created_at) >= datetime(?2)
          AND (?3 IS NULL OR agent_id = ?3)
          AND (?4 IS NULL OR model_key = ?4)
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(since_text)
    .bind(query.agent_id.map(|id| id.to_string()))
    .bind(model_key)
    .fetch_one(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying llm usage totals: {err}")))?;
    let tokens: i64 = row.get("tokens");
    let estimated_cost_usd: f64 = row.get("estimated_cost_usd");

    Ok((
        StatusCode::OK,
        Json(LlmUsageResponse {
            tenant_id,
            window_secs,
            since,
            tokens,
            estimated_cost_usd,
            agent_id: query.agent_id,
            model_key: model_key.map(ToString::to_string),
        }),
    ))
}

async fn get_payments_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<PaymentLedgerQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);

    let status = trim_non_empty(query.status.as_deref());
    let destination = trim_non_empty(query.destination.as_deref());
    let idempotency_key = trim_non_empty(query.idempotency_key.as_deref());
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let body = list_tenant_payment_ledger_from_sqlite(
        sqlite,
        tenant_id.as_str(),
        query.run_id,
        query.agent_id,
        status,
        destination,
        idempotency_key,
        limit,
    )
    .await?;

    Ok((StatusCode::OK, Json(body)))
}

async fn list_tenant_payment_ledger_from_sqlite(
    sqlite: &SqlitePool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    status: Option<&str>,
    destination: Option<&str>,
    idempotency_key: Option<&str>,
    limit: i64,
) -> ApiResult<Vec<PaymentLedgerResponse>> {
    let rows = sqlx::query(
        r#"
        SELECT pr.id,
               pr.action_request_id,
               pr.run_id,
               pr.tenant_id,
               pr.agent_id,
               pr.provider,
               pr.operation,
               pr.destination,
               pr.idempotency_key,
               pr.amount_msat,
               pr.request_json,
               pr.status,
               pr.created_at,
               pr.updated_at,
               latest.status AS latest_result_status,
               latest.result_json AS latest_result_json,
               latest.error_json AS latest_error_json,
               latest.created_at AS latest_result_created_at
        FROM payment_requests pr
        LEFT JOIN payment_results latest
          ON latest.id = (
              SELECT id
              FROM payment_results
              WHERE payment_request_id = pr.id
              ORDER BY datetime(created_at) DESC, id DESC
              LIMIT 1
          )
        WHERE pr.tenant_id = ?1
          AND (?2 IS NULL OR pr.run_id = ?2)
          AND (?3 IS NULL OR pr.agent_id = ?3)
          AND (?4 IS NULL OR pr.status = ?4)
          AND (?5 IS NULL OR pr.destination = ?5)
          AND (?6 IS NULL OR pr.idempotency_key = ?6)
        ORDER BY datetime(pr.created_at) DESC, pr.id DESC
        LIMIT ?7
        "#,
    )
    .bind(tenant_id)
    .bind(run_id.map(|id| id.to_string()))
    .bind(agent_id.map(|id| id.to_string()))
    .bind(status)
    .bind(destination)
    .bind(idempotency_key)
    .bind(limit)
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying payment ledger: {err}")))?;

    rows.into_iter()
        .map(|row| {
            let latest_result_json = parse_sqlite_json_optional(&row, "latest_result_json")?;
            let latest_error_json = parse_sqlite_json_optional(&row, "latest_error_json")?;
            let provider: String = row.get("provider");
            let status: String = row.get("status");
            let latest_result_status: Option<String> = row.get("latest_result_status");
            let settlement_status = latest_result_json
                .as_ref()
                .and_then(|json| json.get("settlement_status"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let settlement_rail = latest_result_json
                .as_ref()
                .and_then(|json| json.get("rail"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| Some(provider.clone()));
            let normalized_outcome =
                normalize_payment_outcome(status.as_str(), latest_result_status.as_deref())
                    .to_string();
            let normalized_error_code = latest_error_json
                .as_ref()
                .and_then(|json| json.get("code"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let normalized_error_class = normalized_error_code
                .as_deref()
                .map(classify_payment_error_code)
                .map(ToString::to_string);

            Ok(PaymentLedgerResponse {
                id: parse_sqlite_uuid_required(&row, "id")?,
                action_request_id: parse_sqlite_uuid_required(&row, "action_request_id")?,
                run_id: parse_sqlite_uuid_required(&row, "run_id")?,
                tenant_id: row.get("tenant_id"),
                agent_id: parse_sqlite_uuid_required(&row, "agent_id")?,
                provider,
                operation: row.get("operation"),
                destination: row.get("destination"),
                idempotency_key: row.get("idempotency_key"),
                amount_msat: row.get("amount_msat"),
                status,
                request_json: parse_sqlite_json_required(&row, "request_json")?,
                latest_result_status,
                latest_result_json,
                latest_error_json,
                settlement_status,
                settlement_rail,
                normalized_outcome,
                normalized_error_code,
                normalized_error_class,
                created_at: parse_sqlite_datetime_required(&row, "created_at")?,
                updated_at: parse_sqlite_datetime_required(&row, "updated_at")?,
                latest_result_created_at: parse_sqlite_datetime_optional(
                    &row,
                    "latest_result_created_at",
                )?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()
}

async fn get_payment_summary_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<PaymentSummaryQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.map(|value| value.clamp(1, 31_536_000));
    let since = window_secs
        .map(|seconds| OffsetDateTime::now_utc() - time::Duration::seconds(seconds as i64));
    let since_text = since
        .map(|value| {
            value.format(&Rfc3339).map_err(|err| {
                ApiError::internal(format!("failed formatting payment summary since: {err}"))
            })
        })
        .transpose()?;
    let operation = trim_non_empty(query.operation.as_deref());
    if let Some(value) = operation {
        let is_valid = matches!(value, "pay_invoice" | "make_invoice" | "get_balance");
        if !is_valid {
            return Err(ApiError::bad_request(
                "operation must be one of: pay_invoice, make_invoice, get_balance",
            ));
        }
    }
    let agent_id = query.agent_id.map(|id| id.to_string());
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;

    let row = sqlx::query(
        r#"
        SELECT COUNT(*) AS total_requests,
               SUM(CASE WHEN status = 'requested' THEN 1 ELSE 0 END) AS requested_count,
               SUM(CASE WHEN status = 'executed' THEN 1 ELSE 0 END) AS executed_count,
               SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed_count,
               SUM(CASE WHEN status = 'duplicate' THEN 1 ELSE 0 END) AS duplicate_count,
               SUM(
                   CASE
                     WHEN operation = 'pay_invoice' AND status = 'executed' THEN COALESCE(amount_msat, 0)
                     ELSE 0
                   END
               ) AS executed_spend_msat
        FROM payment_requests
        WHERE tenant_id = ?1
          AND (?2 IS NULL OR datetime(created_at) >= datetime(?2))
          AND (?3 IS NULL OR agent_id = ?3)
          AND (?4 IS NULL OR operation = ?4)
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(since_text)
    .bind(agent_id)
    .bind(operation)
    .fetch_one(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying payment summary: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(PaymentSummaryResponse {
            tenant_id,
            window_secs,
            since,
            agent_id: query.agent_id,
            operation: operation.map(ToString::to_string),
            total_requests: row.get("total_requests"),
            requested_count: row.get("requested_count"),
            executed_count: row.get("executed_count"),
            failed_count: row.get("failed_count"),
            duplicate_count: row.get("duplicate_count"),
            executed_spend_msat: row.get("executed_spend_msat"),
        }),
    ))
}

async fn create_trigger_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    if req.interval_seconds <= 0 {
        return Err(ApiError::bad_request(
            "interval_seconds must be greater than zero",
        ));
    }
    if req.interval_seconds > 31_536_000 {
        return Err(ApiError::bad_request(
            "interval_seconds exceeds maximum of 31536000",
        ));
    }
    if req.max_inflight_runs <= 0 || req.max_inflight_runs > 1000 {
        return Err(ApiError::bad_request(
            "max_inflight_runs must be between 1 and 1000",
        ));
    }
    if req.jitter_seconds < 0 || req.jitter_seconds > 3600 {
        return Err(ApiError::bad_request(
            "jitter_seconds must be between 0 and 3600",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let effective_triggered_by_user_id =
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?;
    let requested_capabilities = req.requested_capabilities;
    let granted_capabilities =
        resolve_granted_capabilities(req.recipe_id.as_str(), role_preset, &requested_capabilities)?;
    ensure_tenant_trigger_capacity(&state, tenant_id.as_str()).await?;

    let created = create_interval_trigger(
        &state.pool,
        &NewIntervalTrigger {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            agent_id: req.agent_id,
            triggered_by_user_id: effective_triggered_by_user_id,
            recipe_id: req.recipe_id,
            interval_seconds: req.interval_seconds,
            input_json: req.input,
            requested_capabilities,
            granted_capabilities,
            next_fire_at: OffsetDateTime::now_utc() + time::Duration::seconds(req.interval_seconds),
            status: "enabled".to_string(),
            misfire_policy: "fire_now".to_string(),
            max_attempts: 3,
            max_inflight_runs: req.max_inflight_runs,
            jitter_seconds: req.jitter_seconds,
            webhook_secret_ref: None,
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed creating trigger: {err}")))?;

    append_trigger_audit(
        &state.pool,
        &tenant_id,
        created.id,
        role_preset,
        "trigger.created",
        json!({
            "trigger_type": created.trigger_type,
            "interval_seconds": created.interval_seconds,
            "max_inflight_runs": created.max_inflight_runs,
            "jitter_seconds": created.jitter_seconds,
        }),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(trigger_to_response(created))))
}

async fn create_cron_trigger_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateCronTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    if req.cron_expression.trim().is_empty() {
        return Err(ApiError::bad_request("cron_expression must not be empty"));
    }
    if req.schedule_timezone.trim().is_empty() {
        return Err(ApiError::bad_request("schedule_timezone must not be empty"));
    }
    if req.max_attempts <= 0 || req.max_attempts > 20 {
        return Err(ApiError::bad_request(
            "max_attempts must be between 1 and 20",
        ));
    }
    if req.max_inflight_runs <= 0 || req.max_inflight_runs > 1000 {
        return Err(ApiError::bad_request(
            "max_inflight_runs must be between 1 and 1000",
        ));
    }
    if req.jitter_seconds < 0 || req.jitter_seconds > 3600 {
        return Err(ApiError::bad_request(
            "jitter_seconds must be between 0 and 3600",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let effective_triggered_by_user_id =
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?;
    let requested_capabilities = req.requested_capabilities;
    let granted_capabilities =
        resolve_granted_capabilities(req.recipe_id.as_str(), role_preset, &requested_capabilities)?;
    ensure_tenant_trigger_capacity(&state, tenant_id.as_str()).await?;

    let created = create_cron_trigger(
        &state.pool,
        &NewCronTrigger {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            agent_id: req.agent_id,
            triggered_by_user_id: effective_triggered_by_user_id,
            recipe_id: req.recipe_id,
            cron_expression: req.cron_expression.trim().to_string(),
            schedule_timezone: req.schedule_timezone.trim().to_string(),
            input_json: req.input,
            requested_capabilities,
            granted_capabilities,
            status: "enabled".to_string(),
            misfire_policy: "fire_now".to_string(),
            max_attempts: req.max_attempts,
            max_inflight_runs: req.max_inflight_runs,
            jitter_seconds: req.jitter_seconds,
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed creating cron trigger: {err}")))?;

    append_trigger_audit(
        &state.pool,
        &tenant_id,
        created.id,
        role_preset,
        "trigger.created",
        json!({
            "trigger_type": created.trigger_type,
            "cron_expression": created.cron_expression,
            "schedule_timezone": created.schedule_timezone,
            "max_inflight_runs": created.max_inflight_runs,
            "jitter_seconds": created.jitter_seconds,
        }),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(trigger_to_response(created))))
}

async fn create_webhook_trigger_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateWebhookTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    if req.max_attempts <= 0 || req.max_attempts > 20 {
        return Err(ApiError::bad_request(
            "max_attempts must be between 1 and 20",
        ));
    }
    if req.max_inflight_runs <= 0 || req.max_inflight_runs > 1000 {
        return Err(ApiError::bad_request(
            "max_inflight_runs must be between 1 and 1000",
        ));
    }
    if req.jitter_seconds < 0 || req.jitter_seconds > 3600 {
        return Err(ApiError::bad_request(
            "jitter_seconds must be between 0 and 3600",
        ));
    }
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let effective_triggered_by_user_id =
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?;
    let requested_capabilities = req.requested_capabilities;
    let granted_capabilities =
        resolve_granted_capabilities(req.recipe_id.as_str(), role_preset, &requested_capabilities)?;
    ensure_tenant_trigger_capacity(&state, tenant_id.as_str()).await?;

    let created = create_webhook_trigger(
        &state.pool,
        &NewWebhookTrigger {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            agent_id: req.agent_id,
            triggered_by_user_id: effective_triggered_by_user_id,
            recipe_id: req.recipe_id,
            input_json: req.input,
            requested_capabilities,
            granted_capabilities,
            status: "enabled".to_string(),
            max_attempts: req.max_attempts,
            max_inflight_runs: req.max_inflight_runs,
            jitter_seconds: req.jitter_seconds,
            webhook_secret_ref: req.webhook_secret_ref,
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed creating webhook trigger: {err}")))?;

    append_trigger_audit(
        &state.pool,
        &tenant_id,
        created.id,
        role_preset,
        "trigger.created",
        json!({
            "trigger_type": created.trigger_type,
            "max_attempts": created.max_attempts,
            "max_inflight_runs": created.max_inflight_runs,
            "jitter_seconds": created.jitter_seconds,
            "webhook_secret_configured": created.webhook_secret_ref.is_some(),
        }),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(trigger_to_response(created))))
}

async fn update_trigger_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
    Json(req): Json<UpdateTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;

    if let Some(seconds) = req.interval_seconds {
        if seconds <= 0 || seconds > 31_536_000 {
            return Err(ApiError::bad_request(
                "interval_seconds must be between 1 and 31536000",
            ));
        }
    }
    if let Some(ref expression) = req.cron_expression {
        if expression.trim().is_empty() {
            return Err(ApiError::bad_request("cron_expression must not be empty"));
        }
    }
    if let Some(ref timezone) = req.schedule_timezone {
        if timezone.trim().is_empty() {
            return Err(ApiError::bad_request("schedule_timezone must not be empty"));
        }
    }
    if let Some(ref policy) = req.misfire_policy {
        if policy != "fire_now" && policy != "skip" {
            return Err(ApiError::bad_request(
                "misfire_policy must be one of: fire_now, skip",
            ));
        }
    }
    if let Some(attempts) = req.max_attempts {
        if attempts <= 0 || attempts > 20 {
            return Err(ApiError::bad_request(
                "max_attempts must be between 1 and 20",
            ));
        }
    }
    if let Some(max_inflight_runs) = req.max_inflight_runs {
        if max_inflight_runs <= 0 || max_inflight_runs > 1000 {
            return Err(ApiError::bad_request(
                "max_inflight_runs must be between 1 and 1000",
            ));
        }
    }
    if let Some(jitter_seconds) = req.jitter_seconds {
        if !(0..=3600).contains(&jitter_seconds) {
            return Err(ApiError::bad_request(
                "jitter_seconds must be between 0 and 3600",
            ));
        }
    }

    let Some(existing) = get_trigger(&state.pool, tenant_id.as_str(), trigger_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading trigger: {err}")))?
    else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, existing.triggered_by_user_id)?;

    match existing.trigger_type.as_str() {
        "interval" => {
            if req.cron_expression.is_some() || req.schedule_timezone.is_some() {
                return Err(ApiError::bad_request(
                    "interval trigger does not support cron schedule fields",
                ));
            }
        }
        "cron" => {
            if req.interval_seconds.is_some() {
                return Err(ApiError::bad_request(
                    "cron trigger does not support interval_seconds",
                ));
            }
        }
        "webhook" => {
            if req.interval_seconds.is_some()
                || req.cron_expression.is_some()
                || req.schedule_timezone.is_some()
            {
                return Err(ApiError::bad_request(
                    "webhook trigger does not support schedule fields",
                ));
            }
        }
        _ => {}
    }

    let updated = update_trigger_config(
        &state.pool,
        tenant_id.as_str(),
        trigger_id,
        &UpdateTriggerParams {
            interval_seconds: req.interval_seconds,
            cron_expression: req.cron_expression.map(|value| value.trim().to_string()),
            schedule_timezone: req.schedule_timezone.map(|value| value.trim().to_string()),
            misfire_policy: req.misfire_policy,
            max_attempts: req.max_attempts,
            max_inflight_runs: req.max_inflight_runs,
            jitter_seconds: req.jitter_seconds,
            webhook_secret_ref: req.webhook_secret_ref,
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed updating trigger: {err}")))?
    .ok_or_else(|| ApiError::not_found("trigger not found"))?;

    append_trigger_audit(
        &state.pool,
        &tenant_id,
        trigger_id,
        role_preset,
        "trigger.updated",
        json!({
            "trigger_type": updated.trigger_type,
            "interval_seconds": updated.interval_seconds,
            "cron_expression": updated.cron_expression,
            "schedule_timezone": updated.schedule_timezone,
            "misfire_policy": updated.misfire_policy,
            "max_attempts": updated.max_attempts,
            "max_inflight_runs": updated.max_inflight_runs,
            "jitter_seconds": updated.jitter_seconds,
            "webhook_secret_configured": updated.webhook_secret_ref.is_some(),
        }),
    )
    .await?;

    Ok((StatusCode::OK, Json(trigger_to_response(updated))))
}

async fn enable_trigger_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    set_trigger_status_handler(state, headers, trigger_id, "enabled", "trigger.enabled").await
}

async fn disable_trigger_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    set_trigger_status_handler(state, headers, trigger_id, "disabled", "trigger.disabled").await
}

async fn ingest_trigger_event_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
    Json(req): Json<TriggerEventRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    if req.event_id.trim().is_empty() {
        return Err(ApiError::bad_request("event_id must not be empty"));
    }
    if req.payload.as_object().is_none() {
        return Err(ApiError::bad_request(
            "trigger event payload must be a JSON object",
        ));
    }

    let Some(trigger) = get_trigger(&state.pool, tenant_id.as_str(), trigger_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading trigger: {err}")))?
    else {
        return Err(ApiError::not_found("trigger not found"));
    };
    if trigger.trigger_type != "webhook" {
        return Err(ApiError::bad_request(
            "trigger does not accept webhook events",
        ));
    }
    if trigger.status != "enabled" || trigger.dead_lettered_at.is_some() {
        return Err(ApiError::conflict("trigger is not enabled"));
    }
    if let Some(reference) = trigger.webhook_secret_ref {
        let provided = headers
            .get(TRIGGER_SECRET_HEADER)
            .ok_or_else(|| ApiError::bad_request("missing x-trigger-secret header"))?
            .to_str()
            .map_err(|_| ApiError::bad_request("x-trigger-secret must be valid UTF-8"))?;
        let expected = resolve_secret_value(None, Some(reference), shared_secret_resolver())
            .map_err(|err| ApiError::internal(format!("failed resolving trigger secret: {err}")))?
            .ok_or_else(|| ApiError::internal("trigger secret reference resolved to empty"))?;
        if provided != expected {
            return Err(ApiError {
                status: StatusCode::UNAUTHORIZED,
                code: "UNAUTHORIZED",
                message: "trigger secret validation failed".to_string(),
            });
        }
    }

    let outcome = enqueue_trigger_event(
        &state.pool,
        tenant_id.as_str(),
        trigger_id,
        req.event_id.as_str(),
        req.payload,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed enqueueing trigger event: {err}")))?;

    let status = match outcome {
        TriggerEventEnqueueOutcome::Enqueued => "queued",
        TriggerEventEnqueueOutcome::Duplicate => "duplicate",
        TriggerEventEnqueueOutcome::TriggerUnavailable { reason } => {
            return Err(map_trigger_enqueue_unavailable_reason(reason));
        }
    };

    Ok((
        StatusCode::ACCEPTED,
        Json(TriggerEventIngestResponse {
            trigger_id,
            event_id: req.event_id,
            status,
        }),
    ))
}

async fn fire_trigger_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(trigger_id): Path<Uuid>,
    Json(req): Json<FireTriggerRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;

    let idempotency_key = req.idempotency_key.trim();
    if idempotency_key.is_empty() {
        return Err(ApiError::bad_request("idempotency_key must not be empty"));
    }
    if idempotency_key.len() > 128 {
        return Err(ApiError::bad_request(
            "idempotency_key exceeds maximum length of 128",
        ));
    }

    let Some(trigger) = get_trigger(&state.pool, tenant_id.as_str(), trigger_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading trigger: {err}")))?
    else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, trigger.triggered_by_user_id)?;
    if trigger.status != "enabled" || trigger.dead_lettered_at.is_some() {
        return Err(ApiError::conflict("trigger is not enabled"));
    }

    let outcome = fire_trigger_manually(
        &state.pool,
        tenant_id.as_str(),
        trigger_id,
        idempotency_key,
        req.payload,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed firing trigger: {err}")))?;

    match outcome {
        ManualTriggerFireOutcome::Created(dispatched) => {
            append_trigger_audit(
                &state.pool,
                &tenant_id,
                trigger_id,
                role_preset,
                "trigger.fired_manual",
                json!({
                    "status": "created",
                    "run_id": dispatched.run_id,
                    "idempotency_key": idempotency_key,
                }),
            )
            .await?;

            append_audit_event(
                &state.pool,
                &NewAuditEvent {
                    id: Uuid::new_v4(),
                    run_id: dispatched.run_id,
                    step_id: None,
                    tenant_id: tenant_id.clone(),
                    agent_id: Some(dispatched.agent_id),
                    user_id: dispatched.triggered_by_user_id,
                    actor: "api".to_string(),
                    event_type: "run.created".to_string(),
                    payload_json: json!({
                        "source": "trigger_manual_api",
                        "trigger_id": dispatched.trigger_id,
                        "trigger_type": dispatched.trigger_type,
                        "trigger_event_id": dispatched.trigger_event_id,
                        "role_preset": role_preset.as_str(),
                        "scheduled_for": dispatched.scheduled_for,
                    }),
                },
            )
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed appending manual trigger run.created audit event: {err}"
                ))
            })?;

            Ok((
                StatusCode::ACCEPTED,
                Json(TriggerFireResponse {
                    trigger_id,
                    run_id: Some(dispatched.run_id),
                    idempotency_key: idempotency_key.to_string(),
                    status: "created",
                }),
            ))
        }
        ManualTriggerFireOutcome::Duplicate { run_id } => {
            append_trigger_audit(
                &state.pool,
                &tenant_id,
                trigger_id,
                role_preset,
                "trigger.fired_manual",
                json!({
                    "status": "duplicate",
                    "run_id": run_id,
                    "idempotency_key": idempotency_key,
                }),
            )
            .await?;

            Ok((
                StatusCode::OK,
                Json(TriggerFireResponse {
                    trigger_id,
                    run_id,
                    idempotency_key: idempotency_key.to_string(),
                    status: "duplicate",
                }),
            ))
        }
        ManualTriggerFireOutcome::InflightLimited => Err(ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "TRIGGER_INFLIGHT_LIMITED",
            message: "trigger or tenant is at max inflight run capacity".to_string(),
        }),
        ManualTriggerFireOutcome::TriggerUnavailable => {
            Err(ApiError::conflict("trigger is not enabled"))
        }
    }
}

async fn replay_trigger_event_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((trigger_id, event_id)): Path<(Uuid, String)>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;

    let event_id = event_id.trim();
    if event_id.is_empty() {
        return Err(ApiError::bad_request("event_id must not be empty"));
    }

    let Some(trigger) = get_trigger(&state.pool, tenant_id.as_str(), trigger_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading trigger: {err}")))?
    else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, trigger.triggered_by_user_id)?;
    if trigger.trigger_type != "webhook" {
        return Err(ApiError::bad_request(
            "trigger does not support event replay",
        ));
    }
    if trigger.status != "enabled" || trigger.dead_lettered_at.is_some() {
        return Err(ApiError::conflict("trigger is not enabled"));
    }

    let replay_outcome =
        requeue_dead_letter_trigger_event(&state.pool, tenant_id.as_str(), trigger_id, event_id)
            .await
            .map_err(|err| ApiError::internal(format!("failed replaying trigger event: {err}")))?;

    match replay_outcome {
        TriggerEventReplayOutcome::Requeued => {
            append_trigger_audit(
                &state.pool,
                &tenant_id,
                trigger_id,
                role_preset,
                "trigger.event.replayed",
                json!({
                    "event_id": event_id,
                    "status": "queued_for_replay",
                }),
            )
            .await?;
            Ok((
                StatusCode::ACCEPTED,
                Json(TriggerEventIngestResponse {
                    trigger_id,
                    event_id: event_id.to_string(),
                    status: "queued_for_replay",
                }),
            ))
        }
        TriggerEventReplayOutcome::NotFound => Err(ApiError::not_found("trigger event not found")),
        TriggerEventReplayOutcome::NotDeadLettered { status } => Err(ApiError {
            status: StatusCode::CONFLICT,
            code: "TRIGGER_EVENT_NOT_REPLAYABLE",
            message: format!("trigger event cannot be replayed from status `{status}`"),
        }),
    }
}

async fn get_run_audit_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
    Query(query): Query<AuditQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);

    let run_exists = get_run_status_dual(&state.db_pool, &tenant_id, run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed checking run existence: {err}")))?
        .is_some();
    if !run_exists {
        return Err(ApiError::not_found("run not found"));
    }

    let events = list_run_audit_events_dual(&state.db_pool, &tenant_id, run_id, limit)
        .await
        .map_err(|err| ApiError::internal(format!("failed fetching run audit events: {err}")))?;

    let body: Vec<AuditEventResponse> = events
        .into_iter()
        .map(|event| AuditEventResponse {
            id: event.id,
            run_id: event.run_id,
            step_id: event.step_id,
            actor: event.actor,
            event_type: event.event_type,
            payload_json: event.payload_json,
            created_at: event.created_at,
        })
        .collect();

    Ok((StatusCode::OK, Json(body)))
}

async fn get_run_audit_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
    Query(query): Query<AuditQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);

    let run_exists = get_run_status_dual(&state.db_pool, &tenant_id, run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed checking run existence: {err}")))?
        .is_some();
    if !run_exists {
        return Err(ApiError::not_found("run not found"));
    }

    let events = list_run_audit_events_dual(&state.db_pool, &tenant_id, run_id, limit)
        .await
        .map_err(|err| ApiError::internal(format!("failed fetching run audit events: {err}")))?;

    let body: Vec<AuditEventResponse> = events
        .into_iter()
        .map(|event| AuditEventResponse {
            id: event.id,
            run_id: event.run_id,
            step_id: event.step_id,
            actor: event.actor,
            event_type: event.event_type,
            payload_json: event.payload_json,
            created_at: event.created_at,
        })
        .collect();

    Ok((StatusCode::OK, Json(body)))
}

async fn get_agent_context_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let snapshot = load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
        .map_err(map_agent_context_load_error)?;
    let summary_digest_sha256 = snapshot.summary_digest_sha256().map_err(|err| {
        ApiError::internal(format!("failed serializing agent context summary: {err}"))
    })?;
    let aggregate_sha256 = snapshot.aggregate_sha256();

    Ok((
        StatusCode::OK,
        Json(AgentContextInspectResponse {
            tenant_id,
            agent_id,
            source_dir: snapshot.source_dir.display().to_string(),
            loaded_at: snapshot.loaded_at,
            loaded_file_count: snapshot.loaded_file_count(),
            total_loaded_bytes: snapshot.total_loaded_bytes(),
            aggregate_sha256,
            summary_digest_sha256,
            missing_required_files: snapshot.missing_required_files.clone(),
            warnings: snapshot.warnings.clone(),
            precedence_order: agent_context_precedence_order(),
            required_files: snapshot
                .required_files
                .iter()
                .map(agent_context_file_to_response)
                .collect(),
            memory_files: snapshot
                .memory_files
                .iter()
                .map(agent_context_file_to_response)
                .collect(),
            session_files: snapshot
                .session_files
                .iter()
                .map(agent_context_file_to_response)
                .collect(),
        }),
    ))
}

async fn get_agent_bootstrap_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    if !state.agent_bootstrap_enabled {
        return Ok((
            StatusCode::OK,
            Json(AgentBootstrapInspectResponse {
                tenant_id,
                agent_id,
                enabled: false,
                status: "disabled".to_string(),
                source_dir: "".to_string(),
                bootstrap_present: false,
                bootstrap_path: None,
                bootstrap_sha256: None,
                bootstrap_bytes: None,
                bootstrap_markdown: None,
                completed_at: None,
                completed_by_user_id: None,
                completion_note: None,
                updated_files: Vec::new(),
            }),
        ));
    }

    let snapshot = load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
        .map_err(map_agent_context_load_error)?;
    let bootstrap_path = snapshot.source_dir.join(BOOTSTRAP_FILE_NAME);
    let bootstrap_bytes = fs::read(&bootstrap_path).ok();
    let bootstrap_markdown = bootstrap_bytes
        .as_deref()
        .and_then(|value| String::from_utf8(value.to_vec()).ok());
    let bootstrap_sha256 = bootstrap_bytes
        .as_deref()
        .map(|value| format!("{:x}", Sha256::digest(value)));
    let bootstrap_size = bootstrap_bytes.as_ref().map(Vec::len);
    let completion = find_latest_bootstrap_completion(&snapshot.session_files)?;
    let status = if completion.is_some() {
        "completed".to_string()
    } else if bootstrap_markdown.is_some() {
        "pending".to_string()
    } else {
        "not_configured".to_string()
    };

    Ok((
        StatusCode::OK,
        Json(AgentBootstrapInspectResponse {
            tenant_id,
            agent_id,
            enabled: true,
            status,
            source_dir: snapshot.source_dir.display().to_string(),
            bootstrap_present: bootstrap_markdown.is_some(),
            bootstrap_path: bootstrap_markdown
                .as_ref()
                .map(|_| bootstrap_path.display().to_string()),
            bootstrap_sha256,
            bootstrap_bytes: bootstrap_size,
            bootstrap_markdown,
            completed_at: completion.as_ref().map(|entry| entry.completed_at),
            completed_by_user_id: completion.as_ref().map(|entry| entry.completed_by_user_id),
            completion_note: completion
                .as_ref()
                .and_then(|entry| entry.completion_note.clone()),
            updated_files: completion
                .map(|entry| entry.updated_files)
                .unwrap_or_default(),
        }),
    ))
}

async fn complete_agent_bootstrap_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<CompleteBootstrapRequest>,
) -> ApiResult<impl IntoResponse> {
    if !state.agent_bootstrap_enabled {
        return Err(ApiError::forbidden(
            "agent bootstrap is disabled by API_AGENT_BOOTSTRAP_ENABLED",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_owner_role(
        role_preset,
        "only owner role can complete agent bootstrap workflow",
    )?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?.ok_or_else(|| {
        ApiError::forbidden("bootstrap completion requires x-user-id for attribution")
    })?;
    let snapshot = load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
        .map_err(map_agent_context_load_error)?;

    let bootstrap_path = snapshot.source_dir.join(BOOTSTRAP_FILE_NAME);
    if !bootstrap_path.is_file() {
        return Err(ApiError::conflict(
            "BOOTSTRAP.md is missing; add the file before completing bootstrap",
        ));
    }
    let existing_completion = find_latest_bootstrap_completion(&snapshot.session_files)?;
    if existing_completion.is_some() && !req.force {
        return Err(ApiError::conflict(
            "bootstrap is already completed; pass force=true to record a new completion event",
        ));
    }

    let mut updates = Vec::new();
    if let Some(content) = normalize_optional_text(req.identity_markdown.as_deref()) {
        updates.push(("IDENTITY.md".to_string(), content.to_string()));
    }
    if let Some(content) = normalize_optional_text(req.soul_markdown.as_deref()) {
        updates.push(("SOUL.md".to_string(), content.to_string()));
    }
    if let Some(content) = normalize_optional_text(req.user_markdown.as_deref()) {
        updates.push(("USER.md".to_string(), content.to_string()));
    }
    if let Some(content) = normalize_optional_text(req.heartbeat_markdown.as_deref()) {
        updates.push(("HEARTBEAT.md".to_string(), content.to_string()));
    }

    let mut written_files = Vec::with_capacity(updates.len());
    for (relative_path, content) in updates {
        let full_path = snapshot.source_dir.join(relative_path.as_str());
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                ApiError::internal(format!(
                    "failed creating context file directory {}: {err}",
                    parent.display()
                ))
            })?;
        }
        if content.as_bytes().len() > state.agent_context_loader.max_file_bytes {
            return Err(ApiError::bad_request(format!(
                "{} exceeds max file size ({} bytes)",
                relative_path, state.agent_context_loader.max_file_bytes
            )));
        }
        fs::write(&full_path, content.as_bytes()).map_err(|err| {
            ApiError::internal(format!(
                "failed writing bootstrap target {}: {err}",
                full_path.display()
            ))
        })?;
        let bytes = fs::read(&full_path).map_err(|err| {
            ApiError::internal(format!(
                "failed reading bootstrap target {}: {err}",
                full_path.display()
            ))
        })?;
        written_files.push(BootstrapFileWriteResponse {
            relative_path,
            sha256: format!("{:x}", Sha256::digest(bytes.as_slice())),
            bytes: bytes.len(),
        });
    }

    let completed_at = OffsetDateTime::now_utc();
    let completion_note =
        normalize_optional_text(req.completion_note.as_deref()).map(str::to_string);
    let completed_at_rfc3339 = completed_at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|err| {
            ApiError::internal(format!(
                "failed formatting bootstrap completion timestamp: {err}"
            ))
        })?;
    let status_record = json!({
        "schema_version": "v1",
        "status": "completed",
        "completed_at": completed_at_rfc3339,
        "completed_by_user_id": actor_user_id,
        "completion_note": completion_note,
        "updated_files": written_files.iter().map(|item| item.relative_path.clone()).collect::<Vec<_>>(),
    });
    let status_payload = serde_json::to_string(&status_record).map_err(|err| {
        ApiError::internal(format!(
            "failed serializing bootstrap completion status payload: {err}"
        ))
    })?;
    append_context_jsonl_line(
        &snapshot.source_dir,
        BOOTSTRAP_STATUS_FILE_PATH,
        status_payload.as_str(),
        state.agent_context_loader.max_file_bytes,
    )?;

    Ok((
        StatusCode::OK,
        Json(AgentBootstrapCompleteResponse {
            tenant_id,
            agent_id,
            status: "completed".to_string(),
            source_dir: snapshot.source_dir.display().to_string(),
            completed_at,
            completed_by_user_id: actor_user_id,
            completion_note,
            force: req.force,
            updated_files: written_files,
            status_record_relative_path: BOOTSTRAP_STATUS_FILE_PATH.to_string(),
        }),
    ))
}

async fn compile_agent_heartbeat_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<CompileHeartbeatRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let (
        heartbeat_markdown,
        source,
        source_path,
        context_aggregate_sha256,
        context_summary_digest_sha256,
    ) = if let Some(inline) = req
        .heartbeat_markdown
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        (inline.to_string(), "inline".to_string(), None, None, None)
    } else {
        let snapshot =
            load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
                .map_err(map_agent_context_load_error)?;
        let heartbeat = snapshot
            .required_file_content("HEARTBEAT.md")
            .ok_or_else(|| {
                ApiError::bad_request(
                    "HEARTBEAT.md is missing; provide heartbeat_markdown inline or add the file",
                )
            })?
            .to_string();
        let summary_digest = snapshot.summary_digest_sha256().map_err(|err| {
            ApiError::internal(format!(
                "failed serializing agent context summary for heartbeat compile: {err}"
            ))
        })?;
        (
            heartbeat,
            "context_file".to_string(),
            Some(format!("{}/HEARTBEAT.md", snapshot.source_dir.display())),
            Some(snapshot.aggregate_sha256()),
            Some(summary_digest),
        )
    };

    let report = compile_agent_heartbeat_markdown(heartbeat_markdown.as_str());
    Ok((
        StatusCode::OK,
        Json(AgentHeartbeatCompileResponse {
            tenant_id,
            agent_id,
            source,
            source_path,
            context_aggregate_sha256,
            context_summary_digest_sha256,
            candidate_count: report.candidates.len(),
            issue_count: report.issues.len(),
            candidates: report.candidates,
            issues: report.issues,
        }),
    ))
}

async fn find_existing_heartbeat_trigger_id_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
    agent_id: Uuid,
    candidate: &agent_core::HeartbeatTriggerCandidate,
) -> ApiResult<Option<Uuid>> {
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let raw: Option<String> = match candidate.kind {
        agent_core::HeartbeatIntentKind::Interval => {
            let interval_seconds = candidate.interval_seconds.ok_or_else(|| {
                ApiError::internal("heartbeat interval candidate missing interval seconds")
            })?;
            sqlx::query_scalar(
                r#"
                SELECT id
                FROM triggers
                WHERE tenant_id = ?1
                  AND agent_id = ?2
                  AND trigger_type = 'interval'
                  AND recipe_id = ?3
                  AND interval_seconds = ?4
                  AND max_inflight_runs = ?5
                  AND jitter_seconds = ?6
                ORDER BY datetime(created_at) DESC, id DESC
                LIMIT 1
                "#,
            )
            .bind(tenant_id)
            .bind(agent_id.to_string())
            .bind(candidate.recipe_id.as_str())
            .bind(interval_seconds)
            .bind(candidate.max_inflight_runs)
            .bind(candidate.jitter_seconds)
            .fetch_optional(sqlite)
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed checking existing interval heartbeat trigger: {err}"
                ))
            })?
        }
        agent_core::HeartbeatIntentKind::Cron => {
            let cron_expression = candidate.cron_expression.as_deref().ok_or_else(|| {
                ApiError::internal("heartbeat cron candidate missing cron expression")
            })?;
            let timezone = candidate.timezone.as_deref().unwrap_or("UTC");
            sqlx::query_scalar(
                r#"
                SELECT id
                FROM triggers
                WHERE tenant_id = ?1
                  AND agent_id = ?2
                  AND trigger_type = 'cron'
                  AND recipe_id = ?3
                  AND cron_expression = ?4
                  AND schedule_timezone = ?5
                  AND max_inflight_runs = ?6
                  AND jitter_seconds = ?7
                ORDER BY datetime(created_at) DESC, id DESC
                LIMIT 1
                "#,
            )
            .bind(tenant_id)
            .bind(agent_id.to_string())
            .bind(candidate.recipe_id.as_str())
            .bind(cron_expression)
            .bind(timezone)
            .bind(candidate.max_inflight_runs)
            .bind(candidate.jitter_seconds)
            .fetch_optional(sqlite)
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed checking existing cron heartbeat trigger: {err}"
                ))
            })?
        }
    };

    raw.map(|value| {
        Uuid::parse_str(value.as_str()).map_err(|err| {
            ApiError::internal(format!(
                "invalid trigger id returned from sqlite heartbeat lookup: {err} (value={value})"
            ))
        })
    })
    .transpose()
}

async fn create_heartbeat_trigger_sqlite(
    state: &SqliteAppState,
    tenant_id: &str,
    agent_id: Uuid,
    triggered_by_user_id: Option<Uuid>,
    candidate: &agent_core::HeartbeatTriggerCandidate,
    input_json: &Value,
    requested_capabilities: &Value,
    granted_capabilities: &Value,
    cron_max_attempts: i32,
) -> ApiResult<Uuid> {
    let trigger_id = Uuid::new_v4();
    let now = OffsetDateTime::now_utc();
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;

    match candidate.kind {
        agent_core::HeartbeatIntentKind::Interval => {
            let interval_seconds = candidate.interval_seconds.ok_or_else(|| {
                ApiError::internal("heartbeat interval candidate missing interval seconds")
            })?;
            let next_fire_at = now + time::Duration::seconds(interval_seconds);
            sqlx::query(
                r#"
                INSERT INTO triggers (
                    id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
                    interval_seconds, schedule_timezone, misfire_policy, max_attempts, max_inflight_runs,
                    jitter_seconds, input_json, requested_capabilities, granted_capabilities, next_fire_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', 'interval', ?6, 'UTC', 'fire_now', 3, ?7, ?8, ?9, ?10, ?11, ?12)
                "#,
            )
            .bind(trigger_id.to_string())
            .bind(tenant_id)
            .bind(agent_id.to_string())
            .bind(triggered_by_user_id.map(|id| id.to_string()))
            .bind(candidate.recipe_id.as_str())
            .bind(interval_seconds)
            .bind(candidate.max_inflight_runs)
            .bind(candidate.jitter_seconds)
            .bind(input_json.to_string())
            .bind(requested_capabilities.to_string())
            .bind(granted_capabilities.to_string())
            .bind(next_fire_at.format(&Rfc3339).map_err(|err| {
                ApiError::internal(format!("failed formatting heartbeat interval next_fire_at: {err}"))
            })?)
            .execute(sqlite)
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed creating interval trigger from heartbeat candidate: {err}"
                ))
            })?;
        }
        agent_core::HeartbeatIntentKind::Cron => {
            let cron_expression = candidate.cron_expression.as_deref().ok_or_else(|| {
                ApiError::internal("heartbeat cron candidate missing cron expression")
            })?;
            let timezone = candidate.timezone.as_deref().unwrap_or("UTC");
            sqlx::query(
                r#"
                INSERT INTO triggers (
                    id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
                    cron_expression, schedule_timezone, misfire_policy, max_attempts, max_inflight_runs,
                    jitter_seconds, input_json, requested_capabilities, granted_capabilities, next_fire_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, 'enabled', 'cron', ?6, ?7, 'fire_now', ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                "#,
            )
            .bind(trigger_id.to_string())
            .bind(tenant_id)
            .bind(agent_id.to_string())
            .bind(triggered_by_user_id.map(|id| id.to_string()))
            .bind(candidate.recipe_id.as_str())
            .bind(cron_expression)
            .bind(timezone)
            .bind(cron_max_attempts)
            .bind(candidate.max_inflight_runs)
            .bind(candidate.jitter_seconds)
            .bind(input_json.to_string())
            .bind(requested_capabilities.to_string())
            .bind(granted_capabilities.to_string())
            .bind(now.format(&Rfc3339).map_err(|err| {
                ApiError::internal(format!("failed formatting heartbeat cron next_fire_at: {err}"))
            })?)
            .execute(sqlite)
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed creating cron trigger from heartbeat candidate: {err}"
                ))
            })?;
        }
    }

    Ok(trigger_id)
}

async fn materialize_agent_heartbeat_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<MaterializeHeartbeatRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let actor_user_id = user_id_from_headers_sqlite(&state, &headers)?;

    if req.apply && !req.approval_confirmed {
        return Err(ApiError::forbidden(
            "heartbeat materialization requires approval_confirmed=true",
        ));
    }
    if req.apply && actor_user_id.is_none() {
        return Err(ApiError::forbidden(
            "heartbeat materialization requires x-user-id for approval attribution",
        ));
    }

    let cron_max_attempts = req.cron_max_attempts.unwrap_or(3);
    if !(1..=20).contains(&cron_max_attempts) {
        return Err(ApiError::bad_request(
            "cron_max_attempts must be between 1 and 20",
        ));
    }

    let requested_capabilities =
        normalize_requested_capabilities_payload(req.requested_capabilities)?;
    let input_json = normalize_materialization_input_payload(req.input);
    let effective_triggered_by_user_id = if req.apply {
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?
    } else {
        None
    };

    let (
        heartbeat_markdown,
        source,
        source_path,
        context_aggregate_sha256,
        context_summary_digest_sha256,
    ) = if let Some(inline) = req
        .heartbeat_markdown
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        (inline.to_string(), "inline".to_string(), None, None, None)
    } else {
        let snapshot =
            load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
                .map_err(map_agent_context_load_error)?;
        let heartbeat = snapshot
            .required_file_content("HEARTBEAT.md")
            .ok_or_else(|| {
                ApiError::bad_request(
                    "HEARTBEAT.md is missing; provide heartbeat_markdown inline or add the file",
                )
            })?
            .to_string();
        let summary_digest = snapshot.summary_digest_sha256().map_err(|err| {
            ApiError::internal(format!(
                "failed serializing agent context summary for heartbeat materialization: {err}"
            ))
        })?;
        (
            heartbeat,
            "context_file".to_string(),
            Some(format!("{}/HEARTBEAT.md", snapshot.source_dir.display())),
            Some(snapshot.aggregate_sha256()),
            Some(summary_digest),
        )
    };

    let report = compile_agent_heartbeat_markdown(heartbeat_markdown.as_str());
    if req.apply && !report.issues.is_empty() {
        return Err(ApiError::conflict(
            "heartbeat compile produced issues; resolve issues before apply",
        ));
    }

    let mut planned_count = 0usize;
    let mut created_count = 0usize;
    let mut existing_count = 0usize;
    let mut candidates = Vec::with_capacity(report.candidates.len());

    for candidate in &report.candidates {
        let kind = match candidate.kind {
            agent_core::HeartbeatIntentKind::Interval => "interval",
            agent_core::HeartbeatIntentKind::Cron => "cron",
        }
        .to_string();
        let mut status = "planned".to_string();
        let mut trigger_id = None;

        if req.apply {
            if let Some(existing_id) = find_existing_heartbeat_trigger_id_sqlite(
                &state,
                tenant_id.as_str(),
                agent_id,
                candidate,
            )
            .await?
            {
                status = "existing".to_string();
                trigger_id = Some(existing_id);
                existing_count += 1;
            } else {
                ensure_tenant_trigger_capacity_sqlite(&state, tenant_id.as_str()).await?;
                let granted_capabilities = resolve_granted_capabilities(
                    candidate.recipe_id.as_str(),
                    role_preset,
                    &requested_capabilities,
                )?;
                let created_id = create_heartbeat_trigger_sqlite(
                    &state,
                    tenant_id.as_str(),
                    agent_id,
                    effective_triggered_by_user_id,
                    candidate,
                    &input_json,
                    &requested_capabilities,
                    &granted_capabilities,
                    cron_max_attempts,
                )
                .await?;

                append_trigger_audit_sqlite(
                    &state,
                    &tenant_id,
                    created_id,
                    role_preset,
                    "trigger.materialized",
                    json!({
                        "source": source,
                        "line": candidate.line,
                        "source_line": candidate.source_line,
                        "recipe_id": candidate.recipe_id,
                        "approval_confirmed": req.approval_confirmed,
                        "approval_note": req.approval_note,
                        "approved_by_user_id": actor_user_id,
                        "cron_max_attempts": cron_max_attempts,
                    }),
                )
                .await?;

                status = "created".to_string();
                trigger_id = Some(created_id);
                created_count += 1;
            }
        } else {
            planned_count += 1;
        }

        candidates.push(AgentHeartbeatMaterializeItemResponse {
            line: candidate.line,
            kind,
            recipe_id: candidate.recipe_id.clone(),
            interval_seconds: candidate.interval_seconds,
            cron_expression: candidate.cron_expression.clone(),
            timezone: candidate.timezone.clone(),
            max_inflight_runs: candidate.max_inflight_runs,
            jitter_seconds: candidate.jitter_seconds,
            status,
            trigger_id,
        });
    }

    Ok((
        StatusCode::OK,
        Json(AgentHeartbeatMaterializeResponse {
            tenant_id,
            agent_id,
            source,
            source_path,
            context_aggregate_sha256,
            context_summary_digest_sha256,
            apply_requested: req.apply,
            approval_confirmed: req.approval_confirmed,
            approval_note: req.approval_note,
            approved_by_user_id: actor_user_id,
            cron_max_attempts,
            candidate_count: report.candidates.len(),
            issue_count: report.issues.len(),
            planned_count,
            created_count,
            existing_count,
            candidates,
            issues: report.issues,
        }),
    ))
}

async fn mutate_agent_context_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<MutateAgentContextRequest>,
) -> ApiResult<impl IntoResponse> {
    if !state.agent_context_mutation_enabled {
        return Err(ApiError::forbidden(
            "agent-context mutation endpoints are disabled by API_AGENT_CONTEXT_MUTATION_ENABLED",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    let mode = parse_context_mutation_mode(req.mode.as_str())?;
    let relative_path = normalize_context_mutation_path(req.relative_path.as_str())?;
    let mutability = classify_agent_context_mutability(relative_path.as_str()).ok_or_else(|| {
        ApiError::bad_request(
            "relative_path must target a supported agent-context file (USER.md, HEARTBEAT.md, BOOTSTRAP.md, MEMORY.md, memory/*.md, sessions/*.jsonl)",
        )
    })?;
    ensure_context_mutation_role(role_preset, mutability)?;
    validate_context_mutation_mode(mode, relative_path.as_str(), mutability)?;

    let source_dir = resolve_or_create_agent_context_source_dir(
        &state.agent_context_loader,
        &tenant_id,
        agent_id,
    )?;
    let full_path = source_dir.join(relative_path.as_str());
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ApiError::internal(format!(
                "failed creating context file directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let mut payload = req.content;
    if matches!(mode, ContextMutationMode::Append)
        && relative_path.starts_with("sessions/")
        && !payload.ends_with('\n')
    {
        payload.push('\n');
    }

    let current_len = fs::metadata(&full_path)
        .map(|meta| meta.len() as usize)
        .unwrap_or(0usize);
    let projected_len = match mode {
        ContextMutationMode::Replace => payload.as_bytes().len(),
        ContextMutationMode::Append => current_len.saturating_add(payload.as_bytes().len()),
    };
    if projected_len > state.agent_context_loader.max_file_bytes {
        return Err(ApiError::bad_request(format!(
            "mutation exceeds max file size ({} bytes)",
            state.agent_context_loader.max_file_bytes
        )));
    }

    match mode {
        ContextMutationMode::Replace => {
            fs::write(&full_path, payload.as_bytes()).map_err(|err| {
                ApiError::internal(format!(
                    "failed writing context file {}: {err}",
                    full_path.display()
                ))
            })?;
        }
        ContextMutationMode::Append => {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full_path)
                .map_err(|err| {
                    ApiError::internal(format!(
                        "failed opening context file {} for append: {err}",
                        full_path.display()
                    ))
                })?;
            file.write_all(payload.as_bytes()).map_err(|err| {
                ApiError::internal(format!(
                    "failed appending context file {}: {err}",
                    full_path.display()
                ))
            })?;
        }
    }

    let bytes = fs::read(&full_path).map_err(|err| {
        ApiError::internal(format!(
            "failed reading updated context file {}: {err}",
            full_path.display()
        ))
    })?;
    let sha256 = format!("{:x}", Sha256::digest(bytes.as_slice()));

    Ok((
        StatusCode::OK,
        Json(AgentContextMutationResponse {
            tenant_id,
            agent_id,
            relative_path,
            mode: mode.as_str().to_string(),
            mutability,
            sha256,
            bytes: bytes.len(),
        }),
    ))
}

async fn get_agent_context_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let snapshot = load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
        .map_err(map_agent_context_load_error)?;
    let summary_digest_sha256 = snapshot.summary_digest_sha256().map_err(|err| {
        ApiError::internal(format!("failed serializing agent context summary: {err}"))
    })?;
    let aggregate_sha256 = snapshot.aggregate_sha256();

    Ok((
        StatusCode::OK,
        Json(AgentContextInspectResponse {
            tenant_id,
            agent_id,
            source_dir: snapshot.source_dir.display().to_string(),
            loaded_at: snapshot.loaded_at,
            loaded_file_count: snapshot.loaded_file_count(),
            total_loaded_bytes: snapshot.total_loaded_bytes(),
            aggregate_sha256,
            summary_digest_sha256,
            missing_required_files: snapshot.missing_required_files.clone(),
            warnings: snapshot.warnings.clone(),
            precedence_order: agent_context_precedence_order(),
            required_files: snapshot
                .required_files
                .iter()
                .map(agent_context_file_to_response)
                .collect(),
            memory_files: snapshot
                .memory_files
                .iter()
                .map(agent_context_file_to_response)
                .collect(),
            session_files: snapshot
                .session_files
                .iter()
                .map(agent_context_file_to_response)
                .collect(),
        }),
    ))
}

async fn get_agent_bootstrap_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    if !state.agent_bootstrap_enabled {
        return Ok((
            StatusCode::OK,
            Json(AgentBootstrapInspectResponse {
                tenant_id,
                agent_id,
                enabled: false,
                status: "disabled".to_string(),
                source_dir: "".to_string(),
                bootstrap_present: false,
                bootstrap_path: None,
                bootstrap_sha256: None,
                bootstrap_bytes: None,
                bootstrap_markdown: None,
                completed_at: None,
                completed_by_user_id: None,
                completion_note: None,
                updated_files: Vec::new(),
            }),
        ));
    }

    let snapshot = load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
        .map_err(map_agent_context_load_error)?;
    let bootstrap_path = snapshot.source_dir.join(BOOTSTRAP_FILE_NAME);
    let bootstrap_bytes = fs::read(&bootstrap_path).ok();
    let bootstrap_markdown = bootstrap_bytes
        .as_deref()
        .and_then(|value| String::from_utf8(value.to_vec()).ok());
    let bootstrap_sha256 = bootstrap_bytes
        .as_deref()
        .map(|value| format!("{:x}", Sha256::digest(value)));
    let bootstrap_size = bootstrap_bytes.as_ref().map(Vec::len);
    let completion = find_latest_bootstrap_completion(&snapshot.session_files)?;
    let status = if completion.is_some() {
        "completed".to_string()
    } else if bootstrap_markdown.is_some() {
        "pending".to_string()
    } else {
        "not_configured".to_string()
    };

    Ok((
        StatusCode::OK,
        Json(AgentBootstrapInspectResponse {
            tenant_id,
            agent_id,
            enabled: true,
            status,
            source_dir: snapshot.source_dir.display().to_string(),
            bootstrap_present: bootstrap_markdown.is_some(),
            bootstrap_path: bootstrap_markdown
                .as_ref()
                .map(|_| bootstrap_path.display().to_string()),
            bootstrap_sha256,
            bootstrap_bytes: bootstrap_size,
            bootstrap_markdown,
            completed_at: completion.as_ref().map(|entry| entry.completed_at),
            completed_by_user_id: completion.as_ref().map(|entry| entry.completed_by_user_id),
            completion_note: completion
                .as_ref()
                .and_then(|entry| entry.completion_note.clone()),
            updated_files: completion
                .map(|entry| entry.updated_files)
                .unwrap_or_default(),
        }),
    ))
}

async fn complete_agent_bootstrap_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<CompleteBootstrapRequest>,
) -> ApiResult<impl IntoResponse> {
    if !state.agent_bootstrap_enabled {
        return Err(ApiError::forbidden(
            "agent bootstrap is disabled by API_AGENT_BOOTSTRAP_ENABLED",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_owner_role(
        role_preset,
        "only owner role can complete agent bootstrap workflow",
    )?;
    let actor_user_id = user_id_from_headers(&state, &headers)?.ok_or_else(|| {
        ApiError::forbidden("bootstrap completion requires x-user-id for attribution")
    })?;
    let snapshot = load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
        .map_err(map_agent_context_load_error)?;

    let bootstrap_path = snapshot.source_dir.join(BOOTSTRAP_FILE_NAME);
    if !bootstrap_path.is_file() {
        return Err(ApiError::conflict(
            "BOOTSTRAP.md is missing; add the file before completing bootstrap",
        ));
    }
    let existing_completion = find_latest_bootstrap_completion(&snapshot.session_files)?;
    if existing_completion.is_some() && !req.force {
        return Err(ApiError::conflict(
            "bootstrap is already completed; pass force=true to record a new completion event",
        ));
    }

    let mut updates = Vec::new();
    if let Some(content) = normalize_optional_text(req.identity_markdown.as_deref()) {
        updates.push(("IDENTITY.md".to_string(), content.to_string()));
    }
    if let Some(content) = normalize_optional_text(req.soul_markdown.as_deref()) {
        updates.push(("SOUL.md".to_string(), content.to_string()));
    }
    if let Some(content) = normalize_optional_text(req.user_markdown.as_deref()) {
        updates.push(("USER.md".to_string(), content.to_string()));
    }
    if let Some(content) = normalize_optional_text(req.heartbeat_markdown.as_deref()) {
        updates.push(("HEARTBEAT.md".to_string(), content.to_string()));
    }

    let mut written_files = Vec::with_capacity(updates.len());
    for (relative_path, content) in updates {
        let full_path = snapshot.source_dir.join(relative_path.as_str());
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                ApiError::internal(format!(
                    "failed creating context file directory {}: {err}",
                    parent.display()
                ))
            })?;
        }
        if content.as_bytes().len() > state.agent_context_loader.max_file_bytes {
            return Err(ApiError::bad_request(format!(
                "{} exceeds max file size ({} bytes)",
                relative_path, state.agent_context_loader.max_file_bytes
            )));
        }
        fs::write(&full_path, content.as_bytes()).map_err(|err| {
            ApiError::internal(format!(
                "failed writing bootstrap target {}: {err}",
                full_path.display()
            ))
        })?;
        let bytes = fs::read(&full_path).map_err(|err| {
            ApiError::internal(format!(
                "failed reading bootstrap target {}: {err}",
                full_path.display()
            ))
        })?;
        written_files.push(BootstrapFileWriteResponse {
            relative_path,
            sha256: format!("{:x}", Sha256::digest(bytes.as_slice())),
            bytes: bytes.len(),
        });
    }

    let completed_at = OffsetDateTime::now_utc();
    let completion_note =
        normalize_optional_text(req.completion_note.as_deref()).map(str::to_string);
    let completed_at_rfc3339 = completed_at
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|err| {
            ApiError::internal(format!(
                "failed formatting bootstrap completion timestamp: {err}"
            ))
        })?;
    let status_record = json!({
        "schema_version": "v1",
        "status": "completed",
        "completed_at": completed_at_rfc3339,
        "completed_by_user_id": actor_user_id,
        "completion_note": completion_note,
        "updated_files": written_files.iter().map(|item| item.relative_path.clone()).collect::<Vec<_>>(),
    });
    let status_payload = serde_json::to_string(&status_record).map_err(|err| {
        ApiError::internal(format!(
            "failed serializing bootstrap completion status payload: {err}"
        ))
    })?;
    append_context_jsonl_line(
        &snapshot.source_dir,
        BOOTSTRAP_STATUS_FILE_PATH,
        status_payload.as_str(),
        state.agent_context_loader.max_file_bytes,
    )?;

    Ok((
        StatusCode::OK,
        Json(AgentBootstrapCompleteResponse {
            tenant_id,
            agent_id,
            status: "completed".to_string(),
            source_dir: snapshot.source_dir.display().to_string(),
            completed_at,
            completed_by_user_id: actor_user_id,
            completion_note,
            force: req.force,
            updated_files: written_files,
            status_record_relative_path: BOOTSTRAP_STATUS_FILE_PATH.to_string(),
        }),
    ))
}

async fn compile_agent_heartbeat_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<CompileHeartbeatRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let (
        heartbeat_markdown,
        source,
        source_path,
        context_aggregate_sha256,
        context_summary_digest_sha256,
    ) = if let Some(inline) = req
        .heartbeat_markdown
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        (inline.to_string(), "inline".to_string(), None, None, None)
    } else {
        let snapshot =
            load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
                .map_err(map_agent_context_load_error)?;
        let heartbeat = snapshot
            .required_file_content("HEARTBEAT.md")
            .ok_or_else(|| {
                ApiError::bad_request(
                    "HEARTBEAT.md is missing; provide heartbeat_markdown inline or add the file",
                )
            })?
            .to_string();
        let summary_digest = snapshot.summary_digest_sha256().map_err(|err| {
            ApiError::internal(format!(
                "failed serializing agent context summary for heartbeat compile: {err}"
            ))
        })?;
        (
            heartbeat,
            "context_file".to_string(),
            Some(format!("{}/HEARTBEAT.md", snapshot.source_dir.display())),
            Some(snapshot.aggregate_sha256()),
            Some(summary_digest),
        )
    };

    let report = compile_agent_heartbeat_markdown(heartbeat_markdown.as_str());
    Ok((
        StatusCode::OK,
        Json(AgentHeartbeatCompileResponse {
            tenant_id,
            agent_id,
            source,
            source_path,
            context_aggregate_sha256,
            context_summary_digest_sha256,
            candidate_count: report.candidates.len(),
            issue_count: report.issues.len(),
            candidates: report.candidates,
            issues: report.issues,
        }),
    ))
}

async fn materialize_agent_heartbeat_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<MaterializeHeartbeatRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;

    if req.apply && !req.approval_confirmed {
        return Err(ApiError::forbidden(
            "heartbeat materialization requires approval_confirmed=true",
        ));
    }
    if req.apply && actor_user_id.is_none() {
        return Err(ApiError::forbidden(
            "heartbeat materialization requires x-user-id for approval attribution",
        ));
    }

    let cron_max_attempts = req.cron_max_attempts.unwrap_or(3);
    if !(1..=20).contains(&cron_max_attempts) {
        return Err(ApiError::bad_request(
            "cron_max_attempts must be between 1 and 20",
        ));
    }

    let requested_capabilities =
        normalize_requested_capabilities_payload(req.requested_capabilities)?;
    let input_json = normalize_materialization_input_payload(req.input);
    let effective_triggered_by_user_id = if req.apply {
        resolve_trigger_actor_for_create(role_preset, actor_user_id, req.triggered_by_user_id)?
    } else {
        None
    };

    let (
        heartbeat_markdown,
        source,
        source_path,
        context_aggregate_sha256,
        context_summary_digest_sha256,
    ) = if let Some(inline) = req
        .heartbeat_markdown
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        (inline.to_string(), "inline".to_string(), None, None, None)
    } else {
        let snapshot =
            load_agent_context_snapshot(&state.agent_context_loader, &tenant_id, agent_id)
                .map_err(map_agent_context_load_error)?;
        let heartbeat = snapshot
            .required_file_content("HEARTBEAT.md")
            .ok_or_else(|| {
                ApiError::bad_request(
                    "HEARTBEAT.md is missing; provide heartbeat_markdown inline or add the file",
                )
            })?
            .to_string();
        let summary_digest = snapshot.summary_digest_sha256().map_err(|err| {
            ApiError::internal(format!(
                "failed serializing agent context summary for heartbeat materialization: {err}"
            ))
        })?;
        (
            heartbeat,
            "context_file".to_string(),
            Some(format!("{}/HEARTBEAT.md", snapshot.source_dir.display())),
            Some(snapshot.aggregate_sha256()),
            Some(summary_digest),
        )
    };

    let report = compile_agent_heartbeat_markdown(heartbeat_markdown.as_str());
    if req.apply && !report.issues.is_empty() {
        return Err(ApiError::conflict(
            "heartbeat compile produced issues; resolve issues before apply",
        ));
    }

    let mut planned_count = 0usize;
    let mut created_count = 0usize;
    let mut existing_count = 0usize;
    let mut candidates = Vec::with_capacity(report.candidates.len());

    for candidate in &report.candidates {
        let kind = match candidate.kind {
            agent_core::HeartbeatIntentKind::Interval => "interval",
            agent_core::HeartbeatIntentKind::Cron => "cron",
        }
        .to_string();
        let mut status = "planned".to_string();
        let mut trigger_id = None;

        if req.apply {
            if let Some(existing_id) = find_existing_heartbeat_trigger_id(
                &state.pool,
                tenant_id.as_str(),
                agent_id,
                candidate,
            )
            .await?
            {
                status = "existing".to_string();
                trigger_id = Some(existing_id);
                existing_count += 1;
            } else {
                ensure_tenant_trigger_capacity(&state, tenant_id.as_str()).await?;
                let granted_capabilities = resolve_granted_capabilities(
                    candidate.recipe_id.as_str(),
                    role_preset,
                    &requested_capabilities,
                )?;
                let created = match candidate.kind {
                    agent_core::HeartbeatIntentKind::Interval => {
                        let interval_seconds = candidate.interval_seconds.ok_or_else(|| {
                            ApiError::internal(
                                "heartbeat interval candidate missing interval seconds",
                            )
                        })?;
                        create_interval_trigger(
                            &state.pool,
                            &NewIntervalTrigger {
                                id: Uuid::new_v4(),
                                tenant_id: tenant_id.clone(),
                                agent_id,
                                triggered_by_user_id: effective_triggered_by_user_id,
                                recipe_id: candidate.recipe_id.clone(),
                                interval_seconds,
                                input_json: input_json.clone(),
                                requested_capabilities: requested_capabilities.clone(),
                                granted_capabilities,
                                next_fire_at: OffsetDateTime::now_utc()
                                    + time::Duration::seconds(interval_seconds),
                                status: "enabled".to_string(),
                                misfire_policy: "fire_now".to_string(),
                                max_attempts: 3,
                                max_inflight_runs: candidate.max_inflight_runs,
                                jitter_seconds: candidate.jitter_seconds,
                                webhook_secret_ref: None,
                            },
                        )
                        .await
                        .map_err(|err| {
                            ApiError::internal(format!(
                                "failed creating interval trigger from heartbeat line {}: {err}",
                                candidate.line
                            ))
                        })?
                    }
                    agent_core::HeartbeatIntentKind::Cron => create_cron_trigger(
                        &state.pool,
                        &NewCronTrigger {
                            id: Uuid::new_v4(),
                            tenant_id: tenant_id.clone(),
                            agent_id,
                            triggered_by_user_id: effective_triggered_by_user_id,
                            recipe_id: candidate.recipe_id.clone(),
                            cron_expression: candidate.cron_expression.clone().ok_or_else(
                                || {
                                    ApiError::internal(
                                        "heartbeat cron candidate missing cron expression",
                                    )
                                },
                            )?,
                            schedule_timezone: candidate
                                .timezone
                                .clone()
                                .unwrap_or_else(|| "UTC".to_string()),
                            input_json: input_json.clone(),
                            requested_capabilities: requested_capabilities.clone(),
                            granted_capabilities,
                            status: "enabled".to_string(),
                            misfire_policy: "fire_now".to_string(),
                            max_attempts: cron_max_attempts,
                            max_inflight_runs: candidate.max_inflight_runs,
                            jitter_seconds: candidate.jitter_seconds,
                        },
                    )
                    .await
                    .map_err(|err| {
                        ApiError::internal(format!(
                            "failed creating cron trigger from heartbeat line {}: {err}",
                            candidate.line
                        ))
                    })?,
                };

                append_trigger_audit(
                    &state.pool,
                    &tenant_id,
                    created.id,
                    role_preset,
                    "trigger.materialized",
                    json!({
                        "source": source,
                        "line": candidate.line,
                        "source_line": candidate.source_line,
                        "recipe_id": candidate.recipe_id,
                        "approval_confirmed": req.approval_confirmed,
                        "approval_note": req.approval_note,
                        "approved_by_user_id": actor_user_id,
                        "cron_max_attempts": cron_max_attempts,
                    }),
                )
                .await?;

                status = "created".to_string();
                trigger_id = Some(created.id);
                created_count += 1;
            }
        } else {
            planned_count += 1;
        }

        candidates.push(AgentHeartbeatMaterializeItemResponse {
            line: candidate.line,
            kind,
            recipe_id: candidate.recipe_id.clone(),
            interval_seconds: candidate.interval_seconds,
            cron_expression: candidate.cron_expression.clone(),
            timezone: candidate.timezone.clone(),
            max_inflight_runs: candidate.max_inflight_runs,
            jitter_seconds: candidate.jitter_seconds,
            status,
            trigger_id,
        });
    }

    Ok((
        StatusCode::OK,
        Json(AgentHeartbeatMaterializeResponse {
            tenant_id,
            agent_id,
            source,
            source_path,
            context_aggregate_sha256,
            context_summary_digest_sha256,
            apply_requested: req.apply,
            approval_confirmed: req.approval_confirmed,
            approval_note: req.approval_note,
            approved_by_user_id: actor_user_id,
            cron_max_attempts,
            candidate_count: report.candidates.len(),
            issue_count: report.issues.len(),
            planned_count,
            created_count,
            existing_count,
            candidates,
            issues: report.issues,
        }),
    ))
}

async fn mutate_agent_context_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<MutateAgentContextRequest>,
) -> ApiResult<impl IntoResponse> {
    if !state.agent_context_mutation_enabled {
        return Err(ApiError::forbidden(
            "agent-context mutation endpoints are disabled by API_AGENT_CONTEXT_MUTATION_ENABLED",
        ));
    }

    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let mode = parse_context_mutation_mode(req.mode.as_str())?;
    let relative_path = normalize_context_mutation_path(req.relative_path.as_str())?;
    let mutability = classify_agent_context_mutability(relative_path.as_str()).ok_or_else(|| {
        ApiError::bad_request(
            "relative_path must target a supported agent-context file (USER.md, HEARTBEAT.md, BOOTSTRAP.md, MEMORY.md, memory/*.md, sessions/*.jsonl)",
        )
    })?;
    ensure_context_mutation_role(role_preset, mutability)?;
    validate_context_mutation_mode(mode, relative_path.as_str(), mutability)?;

    let source_dir = resolve_or_create_agent_context_source_dir(
        &state.agent_context_loader,
        &tenant_id,
        agent_id,
    )?;
    let full_path = source_dir.join(relative_path.as_str());
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ApiError::internal(format!(
                "failed creating context file directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let mut payload = req.content;
    if matches!(mode, ContextMutationMode::Append)
        && relative_path.starts_with("sessions/")
        && !payload.ends_with('\n')
    {
        payload.push('\n');
    }

    let current_len = fs::metadata(&full_path)
        .map(|meta| meta.len() as usize)
        .unwrap_or(0usize);
    let projected_len = match mode {
        ContextMutationMode::Replace => payload.as_bytes().len(),
        ContextMutationMode::Append => current_len.saturating_add(payload.as_bytes().len()),
    };
    if projected_len > state.agent_context_loader.max_file_bytes {
        return Err(ApiError::bad_request(format!(
            "mutation exceeds max file size ({} bytes)",
            state.agent_context_loader.max_file_bytes
        )));
    }

    match mode {
        ContextMutationMode::Replace => {
            fs::write(&full_path, payload.as_bytes()).map_err(|err| {
                ApiError::internal(format!(
                    "failed writing context file {}: {err}",
                    full_path.display()
                ))
            })?;
        }
        ContextMutationMode::Append => {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full_path)
                .map_err(|err| {
                    ApiError::internal(format!(
                        "failed opening context file {} for append: {err}",
                        full_path.display()
                    ))
                })?;
            file.write_all(payload.as_bytes()).map_err(|err| {
                ApiError::internal(format!(
                    "failed appending context file {}: {err}",
                    full_path.display()
                ))
            })?;
        }
    }

    let bytes = fs::read(&full_path).map_err(|err| {
        ApiError::internal(format!(
            "failed reading updated context file {}: {err}",
            full_path.display()
        ))
    })?;
    let sha256 = format!("{:x}", Sha256::digest(bytes.as_slice()));

    Ok((
        StatusCode::OK,
        Json(AgentContextMutationResponse {
            tenant_id,
            agent_id,
            relative_path,
            mode: mode.as_str().to_string(),
            mutability,
            sha256,
            bytes: bytes.len(),
        }),
    ))
}

async fn create_memory_record_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateMemoryRecordRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_memory_write_role(role_preset)?;
    ensure_tenant_memory_capacity(&state, tenant_id.as_str()).await?;

    let Some(memory_kind) = normalize_memory_kind(req.memory_kind.as_str()) else {
        return Err(ApiError::bad_request(
            "memory_kind must be one of: session, semantic, procedural, handoff",
        ));
    };
    let scope = req.scope.trim();
    if scope.is_empty() {
        return Err(ApiError::bad_request("scope must not be empty"));
    }
    if !is_scope_allowed_for_capability("memory.write", scope) {
        return Err(ApiError::bad_request(
            "scope must be memory-scoped (prefix `memory:`)",
        ));
    }

    let source = req
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("api")
        .to_string();

    if let Some(run_id) = req.run_id {
        let exists = get_run_status(&state.pool, tenant_id.as_str(), run_id)
            .await
            .map_err(|err| ApiError::internal(format!("failed validating run_id: {err}")))?
            .is_some();
        if !exists {
            return Err(ApiError::bad_request("run_id is not found for this tenant"));
        }
    }

    if let Some(step_id) = req.step_id {
        let Some(run_id) = req.run_id else {
            return Err(ApiError::bad_request(
                "step_id requires run_id for tenant validation",
            ));
        };
        let step_exists = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM steps
            WHERE id = $1
              AND run_id = $2
              AND tenant_id = $3
            "#,
        )
        .bind(step_id)
        .bind(run_id)
        .bind(tenant_id.as_str())
        .fetch_one(&state.pool)
        .await
        .map_err(|err| ApiError::internal(format!("failed validating step_id: {err}")))?;
        if step_exists == 0 {
            return Err(ApiError::bad_request(
                "step_id is not found for this tenant/run",
            ));
        }
    }

    let (redacted_content_json, redacted_summary_text, redaction_auto_applied) =
        redact_memory_content(&req.content_json, req.summary_text.as_deref());
    let redaction_applied = req.redaction_applied.unwrap_or(false) || redaction_auto_applied;

    let created = create_memory_record(
        &state.pool,
        &NewMemoryRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            agent_id: req.agent_id,
            run_id: req.run_id,
            step_id: req.step_id,
            memory_kind: memory_kind.to_string(),
            scope: scope.to_string(),
            content_json: redacted_content_json,
            summary_text: redacted_summary_text,
            source,
            redaction_applied,
            expires_at: req.expires_at,
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed creating memory record: {err}")))?;

    if let Some(run_id) = created.run_id {
        append_audit_event(
            &state.pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: created.step_id,
                tenant_id: tenant_id.clone(),
                agent_id: Some(created.agent_id),
                user_id: None,
                actor: "api".to_string(),
                event_type: "memory.recorded".to_string(),
                payload_json: json!({
                    "memory_id": created.id,
                    "memory_kind": created.memory_kind,
                    "scope": created.scope,
                    "redaction_applied": created.redaction_applied,
                    "expires_at": created.expires_at,
                }),
            },
        )
        .await
        .map_err(|err| ApiError::internal(format!("failed appending memory audit event: {err}")))?;
    }

    Ok((StatusCode::CREATED, Json(memory_to_response(created))))
}

async fn create_handoff_packet_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateHandoffPacketRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_memory_write_role(role_preset)?;
    ensure_tenant_memory_capacity(&state, tenant_id.as_str()).await?;

    let title = req.title.trim();
    if title.is_empty() {
        return Err(ApiError::bad_request("title must not be empty"));
    }

    let source = req
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("api")
        .to_string();

    if let Some(run_id) = req.run_id {
        let exists = get_run_status(&state.pool, tenant_id.as_str(), run_id)
            .await
            .map_err(|err| ApiError::internal(format!("failed validating run_id: {err}")))?
            .is_some();
        if !exists {
            return Err(ApiError::bad_request("run_id is not found for this tenant"));
        }
    }

    if let Some(step_id) = req.step_id {
        let Some(run_id) = req.run_id else {
            return Err(ApiError::bad_request(
                "step_id requires run_id for tenant validation",
            ));
        };
        let step_exists = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)::bigint
            FROM steps
            WHERE id = $1
              AND run_id = $2
              AND tenant_id = $3
            "#,
        )
        .bind(step_id)
        .bind(run_id)
        .bind(tenant_id.as_str())
        .fetch_one(&state.pool)
        .await
        .map_err(|err| ApiError::internal(format!("failed validating step_id: {err}")))?;
        if step_exists == 0 {
            return Err(ApiError::bad_request(
                "step_id is not found for this tenant/run",
            ));
        }
    }

    let from_agent_id = req.from_agent_id.unwrap_or(req.to_agent_id);
    let packet_id = Uuid::new_v4();
    let scope = format!("memory:handoff/{}/{}", req.to_agent_id, packet_id);
    let (redacted_payload_json, redacted_title, redaction_auto_applied) =
        redact_memory_content(&req.payload_json, Some(title));
    let redaction_applied = redaction_auto_applied;
    let title_value = redacted_title.unwrap_or_else(|| title.to_string());

    let created = create_memory_record(
        &state.pool,
        &NewMemoryRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            agent_id: req.to_agent_id,
            run_id: req.run_id,
            step_id: req.step_id,
            memory_kind: "handoff".to_string(),
            scope,
            content_json: json!({
                "packet_id": packet_id,
                "from_agent_id": from_agent_id,
                "to_agent_id": req.to_agent_id,
                "title": title_value,
                "payload_json": redacted_payload_json,
            }),
            summary_text: Some(title_value),
            source,
            redaction_applied,
            expires_at: req.expires_at,
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed creating handoff packet: {err}")))?;

    if let Some(run_id) = created.run_id {
        append_audit_event(
            &state.pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: created.step_id,
                tenant_id: tenant_id.clone(),
                agent_id: Some(created.agent_id),
                user_id: None,
                actor: "api".to_string(),
                event_type: "memory.handoff.recorded".to_string(),
                payload_json: json!({
                    "memory_id": created.id,
                    "packet_id": packet_id,
                    "from_agent_id": from_agent_id,
                    "to_agent_id": req.to_agent_id,
                    "scope": created.scope,
                    "redaction_applied": created.redaction_applied,
                    "expires_at": created.expires_at,
                }),
            },
        )
        .await
        .map_err(|err| {
            ApiError::internal(format!(
                "failed appending handoff memory audit event: {err}"
            ))
        })?;
    }

    Ok((
        StatusCode::CREATED,
        Json(handoff_packet_from_memory_record(created)?),
    ))
}

async fn list_handoff_packets_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HandoffPacketQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let rows = list_tenant_handoff_memory_records(
        &state.pool,
        tenant_id.as_str(),
        query.to_agent_id,
        query.from_agent_id,
        limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed listing handoff packets: {err}")))?;

    let body = rows
        .into_iter()
        .map(handoff_packet_from_memory_record)
        .collect::<ApiResult<Vec<HandoffPacketResponse>>>()?;

    Ok((StatusCode::OK, Json(body)))
}

async fn list_memory_records_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryRecordQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let memory_kind = match query.memory_kind.as_deref() {
        Some(value) => {
            let Some(normalized) = normalize_memory_kind(value) else {
                return Err(ApiError::bad_request(
                    "memory_kind must be one of: session, semantic, procedural, handoff",
                ));
            };
            Some(normalized)
        }
        None => None,
    };
    let scope_prefix = trim_non_empty(query.scope_prefix.as_deref());
    if let Some(prefix) = scope_prefix {
        if !is_scope_allowed_for_capability("memory.read", prefix) {
            return Err(ApiError::bad_request(
                "scope_prefix must use memory scope prefix (`memory:`)",
            ));
        }
    }

    let rows = list_tenant_memory_records(
        &state.pool,
        tenant_id.as_str(),
        query.agent_id,
        memory_kind,
        scope_prefix,
        limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed listing memory records: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(
            rows.into_iter()
                .map(memory_to_response)
                .collect::<Vec<MemoryRecordResponse>>(),
        ),
    ))
}

async fn retrieve_memory_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryRetrieveQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(20).clamp(1, 200);
    let memory_kind = match query.memory_kind.as_deref() {
        Some(value) => {
            let Some(normalized) = normalize_memory_kind(value) else {
                return Err(ApiError::bad_request(
                    "memory_kind must be one of: session, semantic, procedural, handoff",
                ));
            };
            Some(normalized)
        }
        None => None,
    };
    let scope_prefix = trim_non_empty(query.scope_prefix.as_deref());
    if let Some(prefix) = scope_prefix {
        if !is_scope_allowed_for_capability("memory.read", prefix) {
            return Err(ApiError::bad_request(
                "scope_prefix must use memory scope prefix (`memory:`)",
            ));
        }
    }
    let query_text = trim_non_empty(query.query_text.as_deref());
    let min_score = query.min_score;
    if let Some(value) = min_score {
        if !(0.0..=2.0).contains(&value) {
            return Err(ApiError::bad_request(
                "min_score must be between 0.0 and 2.0",
            ));
        }
    }
    let source_prefix = trim_non_empty(query.source_prefix.as_deref());
    let require_summary = query.require_summary.unwrap_or(false);

    let candidate_limit = if query_text.is_some()
        || min_score.is_some()
        || source_prefix.is_some()
        || require_summary
    {
        (limit.saturating_mul(5)).clamp(1, 1000)
    } else {
        limit
    };

    let rows = list_tenant_memory_records(
        &state.pool,
        tenant_id.as_str(),
        query.agent_id,
        memory_kind,
        scope_prefix,
        candidate_limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed retrieving memory records: {err}")))?;

    let filtered_rows = rows
        .into_iter()
        .filter(|row| {
            source_prefix
                .map(|prefix| row.source.starts_with(prefix))
                .unwrap_or(true)
        })
        .filter(|row| {
            !require_summary
                || row
                    .summary_text
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
        })
        .collect::<Vec<_>>();

    let query_tokens = query_text.map(tokenize_retrieval_query).unwrap_or_default();
    let total_candidates = filtered_rows.len();
    let mut scored_rows = filtered_rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let score = compute_memory_retrieval_score(
                &row,
                query_tokens.as_slice(),
                index,
                total_candidates.max(1),
            );
            (row, score)
        })
        .collect::<Vec<_>>();
    scored_rows.sort_by(|(left_row, left_score), (right_row, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right_row.created_at.cmp(&left_row.created_at))
            .then_with(|| right_row.id.cmp(&left_row.id))
    });

    let min_score = min_score.map(|value| value.clamp(0.0, 2.0));
    let items = scored_rows
        .into_iter()
        .filter(|(_, score)| {
            min_score
                .map(|threshold| *score >= threshold)
                .unwrap_or(true)
        })
        .take(limit as usize)
        .enumerate()
        .map(|(index, (row, score))| MemoryRetrievalItem {
            rank: (index + 1) as i64,
            score,
            citation: MemoryCitation {
                memory_id: row.id,
                created_at: row.created_at,
                source: row.source.clone(),
                memory_kind: row.memory_kind.clone(),
                scope: row.scope.clone(),
            },
            content_json: row.content_json,
            summary_text: row.summary_text,
        })
        .collect::<Vec<_>>();

    Ok((
        StatusCode::OK,
        Json(MemoryRetrieveResponse {
            tenant_id,
            limit,
            retrieved_count: items.len() as i64,
            agent_id: query.agent_id,
            memory_kind: memory_kind.map(ToString::to_string),
            scope_prefix: scope_prefix.map(ToString::to_string),
            query_text: query_text.map(ToString::to_string),
            min_score,
            source_prefix: source_prefix.map(ToString::to_string),
            require_summary,
            items,
        }),
    ))
}

async fn get_memory_compaction_stats_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MemoryCompactionStatsQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.map(|value| value.clamp(1, 31_536_000));
    let since = window_secs
        .map(|seconds| OffsetDateTime::now_utc() - time::Duration::seconds(seconds as i64));

    let stats = get_tenant_memory_compaction_stats(&state.pool, tenant_id.as_str(), since)
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed querying memory compaction stats: {err}"))
        })?;

    Ok((
        StatusCode::OK,
        Json(MemoryCompactionStatsResponse {
            tenant_id,
            window_secs,
            since,
            compacted_groups_window: stats.compacted_groups_window,
            compacted_source_records_window: stats.compacted_source_records_window,
            pending_uncompacted_records: stats.pending_uncompacted_records,
            last_compacted_at: stats.last_compacted_at,
        }),
    ))
}

async fn purge_memory_records_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PurgeMemoryRecordsRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_owner_role(role_preset, "only owner can purge memory records")?;

    let as_of = req.as_of.unwrap_or_else(OffsetDateTime::now_utc);
    let run_impact_rows = sqlx::query(
        r#"
        SELECT run_id, COUNT(*)::bigint AS row_count
        FROM memory_records
        WHERE tenant_id = $1
          AND expires_at IS NOT NULL
          AND expires_at <= $2
          AND run_id IS NOT NULL
        GROUP BY run_id
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(as_of)
    .fetch_all(&state.pool)
    .await
    .map_err(|err| ApiError::internal(format!("failed loading memory purge run impacts: {err}")))?;

    let outcome = purge_expired_tenant_memory_records(&state.pool, tenant_id.as_str(), as_of)
        .await
        .map_err(|err| ApiError::internal(format!("failed purging memory records: {err}")))?;

    for row in run_impact_rows {
        let run_id: Uuid = row.get("run_id");
        let run_deleted_count: i64 = row.get("row_count");
        if run_deleted_count <= 0 {
            continue;
        }

        let Some(run) = get_run_status(&state.pool, tenant_id.as_str(), run_id)
            .await
            .map_err(|err| {
                ApiError::internal(format!("failed loading run for memory purge audit: {err}"))
            })?
        else {
            continue;
        };

        append_audit_event(
            &state.pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: None,
                tenant_id: tenant_id.clone(),
                agent_id: Some(run.agent_id),
                user_id: run.triggered_by_user_id,
                actor: "api".to_string(),
                event_type: "memory.purged".to_string(),
                payload_json: json!({
                    "as_of": as_of,
                    "run_deleted_count": run_deleted_count,
                    "tenant_deleted_count": outcome.deleted_count,
                }),
            },
        )
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed appending memory.purged audit event: {err}"))
        })?;
    }

    Ok((
        StatusCode::OK,
        Json(MemoryPurgeResponse {
            tenant_id: outcome.tenant_id,
            deleted_count: outcome.deleted_count,
            as_of: outcome.as_of,
        }),
    ))
}

fn payload_string_field(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn payload_uuid_field(payload: &Value, keys: &[&str]) -> Option<Uuid> {
    payload_string_field(payload, keys).and_then(|value| Uuid::parse_str(value.as_str()).ok())
}

fn compliance_event_to_response(
    event: agent_core::ComplianceAuditEventDetailRecord,
) -> ComplianceAuditEventResponse {
    ComplianceAuditEventResponse {
        id: event.id,
        source_audit_event_id: event.source_audit_event_id,
        tamper_chain_seq: event.tamper_chain_seq,
        tamper_prev_hash: event.tamper_prev_hash,
        tamper_hash: event.tamper_hash,
        run_id: event.run_id,
        step_id: event.step_id,
        tenant_id: event.tenant_id,
        agent_id: event.agent_id,
        user_id: event.user_id,
        actor: event.actor,
        event_type: event.event_type,
        payload_json: event.payload_json,
        request_id: event.request_id,
        session_id: event.session_id,
        action_request_id: event.action_request_id,
        payment_request_id: event.payment_request_id,
        created_at: event.created_at,
        recorded_at: event.recorded_at,
    }
}

async fn list_tenant_compliance_audit_events_from_sqlite(
    sqlite: &SqlitePool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    event_type: Option<&str>,
    limit: i64,
) -> ApiResult<Vec<agent_core::ComplianceAuditEventDetailRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT id,
               source_audit_event_id,
               tamper_chain_seq,
               tamper_prev_hash,
               tamper_hash,
               run_id,
               step_id,
               tenant_id,
               agent_id,
               user_id,
               actor,
               event_type,
               payload_json,
               request_id,
               session_id,
               action_request_id,
               payment_request_id,
               created_at,
               recorded_at
        FROM compliance_audit_events
        WHERE tenant_id = ?1
          AND (?2 IS NULL OR run_id = ?2)
          AND (?3 IS NULL OR event_type = ?3)
        ORDER BY COALESCE(tamper_chain_seq, 0) ASC, datetime(created_at) ASC, id ASC
        LIMIT ?4
        "#,
    )
    .bind(tenant_id)
    .bind(run_id.map(|value| value.to_string()))
    .bind(event_type)
    .bind(limit.clamp(1, 1000))
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying compliance audit events: {err}")))?;

    rows.into_iter()
        .map(|row| {
            let payload_json = parse_sqlite_json_required(&row, "payload_json")?;
            let request_id_column: Option<String> = row.get("request_id");
            let session_id_column: Option<String> = row.get("session_id");
            let action_request_id_column = parse_sqlite_uuid_optional(&row, "action_request_id")?;
            let payment_request_id_column = parse_sqlite_uuid_optional(&row, "payment_request_id")?;

            Ok(agent_core::ComplianceAuditEventDetailRecord {
                id: parse_sqlite_uuid_required(&row, "id")?,
                source_audit_event_id: parse_sqlite_uuid_required(&row, "source_audit_event_id")?,
                tamper_chain_seq: row.get::<Option<i64>, _>("tamper_chain_seq").unwrap_or(0),
                tamper_prev_hash: row.get("tamper_prev_hash"),
                tamper_hash: row.get("tamper_hash"),
                run_id: parse_sqlite_uuid_required(&row, "run_id")?,
                step_id: parse_sqlite_uuid_optional(&row, "step_id")?,
                tenant_id: row.get("tenant_id"),
                agent_id: parse_sqlite_uuid_optional(&row, "agent_id")?,
                user_id: parse_sqlite_uuid_optional(&row, "user_id")?,
                actor: row.get("actor"),
                event_type: row.get("event_type"),
                request_id: request_id_column
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .or_else(|| {
                        payload_string_field(
                            &payload_json,
                            &["request_id", "http_request_id", "correlation_request_id"],
                        )
                    }),
                session_id: session_id_column
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .or_else(|| {
                        payload_string_field(
                            &payload_json,
                            &["session_id", "correlation_session_id"],
                        )
                    }),
                action_request_id: action_request_id_column
                    .or_else(|| payload_uuid_field(&payload_json, &["action_request_id"])),
                payment_request_id: payment_request_id_column
                    .or_else(|| payload_uuid_field(&payload_json, &["payment_request_id"])),
                payload_json,
                created_at: parse_sqlite_datetime_required(&row, "created_at")?,
                recorded_at: parse_sqlite_datetime_required(&row, "recorded_at")?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()
}

fn compliance_siem_delivery_item_from_sqlite_row(
    row: &sqlx::sqlite::SqliteRow,
) -> ApiResult<ComplianceAuditSiemDeliveryItemResponse> {
    Ok(ComplianceAuditSiemDeliveryItemResponse {
        id: parse_sqlite_uuid_required(row, "id")?,
        tenant_id: row.get("tenant_id"),
        run_id: parse_sqlite_uuid_optional(row, "run_id")?,
        adapter: row.get("adapter"),
        delivery_target: row.get("delivery_target"),
        status: row.get("status"),
        attempts: row.get("attempts"),
        max_attempts: row.get("max_attempts"),
        next_attempt_at: parse_sqlite_datetime_required(row, "next_attempt_at")?,
        leased_by: row.get("leased_by"),
        lease_expires_at: parse_sqlite_datetime_optional(row, "lease_expires_at")?,
        last_error: row.get("last_error"),
        last_http_status: row.get("last_http_status"),
        created_at: parse_sqlite_datetime_required(row, "created_at")?,
        updated_at: parse_sqlite_datetime_required(row, "updated_at")?,
        delivered_at: parse_sqlite_datetime_optional(row, "delivered_at")?,
    })
}

fn compliance_siem_delivery_from_item(
    item: &ComplianceAuditSiemDeliveryItemResponse,
) -> ComplianceAuditSiemDeliveryResponse {
    ComplianceAuditSiemDeliveryResponse {
        id: item.id,
        tenant_id: item.tenant_id.clone(),
        run_id: item.run_id,
        adapter: item.adapter.clone(),
        delivery_target: item.delivery_target.clone(),
        status: item.status.clone(),
        attempts: item.attempts,
        max_attempts: item.max_attempts,
        next_attempt_at: item.next_attempt_at,
        created_at: item.created_at,
    }
}

#[derive(Debug)]
struct SqliteSiemDeliveryAlertAckRecord {
    delivery_target: String,
    acknowledged_by_user_id: Uuid,
    acknowledged_by_role: String,
    note: Option<String>,
    acknowledged_at: OffsetDateTime,
}

async fn list_tenant_compliance_siem_delivery_alert_acks_sqlite(
    sqlite: &SqlitePool,
    tenant_id: &str,
    run_scope: &str,
    limit: i64,
) -> ApiResult<Vec<SqliteSiemDeliveryAlertAckRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT delivery_target,
               acknowledged_by_user_id,
               acknowledged_by_role,
               note,
               acknowledged_at
        FROM compliance_siem_delivery_alert_acks
        WHERE tenant_id = ?1
          AND run_scope = ?2
        ORDER BY datetime(acknowledged_at) DESC, id DESC
        LIMIT ?3
        "#,
    )
    .bind(tenant_id)
    .bind(run_scope)
    .bind(limit.clamp(1, 500))
    .fetch_all(sqlite)
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed querying siem delivery alert acknowledgements: {err}"
        ))
    })?;

    rows.into_iter()
        .map(|row| {
            Ok(SqliteSiemDeliveryAlertAckRecord {
                delivery_target: row.get("delivery_target"),
                acknowledged_by_user_id: parse_sqlite_uuid_required(
                    &row,
                    "acknowledged_by_user_id",
                )?,
                acknowledged_by_role: row.get("acknowledged_by_role"),
                note: row.get("note"),
                acknowledged_at: parse_sqlite_datetime_required(&row, "acknowledged_at")?,
            })
        })
        .collect()
}

async fn list_tenant_compliance_siem_delivery_target_summaries_sqlite(
    sqlite: &SqlitePool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    since: Option<OffsetDateTime>,
    limit: i64,
) -> ApiResult<Vec<ComplianceAuditSiemDeliveryTargetSummaryResponse>> {
    let run_id_text = run_id.map(|value| value.to_string());
    let since_text = since
        .map(|value| value.format(&Rfc3339))
        .transpose()
        .map_err(|err| {
            ApiError::internal(format!(
                "failed formatting siem target summary since timestamp: {err}"
            ))
        })?;
    let rows = sqlx::query(
        r#"
        SELECT outbox.delivery_target,
               COALESCE(SUM(CASE WHEN outbox.status = 'pending' THEN 1 ELSE 0 END), 0) AS pending_count,
               COALESCE(SUM(CASE WHEN outbox.status = 'processing' THEN 1 ELSE 0 END), 0) AS processing_count,
               COALESCE(SUM(CASE WHEN outbox.status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
               COALESCE(SUM(CASE WHEN outbox.status = 'delivered' THEN 1 ELSE 0 END), 0) AS delivered_count,
               COALESCE(SUM(CASE WHEN outbox.status = 'dead_lettered' THEN 1 ELSE 0 END), 0) AS dead_lettered_count,
               COUNT(*) AS total_count,
               (
                 SELECT latest.last_error
                 FROM compliance_siem_delivery_outbox latest
                 WHERE latest.tenant_id = ?1
                   AND (?2 IS NULL OR latest.run_id = ?2)
                   AND (?3 IS NULL OR datetime(latest.created_at) >= datetime(?3))
                   AND latest.delivery_target = outbox.delivery_target
                   AND latest.last_error IS NOT NULL
                 ORDER BY datetime(latest.updated_at) DESC, latest.id DESC
                 LIMIT 1
               ) AS last_error,
               (
                 SELECT latest.last_http_status
                 FROM compliance_siem_delivery_outbox latest
                 WHERE latest.tenant_id = ?1
                   AND (?2 IS NULL OR latest.run_id = ?2)
                   AND (?3 IS NULL OR datetime(latest.created_at) >= datetime(?3))
                   AND latest.delivery_target = outbox.delivery_target
                   AND latest.last_error IS NOT NULL
                 ORDER BY datetime(latest.updated_at) DESC, latest.id DESC
                 LIMIT 1
               ) AS last_http_status,
               (
                 SELECT latest.updated_at
                 FROM compliance_siem_delivery_outbox latest
                 WHERE latest.tenant_id = ?1
                   AND (?2 IS NULL OR latest.run_id = ?2)
                   AND (?3 IS NULL OR datetime(latest.created_at) >= datetime(?3))
                   AND latest.delivery_target = outbox.delivery_target
                 ORDER BY datetime(latest.updated_at) DESC, latest.id DESC
                 LIMIT 1
               ) AS last_attempt_at
        FROM compliance_siem_delivery_outbox outbox
        WHERE outbox.tenant_id = ?1
          AND (?2 IS NULL OR outbox.run_id = ?2)
          AND (?3 IS NULL OR datetime(outbox.created_at) >= datetime(?3))
        GROUP BY outbox.delivery_target
        ORDER BY failed_count DESC, dead_lettered_count DESC, total_count DESC, outbox.delivery_target ASC
        LIMIT ?4
        "#,
    )
    .bind(tenant_id)
    .bind(run_id_text)
    .bind(since_text)
    .bind(limit.clamp(1, 200))
    .fetch_all(sqlite)
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed querying siem delivery target summaries: {err}"
        ))
    })?;

    rows.into_iter()
        .map(|row| {
            Ok(ComplianceAuditSiemDeliveryTargetSummaryResponse {
                delivery_target: row.get("delivery_target"),
                total_count: row.get("total_count"),
                pending_count: row.get("pending_count"),
                processing_count: row.get("processing_count"),
                failed_count: row.get("failed_count"),
                delivered_count: row.get("delivered_count"),
                dead_lettered_count: row.get("dead_lettered_count"),
                last_error: row.get("last_error"),
                last_http_status: row.get("last_http_status"),
                last_attempt_at: parse_sqlite_datetime_optional(&row, "last_attempt_at")?,
            })
        })
        .collect()
}

#[derive(Debug)]
struct SqliteComplianceAuditPolicyRecord {
    tenant_id: String,
    compliance_hot_retention_days: i32,
    compliance_archive_retention_days: i32,
    legal_hold: bool,
    legal_hold_reason: Option<String>,
    updated_at: Option<OffsetDateTime>,
}

async fn get_tenant_compliance_audit_policy_sqlite(
    sqlite: &SqlitePool,
    tenant_id: &str,
) -> ApiResult<SqliteComplianceAuditPolicyRecord> {
    let row = sqlx::query(
        r#"
        SELECT ?1 AS tenant_id,
               COALESCE(policy.compliance_hot_retention_days, 180) AS compliance_hot_retention_days,
               COALESCE(policy.compliance_archive_retention_days, 2555) AS compliance_archive_retention_days,
               COALESCE(policy.legal_hold, 0) AS legal_hold,
               policy.legal_hold_reason,
               policy.updated_at
        FROM (SELECT 1) AS seed
        LEFT JOIN compliance_audit_policies policy
          ON policy.tenant_id = ?1
        "#,
    )
    .bind(tenant_id)
    .fetch_one(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed loading compliance policy: {err}")))?;

    Ok(SqliteComplianceAuditPolicyRecord {
        tenant_id: row.get("tenant_id"),
        compliance_hot_retention_days: row.get("compliance_hot_retention_days"),
        compliance_archive_retention_days: row.get("compliance_archive_retention_days"),
        legal_hold: row.get::<i64, _>("legal_hold") != 0,
        legal_hold_reason: row.get("legal_hold_reason"),
        updated_at: parse_sqlite_datetime_optional(&row, "updated_at")?,
    })
}

async fn get_compliance_audit_policy_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let policy = get_tenant_compliance_audit_policy_sqlite(sqlite, tenant_id.as_str()).await?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditPolicyResponse {
            tenant_id: policy.tenant_id,
            compliance_hot_retention_days: policy.compliance_hot_retention_days,
            compliance_archive_retention_days: policy.compliance_archive_retention_days,
            legal_hold: policy.legal_hold,
            legal_hold_reason: policy.legal_hold_reason,
            updated_at: policy.updated_at,
        }),
    ))
}

async fn put_compliance_audit_policy_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<UpdateComplianceAuditPolicyRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_owner_role(role_preset, "only owner can update compliance policy")?;

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let existing = get_tenant_compliance_audit_policy_sqlite(sqlite, tenant_id.as_str()).await?;
    let compliance_hot_retention_days = req
        .compliance_hot_retention_days
        .unwrap_or(existing.compliance_hot_retention_days);
    let compliance_archive_retention_days = req
        .compliance_archive_retention_days
        .unwrap_or(existing.compliance_archive_retention_days);
    let legal_hold = req.legal_hold.unwrap_or(existing.legal_hold);
    let legal_hold_reason = req
        .legal_hold_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if compliance_hot_retention_days <= 0 {
        return Err(ApiError::bad_request(
            "compliance_hot_retention_days must be greater than zero",
        ));
    }
    if compliance_archive_retention_days <= 0 {
        return Err(ApiError::bad_request(
            "compliance_archive_retention_days must be greater than zero",
        ));
    }
    if compliance_archive_retention_days < compliance_hot_retention_days {
        return Err(ApiError::bad_request(
            "compliance_archive_retention_days must be >= compliance_hot_retention_days",
        ));
    }

    let row = sqlx::query(
        r#"
        INSERT INTO compliance_audit_policies (
            tenant_id,
            compliance_hot_retention_days,
            compliance_archive_retention_days,
            legal_hold,
            legal_hold_reason,
            updated_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
        ON CONFLICT (tenant_id)
        DO UPDATE SET
            compliance_hot_retention_days = excluded.compliance_hot_retention_days,
            compliance_archive_retention_days = excluded.compliance_archive_retention_days,
            legal_hold = excluded.legal_hold,
            legal_hold_reason = excluded.legal_hold_reason,
            updated_at = CURRENT_TIMESTAMP
        RETURNING tenant_id,
                  compliance_hot_retention_days,
                  compliance_archive_retention_days,
                  legal_hold,
                  legal_hold_reason,
                  updated_at
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(compliance_hot_retention_days)
    .bind(compliance_archive_retention_days)
    .bind(if legal_hold { 1_i64 } else { 0_i64 })
    .bind(legal_hold_reason)
    .fetch_one(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed updating compliance policy: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditPolicyResponse {
            tenant_id: row.get("tenant_id"),
            compliance_hot_retention_days: row.get("compliance_hot_retention_days"),
            compliance_archive_retention_days: row.get("compliance_archive_retention_days"),
            legal_hold: row.get::<i64, _>("legal_hold") != 0,
            legal_hold_reason: row.get("legal_hold_reason"),
            updated_at: parse_sqlite_datetime_optional(&row, "updated_at")?,
        }),
    ))
}

async fn post_compliance_audit_purge_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_owner_role(role_preset, "only owner can purge compliance audit data")?;

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let policy = get_tenant_compliance_audit_policy_sqlite(sqlite, tenant_id.as_str()).await?;
    let cutoff_at = OffsetDateTime::now_utc()
        - time::Duration::days(policy.compliance_hot_retention_days as i64);

    if policy.legal_hold {
        return Ok((
            StatusCode::OK,
            Json(ComplianceAuditPurgeResponse {
                tenant_id: policy.tenant_id,
                deleted_count: 0,
                legal_hold: true,
                cutoff_at,
                compliance_hot_retention_days: policy.compliance_hot_retention_days,
                compliance_archive_retention_days: policy.compliance_archive_retention_days,
            }),
        ));
    }

    let cutoff_at_text = cutoff_at.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!(
            "failed formatting compliance purge cutoff timestamp: {err}"
        ))
    })?;
    let deleted_count = sqlx::query(
        r#"
        DELETE FROM compliance_audit_events
        WHERE tenant_id = ?1
          AND datetime(created_at) < datetime(?2)
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(cutoff_at_text)
    .execute(sqlite)
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed purging expired compliance audit events: {err}"
        ))
    })?
    .rows_affected() as i64;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditPurgeResponse {
            tenant_id: policy.tenant_id,
            deleted_count,
            legal_hold: false,
            cutoff_at,
            compliance_hot_retention_days: policy.compliance_hot_retention_days,
            compliance_archive_retention_days: policy.compliance_archive_retention_days,
        }),
    ))
}

async fn get_compliance_audit_verify_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT id,
               tamper_chain_seq,
               tamper_prev_hash,
               tamper_hash
        FROM compliance_audit_events
        WHERE tenant_id = ?1
        ORDER BY COALESCE(tamper_chain_seq, 0) ASC, datetime(created_at) ASC, id ASC
        "#,
    )
    .bind(tenant_id.as_str())
    .fetch_all(sqlite)
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed querying compliance tamper-chain rows for verification: {err}"
        ))
    })?;

    let checked_events = rows.len() as i64;
    let mut first_invalid_event_id = None;
    let mut latest_chain_seq: Option<i64> = None;
    let mut latest_tamper_hash: Option<String> = None;
    let mut expected_prev_hash: Option<String> = None;

    for (index, row) in rows.iter().enumerate() {
        let event_id = parse_sqlite_uuid_required(row, "id")?;
        let tamper_chain_seq: Option<i64> = row.get("tamper_chain_seq");
        let tamper_prev_hash: Option<String> = row.get("tamper_prev_hash");
        let tamper_hash: String = row.get("tamper_hash");

        latest_chain_seq = match (latest_chain_seq, tamper_chain_seq) {
            (Some(current_max), Some(candidate)) => Some(current_max.max(candidate)),
            (None, Some(candidate)) => Some(candidate),
            (current, None) => current,
        };
        latest_tamper_hash = Some(tamper_hash.clone());

        let expected_seq = (index as i64) + 1;
        let seq_valid = tamper_chain_seq.is_some_and(|value| value == expected_seq);
        let prev_valid = tamper_prev_hash.clone().unwrap_or_default()
            == expected_prev_hash.clone().unwrap_or_default();
        let hash_valid = !tamper_hash.trim().is_empty();

        if first_invalid_event_id.is_none() && !(seq_valid && prev_valid && hash_valid) {
            first_invalid_event_id = Some(event_id);
        }

        expected_prev_hash = Some(tamper_hash);
    }

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditVerifyResponse {
            tenant_id,
            checked_events,
            verified: first_invalid_event_id.is_none(),
            first_invalid_event_id,
            latest_chain_seq,
            latest_tamper_hash,
        }),
    ))
}

async fn get_compliance_audit_replay_package_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditReplayPackageQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let run = get_run_status_dual(&state.db_pool, &tenant_id, query.run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading replay run: {err}")))?
        .ok_or_else(|| ApiError::not_found("run not found"))?;

    let audit_limit = query.audit_limit.unwrap_or(2000).clamp(1, 5000);
    let compliance_limit = query.compliance_limit.unwrap_or(2000).clamp(1, 5000);
    let payment_limit = query.payment_limit.unwrap_or(500).clamp(1, 2000);
    let include_payments = query.include_payments.unwrap_or(true);

    let run_audits =
        list_run_audit_events_dual(&state.db_pool, &tenant_id, query.run_id, audit_limit)
            .await
            .map_err(|err| {
                ApiError::internal(format!("failed loading replay run audits: {err}"))
            })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let compliance_events = list_tenant_compliance_audit_events_from_sqlite(
        sqlite,
        tenant_id.as_str(),
        Some(query.run_id),
        None,
        compliance_limit,
    )
    .await?;

    let mut payment_ledger = if include_payments {
        list_tenant_payment_ledger_from_sqlite(
            sqlite,
            tenant_id.as_str(),
            Some(query.run_id),
            None,
            None,
            None,
            None,
            payment_limit,
        )
        .await?
    } else {
        Vec::new()
    };
    payment_ledger.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });

    let run_audit_events: Vec<AuditEventResponse> = run_audits
        .into_iter()
        .map(|event| AuditEventResponse {
            id: event.id,
            run_id: event.run_id,
            step_id: event.step_id,
            actor: event.actor,
            event_type: event.event_type,
            payload_json: event.payload_json,
            created_at: event.created_at,
        })
        .collect();
    let compliance_audit_events: Vec<ComplianceAuditEventResponse> = compliance_events
        .into_iter()
        .map(compliance_event_to_response)
        .collect();

    let mut timestamps: Vec<OffsetDateTime> = run_audit_events
        .iter()
        .map(|event| event.created_at)
        .chain(compliance_audit_events.iter().map(|event| event.created_at))
        .chain(payment_ledger.iter().map(|event| event.created_at))
        .collect();
    timestamps.sort();
    let correlation = ComplianceReplayCorrelationSummary {
        run_audit_event_count: run_audit_events.len(),
        compliance_event_count: compliance_audit_events.len(),
        payment_event_count: payment_ledger.len(),
        first_event_at: timestamps.first().copied(),
        last_event_at: timestamps.last().copied(),
    };
    let generated_at = OffsetDateTime::now_utc();
    let manifest = build_replay_manifest(
        tenant_id.as_str(),
        run.id,
        generated_at,
        run_audit_events.as_slice(),
        compliance_audit_events.as_slice(),
        payment_ledger.as_slice(),
        &correlation,
    )?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditReplayPackageResponse {
            tenant_id,
            run: run_to_response(run),
            generated_at,
            run_audit_events,
            compliance_audit_events,
            payment_ledger,
            correlation,
            manifest,
        }),
    ))
}

async fn get_compliance_audit_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let event_type = trim_non_empty(query.event_type.as_deref());
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let events = list_tenant_compliance_audit_events_from_sqlite(
        sqlite,
        tenant_id.as_str(),
        query.run_id,
        event_type,
        limit,
    )
    .await?;
    let body: Vec<ComplianceAuditEventResponse> = events
        .into_iter()
        .map(compliance_event_to_response)
        .collect();

    Ok((StatusCode::OK, Json(body)))
}

async fn get_compliance_audit_export_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditExportQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    let event_type = trim_non_empty(query.event_type.as_deref());
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let events = list_tenant_compliance_audit_events_from_sqlite(
        sqlite,
        tenant_id.as_str(),
        query.run_id,
        event_type,
        limit,
    )
    .await?;
    let ndjson = serialize_compliance_events_as_ndjson(events.as_slice())?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        ndjson,
    ))
}

async fn get_compliance_audit_siem_export_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemExportQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    let event_type = trim_non_empty(query.event_type.as_deref());
    let adapter = SiemAdapter::parse(query.adapter.as_deref())?;
    let elastic_index = trim_non_empty(query.elastic_index.as_deref())
        .unwrap_or("secureagnt-compliance-audit")
        .to_string();
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let events = list_tenant_compliance_audit_events_from_sqlite(
        sqlite,
        tenant_id.as_str(),
        query.run_id,
        event_type,
        limit,
    )
    .await?;
    let payload =
        serialize_siem_adapter_payload(events.as_slice(), adapter, elastic_index.as_str())?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        payload,
    ))
}

async fn post_compliance_audit_siem_delivery_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<ComplianceAuditSiemDeliveryRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let delivery_target = req.delivery_target.trim();
    if delivery_target.is_empty() {
        return Err(ApiError::bad_request("delivery_target must not be empty"));
    }
    let limit = req.limit.unwrap_or(500).clamp(1, 1000);
    let event_type = trim_non_empty(req.event_type.as_deref());
    let adapter = SiemAdapter::parse(req.adapter.as_deref())?;
    let elastic_index = trim_non_empty(req.elastic_index.as_deref())
        .unwrap_or("secureagnt-compliance-audit")
        .to_string();
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;

    let events = list_tenant_compliance_audit_events_from_sqlite(
        sqlite,
        tenant_id.as_str(),
        req.run_id,
        event_type,
        limit,
    )
    .await?;
    let payload =
        serialize_siem_adapter_payload(events.as_slice(), adapter, elastic_index.as_str())?;

    let row = sqlx::query(
        r#"
        INSERT INTO compliance_siem_delivery_outbox (
            id,
            tenant_id,
            run_id,
            adapter,
            delivery_target,
            content_type,
            payload_ndjson,
            max_attempts
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        RETURNING id,
                  tenant_id,
                  run_id,
                  adapter,
                  delivery_target,
                  status,
                  attempts,
                  max_attempts,
                  next_attempt_at,
                  leased_by,
                  lease_expires_at,
                  last_error,
                  last_http_status,
                  created_at,
                  updated_at,
                  delivered_at
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(tenant_id.as_str())
    .bind(req.run_id.map(|value| value.to_string()))
    .bind(adapter.as_str())
    .bind(delivery_target)
    .bind("application/x-ndjson")
    .bind(payload)
    .bind(req.max_attempts.unwrap_or(3).clamp(1, 20))
    .fetch_one(sqlite)
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed queueing siem delivery outbox row: {err}"))
    })?;
    let item = compliance_siem_delivery_item_from_sqlite_row(&row)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(compliance_siem_delivery_from_item(&item)),
    ))
}

async fn get_compliance_audit_siem_deliveries_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliveriesQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let status = parse_siem_outbox_status(trim_non_empty(query.status.as_deref()))?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               run_id,
               adapter,
               delivery_target,
               status,
               attempts,
               max_attempts,
               next_attempt_at,
               leased_by,
               lease_expires_at,
               last_error,
               last_http_status,
               created_at,
               updated_at,
               delivered_at
        FROM compliance_siem_delivery_outbox
        WHERE tenant_id = ?1
          AND (?2 IS NULL OR run_id = ?2)
          AND (?3 IS NULL OR status = ?3)
        ORDER BY datetime(created_at) DESC, id DESC
        LIMIT ?4
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(query.run_id.map(|value| value.to_string()))
    .bind(status)
    .bind(limit)
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying siem deliveries: {err}")))?;

    let body = rows
        .iter()
        .map(compliance_siem_delivery_item_from_sqlite_row)
        .collect::<ApiResult<Vec<_>>>()?;

    Ok((StatusCode::OK, Json(body)))
}

async fn get_compliance_audit_siem_deliveries_summary_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliverySummaryQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;

    let row = sqlx::query(
        r#"
        SELECT COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS pending_count,
               COALESCE(SUM(CASE WHEN status = 'processing' THEN 1 ELSE 0 END), 0) AS processing_count,
               COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
               COALESCE(SUM(CASE WHEN status = 'delivered' THEN 1 ELSE 0 END), 0) AS delivered_count,
               COALESCE(SUM(CASE WHEN status = 'dead_lettered' THEN 1 ELSE 0 END), 0) AS dead_lettered_count,
               (
                 SELECT (julianday('now') - julianday(MIN(created_at))) * 86400.0
                 FROM compliance_siem_delivery_outbox
                 WHERE tenant_id = ?1
                   AND status = 'pending'
                   AND (?2 IS NULL OR run_id = ?2)
               ) AS oldest_pending_age_seconds
        FROM compliance_siem_delivery_outbox
        WHERE tenant_id = ?1
          AND (?2 IS NULL OR run_id = ?2)
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(query.run_id.map(|value| value.to_string()))
    .fetch_one(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying siem delivery summary: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliverySummaryResponse {
            tenant_id,
            run_id: query.run_id,
            pending_count: row.get("pending_count"),
            processing_count: row.get("processing_count"),
            failed_count: row.get("failed_count"),
            delivered_count: row.get("delivered_count"),
            dead_lettered_count: row.get("dead_lettered_count"),
            oldest_pending_age_seconds: row
                .get::<Option<f64>, _>("oldest_pending_age_seconds")
                .map(|value| value.max(0.0)),
        }),
    ))
}

async fn get_compliance_audit_siem_deliveries_slo_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliverySloQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let since_text = since.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting siem slo since timestamp: {err}"))
    })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let row = sqlx::query(
        r#"
        WITH filtered AS (
          SELECT status, created_at
          FROM compliance_siem_delivery_outbox
          WHERE tenant_id = ?1
            AND (?2 IS NULL OR run_id = ?2)
            AND datetime(created_at) >= datetime(?3)
        )
        SELECT COUNT(*) AS total_count,
               COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS pending_count,
               COALESCE(SUM(CASE WHEN status = 'processing' THEN 1 ELSE 0 END), 0) AS processing_count,
               COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
               COALESCE(SUM(CASE WHEN status = 'delivered' THEN 1 ELSE 0 END), 0) AS delivered_count,
               COALESCE(SUM(CASE WHEN status = 'dead_lettered' THEN 1 ELSE 0 END), 0) AS dead_lettered_count,
               CASE
                 WHEN COUNT(*) = 0 THEN NULL
                 ELSE (SUM(CASE WHEN status = 'delivered' THEN 1 ELSE 0 END) * 100.0) / COUNT(*)
               END AS delivery_success_rate_pct,
               CASE
                 WHEN COUNT(*) = 0 THEN NULL
                 ELSE (
                   SUM(CASE WHEN status IN ('failed', 'dead_lettered') THEN 1 ELSE 0 END) * 100.0
                 ) / COUNT(*)
               END AS hard_failure_rate_pct,
               CASE
                 WHEN COUNT(*) = 0 THEN NULL
                 ELSE (SUM(CASE WHEN status = 'dead_lettered' THEN 1 ELSE 0 END) * 100.0) / COUNT(*)
               END AS dead_letter_rate_pct,
               CASE
                 WHEN SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) = 0 THEN NULL
                 ELSE (
                   julianday('now') - julianday(MIN(CASE WHEN status = 'pending' THEN created_at END))
                 ) * 86400.0
               END AS oldest_pending_age_seconds
        FROM filtered
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(query.run_id.map(|value| value.to_string()))
    .bind(since_text)
    .fetch_one(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying siem delivery slo: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliverySloResponse {
            tenant_id,
            run_id: query.run_id,
            window_secs,
            since,
            total_count: row.get("total_count"),
            pending_count: row.get("pending_count"),
            processing_count: row.get("processing_count"),
            failed_count: row.get("failed_count"),
            delivered_count: row.get("delivered_count"),
            dead_lettered_count: row.get("dead_lettered_count"),
            delivery_success_rate_pct: row.get("delivery_success_rate_pct"),
            hard_failure_rate_pct: row.get("hard_failure_rate_pct"),
            dead_letter_rate_pct: row.get("dead_letter_rate_pct"),
            oldest_pending_age_seconds: row
                .get::<Option<f64>, _>("oldest_pending_age_seconds")
                .map(|value| value.max(0.0)),
        }),
    ))
}

async fn get_compliance_audit_siem_delivery_targets_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliveryTargetsQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let body = list_tenant_compliance_siem_delivery_target_summaries_sqlite(
        sqlite,
        tenant_id.as_str(),
        query.run_id,
        Some(since),
        query.limit.unwrap_or(100).clamp(1, 200),
    )
    .await?;

    Ok((StatusCode::OK, Json(body)))
}

async fn get_compliance_audit_siem_delivery_alerts_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliveryAlertsQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let max_hard_failure_rate_pct = query.max_hard_failure_rate_pct.unwrap_or(0.0);
    if !(0.0..=100.0).contains(&max_hard_failure_rate_pct) {
        return Err(ApiError::bad_request(
            "max_hard_failure_rate_pct must be between 0 and 100",
        ));
    }
    let max_dead_letter_rate_pct = query.max_dead_letter_rate_pct.unwrap_or(0.0);
    if !(0.0..=100.0).contains(&max_dead_letter_rate_pct) {
        return Err(ApiError::bad_request(
            "max_dead_letter_rate_pct must be between 0 and 100",
        ));
    }
    let max_pending_count = query.max_pending_count.unwrap_or(0).max(0);

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = list_tenant_compliance_siem_delivery_target_summaries_sqlite(
        sqlite,
        tenant_id.as_str(),
        query.run_id,
        Some(since),
        limit,
    )
    .await?;

    let run_scope = compliance_alert_run_scope(query.run_id);
    let mut ack_by_target: HashMap<String, SqliteSiemDeliveryAlertAckRecord> = HashMap::new();
    let scoped_acks = list_tenant_compliance_siem_delivery_alert_acks_sqlite(
        sqlite,
        tenant_id.as_str(),
        run_scope.as_str(),
        limit,
    )
    .await?;
    for ack in scoped_acks {
        ack_by_target.insert(ack.delivery_target.clone(), ack);
    }
    if run_scope != "*" {
        let global_acks = list_tenant_compliance_siem_delivery_alert_acks_sqlite(
            sqlite,
            tenant_id.as_str(),
            "*",
            limit,
        )
        .await?;
        for ack in global_acks {
            ack_by_target
                .entry(ack.delivery_target.clone())
                .or_insert(ack);
        }
    }

    let mut alerts = rows
        .into_iter()
        .filter_map(|row| {
            let ack = ack_by_target.get(row.delivery_target.as_str());
            let safe_total = row.total_count.max(0) as f64;
            let hard_failure_rate_pct = if safe_total > 0.0 {
                Some(
                    ((row.failed_count.max(0) + row.dead_lettered_count.max(0)) as f64 * 100.0)
                        / safe_total,
                )
            } else {
                None
            };
            let dead_letter_rate_pct = if safe_total > 0.0 {
                Some((row.dead_lettered_count.max(0) as f64 * 100.0) / safe_total)
            } else {
                None
            };

            let mut triggered_rules = Vec::new();
            if row.pending_count > max_pending_count {
                triggered_rules.push(format!(
                    "pending_count {} > {}",
                    row.pending_count, max_pending_count
                ));
            }
            if let Some(rate) = hard_failure_rate_pct {
                if rate > max_hard_failure_rate_pct {
                    triggered_rules.push(format!(
                        "hard_failure_rate_pct {:.2} > {:.2}",
                        rate, max_hard_failure_rate_pct
                    ));
                }
            }
            if let Some(rate) = dead_letter_rate_pct {
                if rate > max_dead_letter_rate_pct {
                    triggered_rules.push(format!(
                        "dead_letter_rate_pct {:.2} > {:.2}",
                        rate, max_dead_letter_rate_pct
                    ));
                }
            }
            if triggered_rules.is_empty() {
                return None;
            }

            let severity = if triggered_rules.iter().any(|rule| {
                rule.starts_with("hard_failure_rate_pct")
                    || rule.starts_with("dead_letter_rate_pct")
            }) {
                "critical"
            } else {
                "warning"
            };

            Some(ComplianceAuditSiemDeliveryAlertItemResponse {
                delivery_target: row.delivery_target,
                total_count: row.total_count,
                pending_count: row.pending_count,
                processing_count: row.processing_count,
                failed_count: row.failed_count,
                delivered_count: row.delivered_count,
                dead_lettered_count: row.dead_lettered_count,
                hard_failure_rate_pct,
                dead_letter_rate_pct,
                triggered_rules,
                severity: severity.to_string(),
                last_error: row.last_error,
                last_http_status: row.last_http_status,
                last_attempt_at: row.last_attempt_at,
                acknowledged: ack.is_some(),
                acknowledged_at: ack.map(|item| item.acknowledged_at),
                acknowledged_by_user_id: ack.map(|item| item.acknowledged_by_user_id),
                acknowledged_by_role: ack.map(|item| item.acknowledged_by_role.clone()),
                acknowledgement_note: ack.and_then(|item| item.note.clone()),
            })
        })
        .collect::<Vec<_>>();

    alerts.sort_by(|left, right| {
        let left_rank = if left.severity == "critical" { 0 } else { 1 };
        let right_rank = if right.severity == "critical" { 0 } else { 1 };
        left_rank
            .cmp(&right_rank)
            .then_with(|| right.triggered_rules.len().cmp(&left.triggered_rules.len()))
            .then_with(|| right.pending_count.cmp(&left.pending_count))
            .then_with(|| left.delivery_target.cmp(&right.delivery_target))
    });

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliveryAlertResponse {
            tenant_id,
            run_id: query.run_id,
            window_secs,
            since,
            thresholds: ComplianceAuditSiemDeliveryAlertThresholdsResponse {
                max_hard_failure_rate_pct: Some(max_hard_failure_rate_pct),
                max_dead_letter_rate_pct: Some(max_dead_letter_rate_pct),
                max_pending_count: Some(max_pending_count),
            },
            alerts,
        }),
    ))
}

async fn ack_compliance_audit_siem_delivery_alert_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Json(req): Json<AckComplianceAuditSiemDeliveryAlertRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let acknowledged_by_user_id =
        user_id_from_headers_sqlite(&state, &headers)?.ok_or_else(|| {
            ApiError::forbidden("x-user-id header is required to acknowledge compliance alerts")
        })?;

    let delivery_target = req.delivery_target.trim();
    if delivery_target.is_empty() {
        return Err(ApiError::bad_request("delivery_target cannot be empty"));
    }

    let note = req
        .note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if note.as_ref().is_some_and(|value| value.len() > 2_000) {
        return Err(ApiError::bad_request("note must be <= 2000 characters"));
    }

    let run_scope = compliance_alert_run_scope(req.run_id);
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let row = sqlx::query(
        r#"
        INSERT INTO compliance_siem_delivery_alert_acks (
            id,
            tenant_id,
            run_scope,
            delivery_target,
            acknowledged_by_user_id,
            acknowledged_by_role,
            note
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT (tenant_id, run_scope, delivery_target)
        DO UPDATE SET
            acknowledged_by_user_id = excluded.acknowledged_by_user_id,
            acknowledged_by_role = excluded.acknowledged_by_role,
            note = excluded.note,
            acknowledged_at = CURRENT_TIMESTAMP
        RETURNING tenant_id,
                  run_scope,
                  delivery_target,
                  acknowledged_by_user_id,
                  acknowledged_by_role,
                  note,
                  created_at,
                  acknowledged_at
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(tenant_id.as_str())
    .bind(run_scope.as_str())
    .bind(delivery_target)
    .bind(acknowledged_by_user_id.to_string())
    .bind(role_preset.as_str())
    .bind(note)
    .fetch_one(sqlite)
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed acknowledging siem delivery alert: {err}"))
    })?;

    let run_scope: String = row.get("run_scope");
    let run_id = if run_scope == "*" {
        None
    } else {
        Uuid::parse_str(run_scope.as_str()).ok()
    };

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliveryAlertAckResponse {
            tenant_id: row.get("tenant_id"),
            run_scope,
            run_id,
            delivery_target: row.get("delivery_target"),
            acknowledged_by_user_id: parse_sqlite_uuid_required(&row, "acknowledged_by_user_id")?,
            acknowledged_by_role: row.get("acknowledged_by_role"),
            acknowledgement_note: row.get("note"),
            created_at: parse_sqlite_datetime_required(&row, "created_at")?,
            acknowledged_at: parse_sqlite_datetime_required(&row, "acknowledged_at")?,
        }),
    ))
}

async fn replay_compliance_audit_siem_delivery_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Path(record_id): Path<Uuid>,
    Json(req): Json<ReplayComplianceAuditSiemDeliveryRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let retry_at = OffsetDateTime::now_utc()
        + time::Duration::seconds(req.delay_secs.unwrap_or(0).clamp(0, 86_400) as i64);
    let retry_at_text = retry_at.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!(
            "failed formatting siem delivery replay retry timestamp: {err}"
        ))
    })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET status = 'pending',
            attempts = 0,
            next_attempt_at = ?3,
            leased_by = NULL,
            lease_expires_at = NULL,
            last_error = NULL,
            last_http_status = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
          AND tenant_id = ?2
          AND status = 'dead_lettered'
        RETURNING id,
                  tenant_id,
                  run_id,
                  adapter,
                  delivery_target,
                  status,
                  attempts,
                  max_attempts,
                  next_attempt_at,
                  leased_by,
                  lease_expires_at,
                  last_error,
                  last_http_status,
                  created_at,
                  updated_at,
                  delivered_at
        "#,
    )
    .bind(record_id.to_string())
    .bind(tenant_id.as_str())
    .bind(retry_at_text)
    .fetch_optional(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed replaying siem delivery row: {err}")))?;

    let Some(row) = row else {
        return Err(ApiError::not_found(
            "siem delivery row not found or not dead_lettered",
        ));
    };
    let body = compliance_siem_delivery_item_from_sqlite_row(&row)?;

    Ok((StatusCode::ACCEPTED, Json(body)))
}

async fn get_compliance_audit_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(200).clamp(1, 1000);
    let event_type = trim_non_empty(query.event_type.as_deref());

    let events = list_tenant_compliance_audit_events(
        &state.pool,
        &tenant_id,
        query.run_id,
        event_type,
        limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed fetching compliance audit events: {err}")))?;

    let body: Vec<ComplianceAuditEventResponse> = events
        .into_iter()
        .map(|event| ComplianceAuditEventResponse {
            id: event.id,
            source_audit_event_id: event.source_audit_event_id,
            tamper_chain_seq: event.tamper_chain_seq,
            tamper_prev_hash: event.tamper_prev_hash,
            tamper_hash: event.tamper_hash,
            run_id: event.run_id,
            step_id: event.step_id,
            tenant_id: event.tenant_id,
            agent_id: event.agent_id,
            user_id: event.user_id,
            actor: event.actor,
            event_type: event.event_type,
            payload_json: event.payload_json,
            request_id: event.request_id,
            session_id: event.session_id,
            action_request_id: event.action_request_id,
            payment_request_id: event.payment_request_id,
            created_at: event.created_at,
            recorded_at: event.recorded_at,
        })
        .collect();

    Ok((StatusCode::OK, Json(body)))
}

async fn get_compliance_audit_policy_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let policy = get_tenant_compliance_audit_policy(&state.pool, &tenant_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed fetching compliance policy: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditPolicyResponse {
            tenant_id: policy.tenant_id,
            compliance_hot_retention_days: policy.compliance_hot_retention_days,
            compliance_archive_retention_days: policy.compliance_archive_retention_days,
            legal_hold: policy.legal_hold,
            legal_hold_reason: policy.legal_hold_reason,
            updated_at: policy.updated_at,
        }),
    ))
}

async fn put_compliance_audit_policy_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpdateComplianceAuditPolicyRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_owner_role(role_preset, "only owner can update compliance policy")?;

    let existing = get_tenant_compliance_audit_policy(&state.pool, &tenant_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading compliance policy: {err}")))?;

    let compliance_hot_retention_days = req
        .compliance_hot_retention_days
        .unwrap_or(existing.compliance_hot_retention_days);
    let compliance_archive_retention_days = req
        .compliance_archive_retention_days
        .unwrap_or(existing.compliance_archive_retention_days);
    let legal_hold = req.legal_hold.unwrap_or(existing.legal_hold);
    let legal_hold_reason = req
        .legal_hold_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if compliance_hot_retention_days <= 0 {
        return Err(ApiError::bad_request(
            "compliance_hot_retention_days must be greater than zero",
        ));
    }
    if compliance_archive_retention_days <= 0 {
        return Err(ApiError::bad_request(
            "compliance_archive_retention_days must be greater than zero",
        ));
    }
    if compliance_archive_retention_days < compliance_hot_retention_days {
        return Err(ApiError::bad_request(
            "compliance_archive_retention_days must be >= compliance_hot_retention_days",
        ));
    }

    let updated = upsert_tenant_compliance_audit_policy(
        &state.pool,
        &tenant_id,
        compliance_hot_retention_days,
        compliance_archive_retention_days,
        legal_hold,
        legal_hold_reason,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed updating compliance policy: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditPolicyResponse {
            tenant_id: updated.tenant_id,
            compliance_hot_retention_days: updated.compliance_hot_retention_days,
            compliance_archive_retention_days: updated.compliance_archive_retention_days,
            legal_hold: updated.legal_hold,
            legal_hold_reason: updated.legal_hold_reason,
            updated_at: updated.updated_at,
        }),
    ))
}

async fn post_compliance_audit_purge_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_owner_role(role_preset, "only owner can purge compliance audit data")?;

    let outcome = purge_expired_tenant_compliance_audit_events(
        &state.pool,
        &tenant_id,
        OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed purging expired compliance audit events: {err}"
        ))
    })?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditPurgeResponse {
            tenant_id: outcome.tenant_id,
            deleted_count: outcome.deleted_count,
            legal_hold: outcome.legal_hold,
            cutoff_at: outcome.cutoff_at,
            compliance_hot_retention_days: outcome.compliance_hot_retention_days,
            compliance_archive_retention_days: outcome.compliance_archive_retention_days,
        }),
    ))
}

async fn get_compliance_audit_verify_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let verification = verify_tenant_compliance_audit_chain(&state.pool, &tenant_id)
        .await
        .map_err(|err| {
            ApiError::internal(format!(
                "failed verifying compliance audit tamper chain: {err}"
            ))
        })?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditVerifyResponse {
            tenant_id: verification.tenant_id,
            checked_events: verification.checked_events,
            verified: verification.verified,
            first_invalid_event_id: verification.first_invalid_event_id,
            latest_chain_seq: verification.latest_chain_seq,
            latest_tamper_hash: verification.latest_tamper_hash,
        }),
    ))
}

async fn get_compliance_audit_export_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditExportQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    let event_type = trim_non_empty(query.event_type.as_deref());

    let events = list_tenant_compliance_audit_events(
        &state.pool,
        &tenant_id,
        query.run_id,
        event_type,
        limit,
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed exporting compliance audit events: {err}"))
    })?;

    let ndjson = serialize_compliance_events_as_ndjson(events.as_slice())?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        ndjson,
    ))
}

async fn get_compliance_audit_siem_export_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemExportQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    let event_type = trim_non_empty(query.event_type.as_deref());
    let adapter = SiemAdapter::parse(query.adapter.as_deref())?;
    let elastic_index = trim_non_empty(query.elastic_index.as_deref())
        .unwrap_or("secureagnt-compliance-audit")
        .to_string();

    let events = list_tenant_compliance_audit_events(
        &state.pool,
        &tenant_id,
        query.run_id,
        event_type,
        limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed exporting siem compliance events: {err}")))?;

    let payload =
        serialize_siem_adapter_payload(events.as_slice(), adapter, elastic_index.as_str())?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        payload,
    ))
}

async fn post_compliance_audit_siem_delivery_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ComplianceAuditSiemDeliveryRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let delivery_target = req.delivery_target.trim();
    if delivery_target.is_empty() {
        return Err(ApiError::bad_request("delivery_target must not be empty"));
    }

    let limit = req.limit.unwrap_or(500).clamp(1, 1000);
    let event_type = trim_non_empty(req.event_type.as_deref());
    let adapter = SiemAdapter::parse(req.adapter.as_deref())?;
    let elastic_index = trim_non_empty(req.elastic_index.as_deref())
        .unwrap_or("secureagnt-compliance-audit")
        .to_string();

    let events =
        list_tenant_compliance_audit_events(&state.pool, &tenant_id, req.run_id, event_type, limit)
            .await
            .map_err(|err| {
                ApiError::internal(format!("failed exporting siem compliance events: {err}"))
            })?;

    let payload =
        serialize_siem_adapter_payload(events.as_slice(), adapter, elastic_index.as_str())?;
    let record = create_compliance_siem_delivery_record(
        &state.pool,
        &NewComplianceSiemDeliveryRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            run_id: req.run_id,
            adapter: adapter.as_str().to_string(),
            delivery_target: delivery_target.to_string(),
            content_type: "application/x-ndjson".to_string(),
            payload_ndjson: payload,
            max_attempts: req.max_attempts.unwrap_or(3).clamp(1, 20),
        },
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed queueing siem delivery outbox row: {err}"))
    })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ComplianceAuditSiemDeliveryResponse {
            id: record.id,
            tenant_id: record.tenant_id,
            run_id: record.run_id,
            adapter: record.adapter,
            delivery_target: record.delivery_target,
            status: record.status,
            attempts: record.attempts,
            max_attempts: record.max_attempts,
            next_attempt_at: record.next_attempt_at,
            created_at: record.created_at,
        }),
    ))
}

async fn get_compliance_audit_siem_deliveries_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliveriesQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let status = parse_siem_outbox_status(trim_non_empty(query.status.as_deref()))?;
    let rows = list_tenant_compliance_siem_delivery_records(
        &state.pool,
        tenant_id.as_str(),
        query.run_id,
        status,
        limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed querying siem deliveries: {err}")))?;

    let body: Vec<ComplianceAuditSiemDeliveryItemResponse> = rows
        .into_iter()
        .map(|row| ComplianceAuditSiemDeliveryItemResponse {
            id: row.id,
            tenant_id: row.tenant_id,
            run_id: row.run_id,
            adapter: row.adapter,
            delivery_target: row.delivery_target,
            status: row.status,
            attempts: row.attempts,
            max_attempts: row.max_attempts,
            next_attempt_at: row.next_attempt_at,
            leased_by: row.leased_by,
            lease_expires_at: row.lease_expires_at,
            last_error: row.last_error,
            last_http_status: row.last_http_status,
            created_at: row.created_at,
            updated_at: row.updated_at,
            delivered_at: row.delivered_at,
        })
        .collect();

    Ok((StatusCode::OK, Json(body)))
}

async fn get_compliance_audit_siem_deliveries_summary_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliverySummaryQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let summary =
        get_tenant_compliance_siem_delivery_summary(&state.pool, tenant_id.as_str(), query.run_id)
            .await
            .map_err(|err| {
                ApiError::internal(format!("failed querying siem delivery summary: {err}"))
            })?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliverySummaryResponse {
            tenant_id,
            run_id: query.run_id,
            pending_count: summary.pending_count,
            processing_count: summary.processing_count,
            failed_count: summary.failed_count,
            delivered_count: summary.delivered_count,
            dead_lettered_count: summary.dead_lettered_count,
            oldest_pending_age_seconds: summary.oldest_pending_age_seconds,
        }),
    ))
}

async fn get_compliance_audit_siem_deliveries_slo_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliverySloQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let slo = get_tenant_compliance_siem_delivery_slo(
        &state.pool,
        tenant_id.as_str(),
        query.run_id,
        since,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed querying siem delivery slo: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliverySloResponse {
            tenant_id,
            run_id: query.run_id,
            window_secs,
            since,
            total_count: slo.total_count,
            pending_count: slo.pending_count,
            processing_count: slo.processing_count,
            failed_count: slo.failed_count,
            delivered_count: slo.delivered_count,
            dead_lettered_count: slo.dead_lettered_count,
            delivery_success_rate_pct: slo.delivery_success_rate_pct,
            hard_failure_rate_pct: slo.hard_failure_rate_pct,
            dead_letter_rate_pct: slo.dead_letter_rate_pct,
            oldest_pending_age_seconds: slo.oldest_pending_age_seconds,
        }),
    ))
}

async fn get_compliance_audit_siem_delivery_targets_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliveryTargetsQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let rows = list_tenant_compliance_siem_delivery_target_summaries(
        &state.pool,
        tenant_id.as_str(),
        query.run_id,
        Some(since),
        query.limit.unwrap_or(100).clamp(1, 200),
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed querying siem delivery target summaries: {err}"
        ))
    })?;

    let body = rows
        .into_iter()
        .map(|row| ComplianceAuditSiemDeliveryTargetSummaryResponse {
            delivery_target: row.delivery_target,
            total_count: row.total_count,
            pending_count: row.pending_count,
            processing_count: row.processing_count,
            failed_count: row.failed_count,
            delivered_count: row.delivered_count,
            dead_lettered_count: row.dead_lettered_count,
            last_error: row.last_error,
            last_http_status: row.last_http_status,
            last_attempt_at: row.last_attempt_at,
        })
        .collect::<Vec<_>>();

    Ok((StatusCode::OK, Json(body)))
}

async fn get_compliance_audit_siem_delivery_alerts_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditSiemDeliveryAlertsQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let max_hard_failure_rate_pct = query.max_hard_failure_rate_pct.unwrap_or(0.0);
    if !(0.0..=100.0).contains(&max_hard_failure_rate_pct) {
        return Err(ApiError::bad_request(
            "max_hard_failure_rate_pct must be between 0 and 100",
        ));
    }
    let max_dead_letter_rate_pct = query.max_dead_letter_rate_pct.unwrap_or(0.0);
    if !(0.0..=100.0).contains(&max_dead_letter_rate_pct) {
        return Err(ApiError::bad_request(
            "max_dead_letter_rate_pct must be between 0 and 100",
        ));
    }
    let max_pending_count = query.max_pending_count.unwrap_or(0).max(0);

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let rows = list_tenant_compliance_siem_delivery_target_summaries(
        &state.pool,
        tenant_id.as_str(),
        query.run_id,
        Some(since),
        query.limit.unwrap_or(100).clamp(1, 200),
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed querying siem delivery target summaries for alerts: {err}"
        ))
    })?;
    let run_scope = compliance_alert_run_scope(query.run_id);
    let mut ack_by_target: HashMap<String, _> = HashMap::new();
    let scoped_acks = list_tenant_compliance_siem_delivery_alert_acks(
        &state.pool,
        tenant_id.as_str(),
        run_scope.as_str(),
        query.limit.unwrap_or(100).clamp(1, 200),
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!(
            "failed querying siem delivery alert acknowledgements: {err}"
        ))
    })?;
    for ack in scoped_acks {
        ack_by_target.insert(ack.delivery_target.clone(), ack);
    }
    if run_scope != "*" {
        let global_acks = list_tenant_compliance_siem_delivery_alert_acks(
            &state.pool,
            tenant_id.as_str(),
            "*",
            query.limit.unwrap_or(100).clamp(1, 200),
        )
        .await
        .map_err(|err| {
            ApiError::internal(format!(
                "failed querying global siem delivery alert acknowledgements: {err}"
            ))
        })?;
        for ack in global_acks {
            ack_by_target
                .entry(ack.delivery_target.clone())
                .or_insert(ack);
        }
    }

    let mut alerts = rows
        .into_iter()
        .filter_map(|row| {
            let ack = ack_by_target.get(row.delivery_target.as_str());
            let safe_total = row.total_count.max(0) as f64;
            let hard_failure_rate_pct = if safe_total > 0.0 {
                Some(
                    ((row.failed_count.max(0) + row.dead_lettered_count.max(0)) as f64 * 100.0)
                        / safe_total,
                )
            } else {
                None
            };
            let dead_letter_rate_pct = if safe_total > 0.0 {
                Some((row.dead_lettered_count.max(0) as f64 * 100.0) / safe_total)
            } else {
                None
            };

            let mut triggered_rules = Vec::new();
            if row.pending_count > max_pending_count {
                triggered_rules.push(format!(
                    "pending_count {} > {}",
                    row.pending_count, max_pending_count
                ));
            }
            if let Some(rate) = hard_failure_rate_pct {
                if rate > max_hard_failure_rate_pct {
                    triggered_rules.push(format!(
                        "hard_failure_rate_pct {:.2} > {:.2}",
                        rate, max_hard_failure_rate_pct
                    ));
                }
            }
            if let Some(rate) = dead_letter_rate_pct {
                if rate > max_dead_letter_rate_pct {
                    triggered_rules.push(format!(
                        "dead_letter_rate_pct {:.2} > {:.2}",
                        rate, max_dead_letter_rate_pct
                    ));
                }
            }

            if triggered_rules.is_empty() {
                return None;
            }

            let severity = if triggered_rules.iter().any(|rule| {
                rule.starts_with("hard_failure_rate_pct")
                    || rule.starts_with("dead_letter_rate_pct")
            }) {
                "critical"
            } else {
                "warning"
            };

            Some(ComplianceAuditSiemDeliveryAlertItemResponse {
                delivery_target: row.delivery_target,
                total_count: row.total_count,
                pending_count: row.pending_count,
                processing_count: row.processing_count,
                failed_count: row.failed_count,
                delivered_count: row.delivered_count,
                dead_lettered_count: row.dead_lettered_count,
                hard_failure_rate_pct,
                dead_letter_rate_pct,
                triggered_rules,
                severity: severity.to_string(),
                last_error: row.last_error,
                last_http_status: row.last_http_status,
                last_attempt_at: row.last_attempt_at,
                acknowledged: ack.is_some(),
                acknowledged_at: ack.map(|item| item.acknowledged_at),
                acknowledged_by_user_id: ack.map(|item| item.acknowledged_by_user_id),
                acknowledged_by_role: ack.map(|item| item.acknowledged_by_role.clone()),
                acknowledgement_note: ack.and_then(|item| item.note.clone()),
            })
        })
        .collect::<Vec<_>>();

    alerts.sort_by(|left, right| {
        let left_rank = if left.severity == "critical" { 0 } else { 1 };
        let right_rank = if right.severity == "critical" { 0 } else { 1 };
        left_rank
            .cmp(&right_rank)
            .then_with(|| right.triggered_rules.len().cmp(&left.triggered_rules.len()))
            .then_with(|| right.pending_count.cmp(&left.pending_count))
            .then_with(|| left.delivery_target.cmp(&right.delivery_target))
    });

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliveryAlertResponse {
            tenant_id,
            run_id: query.run_id,
            window_secs,
            since,
            thresholds: ComplianceAuditSiemDeliveryAlertThresholdsResponse {
                max_hard_failure_rate_pct: Some(max_hard_failure_rate_pct),
                max_dead_letter_rate_pct: Some(max_dead_letter_rate_pct),
                max_pending_count: Some(max_pending_count),
            },
            alerts,
        }),
    ))
}

async fn ack_compliance_audit_siem_delivery_alert_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AckComplianceAuditSiemDeliveryAlertRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let acknowledged_by_user_id = user_id_from_headers(&state, &headers)?.ok_or_else(|| {
        ApiError::forbidden("x-user-id header is required to acknowledge compliance alerts")
    })?;

    let delivery_target = req.delivery_target.trim();
    if delivery_target.is_empty() {
        return Err(ApiError::bad_request("delivery_target cannot be empty"));
    }

    let note = req
        .note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if note.as_ref().is_some_and(|value| value.len() > 2_000) {
        return Err(ApiError::bad_request("note must be <= 2000 characters"));
    }

    let run_scope = compliance_alert_run_scope(req.run_id);
    let ack = upsert_tenant_compliance_siem_delivery_alert_ack(
        &state.pool,
        &NewComplianceSiemDeliveryAlertAckRecord {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.clone(),
            run_scope: run_scope.clone(),
            delivery_target: delivery_target.to_string(),
            acknowledged_by_user_id,
            acknowledged_by_role: role_preset.as_str().to_string(),
            note,
        },
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed acknowledging siem delivery alert: {err}"))
    })?;

    let run_id = if ack.run_scope == "*" {
        None
    } else {
        Uuid::parse_str(ack.run_scope.as_str()).ok()
    };

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditSiemDeliveryAlertAckResponse {
            tenant_id: ack.tenant_id,
            run_scope: ack.run_scope,
            run_id,
            delivery_target: ack.delivery_target,
            acknowledged_by_user_id: ack.acknowledged_by_user_id,
            acknowledged_by_role: ack.acknowledged_by_role,
            acknowledgement_note: ack.note,
            created_at: ack.created_at,
            acknowledged_at: ack.acknowledged_at,
        }),
    ))
}

async fn replay_compliance_audit_siem_delivery_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(record_id): Path<Uuid>,
    Json(req): Json<ReplayComplianceAuditSiemDeliveryRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let retry_at = OffsetDateTime::now_utc()
        + time::Duration::seconds(req.delay_secs.unwrap_or(0).clamp(0, 86_400) as i64);
    let replayed = requeue_dead_letter_compliance_siem_delivery_record(
        &state.pool,
        tenant_id.as_str(),
        record_id,
        retry_at,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed replaying siem delivery row: {err}")))?;

    let Some(record) = replayed else {
        return Err(ApiError::not_found(
            "siem delivery row not found or not dead_lettered",
        ));
    };

    Ok((
        StatusCode::ACCEPTED,
        Json(ComplianceAuditSiemDeliveryItemResponse {
            id: record.id,
            tenant_id: record.tenant_id,
            run_id: record.run_id,
            adapter: record.adapter,
            delivery_target: record.delivery_target,
            status: record.status,
            attempts: record.attempts,
            max_attempts: record.max_attempts,
            next_attempt_at: record.next_attempt_at,
            leased_by: record.leased_by,
            lease_expires_at: record.lease_expires_at,
            last_error: record.last_error,
            last_http_status: record.last_http_status,
            created_at: record.created_at,
            updated_at: record.updated_at,
            delivered_at: record.delivered_at,
        }),
    ))
}

async fn get_compliance_audit_replay_package_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditReplayPackageQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let run = get_run_status(&state.pool, &tenant_id, query.run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading replay run: {err}")))?
        .ok_or_else(|| ApiError::not_found("run not found"))?;

    let audit_limit = query.audit_limit.unwrap_or(2000).clamp(1, 5000);
    let compliance_limit = query.compliance_limit.unwrap_or(2000).clamp(1, 5000);
    let payment_limit = query.payment_limit.unwrap_or(500).clamp(1, 2000);
    let include_payments = query.include_payments.unwrap_or(true);

    let run_audits = list_run_audit_events(&state.pool, &tenant_id, query.run_id, audit_limit)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading replay run audits: {err}")))?;
    let compliance_events = list_tenant_compliance_audit_events(
        &state.pool,
        &tenant_id,
        Some(query.run_id),
        None,
        compliance_limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed loading replay compliance audits: {err}")))?;

    let mut payment_rows = if include_payments {
        list_tenant_payment_ledger(
            &state.pool,
            tenant_id.as_str(),
            Some(query.run_id),
            None,
            None,
            None,
            None,
            payment_limit,
        )
        .await
        .map_err(|err| ApiError::internal(format!("failed loading replay payment ledger: {err}")))?
    } else {
        Vec::new()
    };
    payment_rows.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });

    let run_audit_events: Vec<AuditEventResponse> = run_audits
        .into_iter()
        .map(|event| AuditEventResponse {
            id: event.id,
            run_id: event.run_id,
            step_id: event.step_id,
            actor: event.actor,
            event_type: event.event_type,
            payload_json: event.payload_json,
            created_at: event.created_at,
        })
        .collect();

    let compliance_audit_events: Vec<ComplianceAuditEventResponse> = compliance_events
        .into_iter()
        .map(|event| ComplianceAuditEventResponse {
            id: event.id,
            source_audit_event_id: event.source_audit_event_id,
            tamper_chain_seq: event.tamper_chain_seq,
            tamper_prev_hash: event.tamper_prev_hash,
            tamper_hash: event.tamper_hash,
            run_id: event.run_id,
            step_id: event.step_id,
            tenant_id: event.tenant_id,
            agent_id: event.agent_id,
            user_id: event.user_id,
            actor: event.actor,
            event_type: event.event_type,
            payload_json: event.payload_json,
            request_id: event.request_id,
            session_id: event.session_id,
            action_request_id: event.action_request_id,
            payment_request_id: event.payment_request_id,
            created_at: event.created_at,
            recorded_at: event.recorded_at,
        })
        .collect();

    let payment_ledger: Vec<PaymentLedgerResponse> = payment_rows
        .into_iter()
        .map(|row| {
            let settlement_status = row
                .latest_result_json
                .as_ref()
                .and_then(|json| json.get("settlement_status"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let settlement_rail = row
                .latest_result_json
                .as_ref()
                .and_then(|json| json.get("rail"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| Some(row.provider.clone()));
            let normalized_outcome =
                normalize_payment_outcome(row.status.as_str(), row.latest_result_status.as_deref())
                    .to_string();
            let normalized_error_code = row
                .latest_error_json
                .as_ref()
                .and_then(|json| json.get("code"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let normalized_error_class = normalized_error_code
                .as_deref()
                .map(classify_payment_error_code)
                .map(ToString::to_string);
            PaymentLedgerResponse {
                id: row.id,
                action_request_id: row.action_request_id,
                run_id: row.run_id,
                tenant_id: row.tenant_id,
                agent_id: row.agent_id,
                provider: row.provider,
                operation: row.operation,
                destination: row.destination,
                idempotency_key: row.idempotency_key,
                amount_msat: row.amount_msat,
                status: row.status,
                request_json: row.request_json,
                latest_result_status: row.latest_result_status,
                latest_result_json: row.latest_result_json,
                latest_error_json: row.latest_error_json,
                settlement_status,
                settlement_rail,
                normalized_outcome,
                normalized_error_code,
                normalized_error_class,
                created_at: row.created_at,
                updated_at: row.updated_at,
                latest_result_created_at: row.latest_result_created_at,
            }
        })
        .collect();

    let mut timestamps: Vec<OffsetDateTime> = run_audit_events
        .iter()
        .map(|event| event.created_at)
        .chain(compliance_audit_events.iter().map(|event| event.created_at))
        .chain(payment_ledger.iter().map(|event| event.created_at))
        .collect();
    timestamps.sort();

    let correlation = ComplianceReplayCorrelationSummary {
        run_audit_event_count: run_audit_events.len(),
        compliance_event_count: compliance_audit_events.len(),
        payment_event_count: payment_ledger.len(),
        first_event_at: timestamps.first().copied(),
        last_event_at: timestamps.last().copied(),
    };
    let generated_at = OffsetDateTime::now_utc();
    let manifest = build_replay_manifest(
        tenant_id.as_str(),
        run.id,
        generated_at,
        run_audit_events.as_slice(),
        compliance_audit_events.as_slice(),
        payment_ledger.as_slice(),
        &correlation,
    )?;

    Ok((
        StatusCode::OK,
        Json(ComplianceAuditReplayPackageResponse {
            tenant_id,
            run: run_to_response(run),
            generated_at,
            run_audit_events,
            compliance_audit_events,
            payment_ledger,
            correlation,
            manifest,
        }),
    ))
}

async fn get_llm_usage_tokens_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LlmUsageQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let model_key = query
        .model_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let (tokens, estimated_cost_usd) = get_llm_usage_totals_since(
        &state.pool,
        tenant_id.as_str(),
        since,
        query.agent_id,
        model_key,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed querying llm usage totals: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(LlmUsageResponse {
            tenant_id,
            window_secs,
            since,
            tokens,
            estimated_cost_usd,
            agent_id: query.agent_id,
            model_key: model_key.map(ToString::to_string),
        }),
    ))
}

async fn get_ops_summary_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OpsSummaryQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let summary = get_tenant_ops_summary_dual(&state.db_pool, tenant_id.as_str(), since)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying ops summary: {err}")))?;
    let global_inflight_runs = count_inflight_runs_dual(&state.db_pool)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying global inflight runs: {err}")))?;
    let tenant_inflight_pressure =
        compute_pressure(summary.tenant_inflight_runs, state.tenant_max_inflight_runs);

    Ok((
        StatusCode::OK,
        Json(OpsSummaryResponse {
            tenant_id,
            window_secs,
            since,
            queued_runs: summary.queued_runs,
            running_runs: summary.running_runs,
            tenant_inflight_runs: summary.tenant_inflight_runs,
            tenant_inflight_pressure,
            tenant_inflight_cap: state.tenant_max_inflight_runs,
            global_inflight_runs,
            succeeded_runs_window: summary.succeeded_runs_window,
            failed_runs_window: summary.failed_runs_window,
            dead_letter_trigger_events_window: summary.dead_letter_trigger_events_window,
            avg_run_duration_ms: summary.avg_run_duration_ms,
            p95_run_duration_ms: summary.p95_run_duration_ms,
        }),
    ))
}

async fn get_ops_summary_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<OpsSummaryQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let summary = get_tenant_ops_summary_dual(&state.db_pool, tenant_id.as_str(), since)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying ops summary: {err}")))?;
    let global_inflight_runs = count_inflight_runs_dual(&state.db_pool)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying global inflight runs: {err}")))?;
    let tenant_inflight_pressure =
        compute_pressure(summary.tenant_inflight_runs, state.tenant_max_inflight_runs);

    Ok((
        StatusCode::OK,
        Json(OpsSummaryResponse {
            tenant_id,
            window_secs,
            since,
            queued_runs: summary.queued_runs,
            running_runs: summary.running_runs,
            tenant_inflight_runs: summary.tenant_inflight_runs,
            tenant_inflight_pressure,
            tenant_inflight_cap: state.tenant_max_inflight_runs,
            global_inflight_runs,
            succeeded_runs_window: summary.succeeded_runs_window,
            failed_runs_window: summary.failed_runs_window,
            dead_letter_trigger_events_window: summary.dead_letter_trigger_events_window,
            avg_run_duration_ms: summary.avg_run_duration_ms,
            p95_run_duration_ms: summary.p95_run_duration_ms,
        }),
    ))
}

async fn get_ops_llm_gateway_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<OpsLlmGatewayQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let since_text = since
        .format(&Rfc3339)
        .map_err(|err| ApiError::internal(format!("failed formatting llm gateway since: {err}")))?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT COALESCE(
                 NULLIF(CAST(json_extract(ar.result_json, '$.gateway.request_class') AS TEXT), ''),
                 'interactive'
               ) AS request_class,
               COALESCE(
                 NULLIF(CAST(json_extract(ar.result_json, '$.gateway.cache_status') AS TEXT), ''),
                 'unknown'
               ) AS cache_status,
               COALESCE(
                 NULLIF(CAST(json_extract(ar.result_json, '$.gateway.admission_status') AS TEXT), ''),
                 'unknown'
               ) AS admission_status,
               COALESCE(
                 NULLIF(CAST(json_extract(ar.result_json, '$.gateway.slo_status') AS TEXT), ''),
                 'not_configured'
               ) AS slo_status,
               CASE
                 WHEN lower(CAST(COALESCE(json_extract(ar.result_json, '$.gateway.verifier_escalated'), 'false') AS TEXT))
                      IN ('true', '1') THEN 1
                 ELSE 0
               END AS verifier_escalated,
               MAX(
                 (julianday(COALESCE(ar.executed_at, req.created_at)) - julianday(req.created_at)) * 86400000.0,
                 0.0
               ) AS duration_ms
        FROM action_requests req
        JOIN action_results ar ON ar.action_request_id = req.id
        JOIN steps s ON s.id = req.step_id
        JOIN runs r ON r.id = s.run_id
        WHERE r.tenant_id = ?1
          AND req.action_type = 'llm.infer'
          AND datetime(req.created_at) >= datetime(?2)
          AND ar.status = 'executed'
          AND ar.result_json IS NOT NULL
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(&since_text)
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying llm gateway summary: {err}")))?;

    #[derive(Default)]
    struct LaneAccumulator {
        total_count: i64,
        durations: Vec<f64>,
        cache_hit_count: i64,
        distributed_cache_hit_count: i64,
        verifier_escalated_count: i64,
        slo_warn_count: i64,
        slo_breach_count: i64,
        distributed_fail_open_count: i64,
    }

    let mut lanes: HashMap<String, LaneAccumulator> = HashMap::new();
    for row in rows {
        let request_class: String = row.get("request_class");
        let cache_status: String = row.get("cache_status");
        let admission_status: String = row.get("admission_status");
        let slo_status: String = row.get("slo_status");
        let verifier_escalated: i64 = row.get("verifier_escalated");
        let duration_ms = clamp_non_negative_duration_ms(row.get::<Option<f64>, _>("duration_ms"));
        let entry = lanes.entry(request_class).or_default();
        entry.total_count += 1;
        entry.durations.push(duration_ms);
        if cache_status == "hit" || cache_status == "distributed_hit" {
            entry.cache_hit_count += 1;
        }
        if cache_status == "distributed_hit" {
            entry.distributed_cache_hit_count += 1;
        }
        if verifier_escalated > 0 {
            entry.verifier_escalated_count += 1;
        }
        if slo_status == "warn" {
            entry.slo_warn_count += 1;
        }
        if slo_status == "breach" {
            entry.slo_breach_count += 1;
        }
        if admission_status == "distributed_fail_open_local" {
            entry.distributed_fail_open_count += 1;
        }
    }

    let mut lanes_sorted: Vec<_> = lanes.into_iter().collect();
    lanes_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let lane_responses: Vec<OpsLlmGatewayLaneResponse> = lanes_sorted
        .into_iter()
        .map(|(request_class, metrics)| {
            let avg_duration_ms = if metrics.total_count > 0 {
                Some(metrics.durations.iter().sum::<f64>() / metrics.total_count as f64)
            } else {
                None
            };
            let p95_duration_ms = percentile_95_ms(&metrics.durations);
            let cache_hit_rate_pct = if metrics.total_count > 0 {
                Some((metrics.cache_hit_count as f64 / metrics.total_count as f64) * 100.0)
            } else {
                None
            };
            let verifier_escalated_rate_pct = if metrics.total_count > 0 {
                Some((metrics.verifier_escalated_count as f64 / metrics.total_count as f64) * 100.0)
            } else {
                None
            };
            OpsLlmGatewayLaneResponse {
                request_class,
                total_count: metrics.total_count,
                avg_duration_ms,
                p95_duration_ms,
                cache_hit_count: metrics.cache_hit_count,
                distributed_cache_hit_count: metrics.distributed_cache_hit_count,
                cache_hit_rate_pct,
                verifier_escalated_count: metrics.verifier_escalated_count,
                verifier_escalated_rate_pct,
                slo_warn_count: metrics.slo_warn_count,
                slo_breach_count: metrics.slo_breach_count,
                distributed_fail_open_count: metrics.distributed_fail_open_count,
            }
        })
        .collect();
    let total_count: i64 = lane_responses.iter().map(|lane| lane.total_count).sum();

    Ok((
        StatusCode::OK,
        Json(OpsLlmGatewayResponse {
            tenant_id,
            window_secs,
            since,
            total_count,
            lanes: lane_responses,
        }),
    ))
}

async fn get_ops_latency_histogram_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<OpsLatencyHistogramQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let since_text = since.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!(
            "failed formatting ops latency histogram since timestamp: {err}"
        ))
    })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT MAX((julianday(finished_at) - julianday(started_at)) * 86400000.0, 0.0) AS duration_ms
        FROM runs
        WHERE tenant_id = ?1
          AND finished_at IS NOT NULL
          AND started_at IS NOT NULL
          AND datetime(finished_at) >= datetime(?2)
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(since_text)
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying ops latency histogram: {err}")))?;

    let mut buckets = vec![
        OpsLatencyHistogramBucketResponse {
            bucket_label: "0-499ms".to_string(),
            lower_bound_ms: 0,
            upper_bound_exclusive_ms: Some(500),
            run_count: 0,
        },
        OpsLatencyHistogramBucketResponse {
            bucket_label: "500-999ms".to_string(),
            lower_bound_ms: 500,
            upper_bound_exclusive_ms: Some(1000),
            run_count: 0,
        },
        OpsLatencyHistogramBucketResponse {
            bucket_label: "1000-1999ms".to_string(),
            lower_bound_ms: 1000,
            upper_bound_exclusive_ms: Some(2000),
            run_count: 0,
        },
        OpsLatencyHistogramBucketResponse {
            bucket_label: "2000-4999ms".to_string(),
            lower_bound_ms: 2000,
            upper_bound_exclusive_ms: Some(5000),
            run_count: 0,
        },
        OpsLatencyHistogramBucketResponse {
            bucket_label: "5000-9999ms".to_string(),
            lower_bound_ms: 5000,
            upper_bound_exclusive_ms: Some(10000),
            run_count: 0,
        },
        OpsLatencyHistogramBucketResponse {
            bucket_label: "10000ms+".to_string(),
            lower_bound_ms: 10000,
            upper_bound_exclusive_ms: None,
            run_count: 0,
        },
    ];
    for row in rows {
        let duration_ms =
            clamp_non_negative_duration_ms(row.get::<Option<f64>, _>("duration_ms")) as i64;
        let bucket_index = if duration_ms < 500 {
            0
        } else if duration_ms < 1000 {
            1
        } else if duration_ms < 2000 {
            2
        } else if duration_ms < 5000 {
            3
        } else if duration_ms < 10000 {
            4
        } else {
            5
        };
        buckets[bucket_index].run_count += 1;
    }

    Ok((
        StatusCode::OK,
        Json(OpsLatencyHistogramResponse {
            tenant_id,
            window_secs,
            since,
            buckets,
        }),
    ))
}

async fn get_ops_action_latency_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<OpsActionLatencyQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let since_text = since.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!(
            "failed formatting ops action latency since timestamp: {err}"
        ))
    })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT ar.action_type AS action_type,
               ar.status AS action_status,
               MAX(
                 (julianday(COALESCE(ar_latest.executed_at, ar.created_at)) - julianday(ar.created_at)) * 86400000.0,
                 0.0
               ) AS duration_ms
        FROM action_requests ar
        JOIN steps s ON s.id = ar.step_id
        JOIN runs r ON r.id = s.run_id
        LEFT JOIN (
          SELECT action_request_id, MAX(datetime(executed_at)) AS executed_at
          FROM action_results
          GROUP BY action_request_id
        ) ar_latest ON ar_latest.action_request_id = ar.id
        WHERE r.tenant_id = ?1
          AND datetime(ar.created_at) >= datetime(?2)
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(&since_text)
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying ops action latency: {err}")))?;

    #[derive(Default)]
    struct ActionAccumulator {
        total_count: i64,
        durations: Vec<f64>,
        max_duration_ms: Option<i64>,
        failed_count: i64,
        denied_count: i64,
    }

    let mut actions: HashMap<String, ActionAccumulator> = HashMap::new();
    for row in rows {
        let action_type: String = row.get("action_type");
        let action_status: String = row.get("action_status");
        let duration_ms = clamp_non_negative_duration_ms(row.get::<Option<f64>, _>("duration_ms"));
        let duration_ms_i64 = duration_ms.round() as i64;
        let entry = actions.entry(action_type).or_default();
        entry.total_count += 1;
        entry.durations.push(duration_ms);
        entry.max_duration_ms = Some(
            entry
                .max_duration_ms
                .map(|current| current.max(duration_ms_i64))
                .unwrap_or(duration_ms_i64),
        );
        if action_status == "failed" {
            entry.failed_count += 1;
        }
        if action_status == "denied" {
            entry.denied_count += 1;
        }
    }

    let mut action_entries: Vec<OpsActionLatencyEntryResponse> = actions
        .into_iter()
        .map(|(action_type, metrics)| {
            let avg_duration_ms = if metrics.total_count > 0 {
                Some(metrics.durations.iter().sum::<f64>() / metrics.total_count as f64)
            } else {
                None
            };
            OpsActionLatencyEntryResponse {
                action_type,
                total_count: metrics.total_count,
                avg_duration_ms,
                p95_duration_ms: percentile_95_ms(&metrics.durations),
                max_duration_ms: metrics.max_duration_ms,
                failed_count: metrics.failed_count,
                denied_count: metrics.denied_count,
            }
        })
        .collect();
    action_entries.sort_by(|a, b| {
        b.total_count
            .cmp(&a.total_count)
            .then_with(|| a.action_type.cmp(&b.action_type))
    });

    Ok((
        StatusCode::OK,
        Json(OpsActionLatencyResponse {
            tenant_id,
            window_secs,
            since,
            actions: action_entries,
        }),
    ))
}

async fn get_ops_action_latency_traces_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<OpsActionLatencyTracesQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let limit = query.limit.unwrap_or(500).clamp(1, 5000);
    let action_type = trim_non_empty(query.action_type.as_deref());
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let since_text = since.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!(
            "failed formatting ops action latency traces since: {err}"
        ))
    })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT ar.id AS action_request_id,
               s.run_id AS run_id,
               ar.step_id AS step_id,
               ar.action_type AS action_type,
               ar.status AS status,
               MAX(
                 (julianday(COALESCE(ar_latest.executed_at, ar.created_at)) - julianday(ar.created_at)) * 86400000.0,
                 0.0
               ) AS duration_ms,
               ar.created_at AS created_at,
               ar_latest.executed_at AS executed_at
        FROM action_requests ar
        JOIN steps s ON s.id = ar.step_id
        JOIN runs r ON r.id = s.run_id
        LEFT JOIN (
          SELECT action_request_id, MAX(datetime(executed_at)) AS executed_at
          FROM action_results
          GROUP BY action_request_id
        ) ar_latest ON ar_latest.action_request_id = ar.id
        WHERE r.tenant_id = ?1
          AND datetime(ar.created_at) >= datetime(?2)
          AND (?3 IS NULL OR ar.action_type = ?3)
        ORDER BY datetime(ar.created_at) DESC, ar.id DESC
        LIMIT ?4
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(&since_text)
    .bind(action_type)
    .bind(limit)
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying ops action latency traces: {err}")))?;

    let traces: Vec<OpsActionLatencyTraceEntryResponse> = rows
        .into_iter()
        .map(|row| {
            Ok(OpsActionLatencyTraceEntryResponse {
                action_request_id: parse_sqlite_uuid_required(&row, "action_request_id")?,
                run_id: parse_sqlite_uuid_required(&row, "run_id")?,
                step_id: parse_sqlite_uuid_required(&row, "step_id")?,
                action_type: row.get("action_type"),
                status: row.get("status"),
                duration_ms: clamp_non_negative_duration_ms(
                    row.get::<Option<f64>, _>("duration_ms"),
                )
                .round() as i64,
                created_at: parse_sqlite_datetime_required(&row, "created_at")?,
                executed_at: parse_sqlite_datetime_optional(&row, "executed_at")?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;

    Ok((
        StatusCode::OK,
        Json(OpsActionLatencyTracesResponse {
            tenant_id,
            window_secs,
            since,
            limit,
            action_type: action_type.map(ToString::to_string),
            traces,
        }),
    ))
}

async fn get_ops_latency_traces_sqlite_handler(
    State(state): State<SqliteAppState>,
    headers: HeaderMap,
    Query(query): Query<OpsLatencyTracesQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers_sqlite(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let limit = query.limit.unwrap_or(500).clamp(1, 5000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let since_text = since.format(&Rfc3339).map_err(|err| {
        ApiError::internal(format!("failed formatting ops latency traces since: {err}"))
    })?;
    let sqlite = sqlite_pool_from_db_pool(&state.db_pool)?;
    let rows = sqlx::query(
        r#"
        SELECT id AS run_id,
               status,
               MAX((julianday(finished_at) - julianday(started_at)) * 86400000.0, 0.0) AS duration_ms,
               started_at,
               finished_at
        FROM runs
        WHERE tenant_id = ?1
          AND finished_at IS NOT NULL
          AND started_at IS NOT NULL
          AND datetime(finished_at) >= datetime(?2)
        ORDER BY datetime(finished_at) DESC
        LIMIT ?3
        "#,
    )
    .bind(tenant_id.as_str())
    .bind(&since_text)
    .bind(limit)
    .fetch_all(sqlite)
    .await
    .map_err(|err| ApiError::internal(format!("failed querying ops latency traces: {err}")))?;

    let traces: Vec<OpsLatencyTraceResponse> = rows
        .into_iter()
        .map(|row| {
            Ok(OpsLatencyTraceResponse {
                run_id: parse_sqlite_uuid_required(&row, "run_id")?,
                status: row.get("status"),
                duration_ms: clamp_non_negative_duration_ms(
                    row.get::<Option<f64>, _>("duration_ms"),
                )
                .round() as i64,
                started_at: parse_sqlite_datetime_required(&row, "started_at")?,
                finished_at: parse_sqlite_datetime_required(&row, "finished_at")?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;

    Ok((
        StatusCode::OK,
        Json(OpsLatencyTracesResponse {
            tenant_id,
            window_secs,
            since,
            limit,
            traces,
        }),
    ))
}

async fn get_ops_llm_gateway_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OpsLlmGatewayQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let rows = get_tenant_llm_gateway_lane_summary(&state.pool, tenant_id.as_str(), since)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying llm gateway summary: {err}")))?;
    let total_count: i64 = rows.iter().map(|row| row.total_count).sum();

    Ok((
        StatusCode::OK,
        Json(OpsLlmGatewayResponse {
            tenant_id,
            window_secs,
            since,
            total_count,
            lanes: rows
                .into_iter()
                .map(|row| {
                    let cache_hit_rate_pct = if row.total_count > 0 {
                        Some((row.cache_hit_count as f64 / row.total_count as f64) * 100.0)
                    } else {
                        None
                    };
                    let verifier_escalated_rate_pct = if row.total_count > 0 {
                        Some((row.verifier_escalated_count as f64 / row.total_count as f64) * 100.0)
                    } else {
                        None
                    };
                    OpsLlmGatewayLaneResponse {
                        request_class: row.request_class,
                        total_count: row.total_count,
                        avg_duration_ms: row.avg_duration_ms,
                        p95_duration_ms: row.p95_duration_ms,
                        cache_hit_count: row.cache_hit_count,
                        distributed_cache_hit_count: row.distributed_cache_hit_count,
                        cache_hit_rate_pct,
                        verifier_escalated_count: row.verifier_escalated_count,
                        verifier_escalated_rate_pct,
                        slo_warn_count: row.slo_warn_count,
                        slo_breach_count: row.slo_breach_count,
                        distributed_fail_open_count: row.distributed_fail_open_count,
                    }
                })
                .collect(),
        }),
    ))
}

async fn get_ops_latency_histogram_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OpsLatencyHistogramQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let buckets = get_tenant_run_latency_histogram(&state.pool, tenant_id.as_str(), since)
        .await
        .map_err(|err| {
            ApiError::internal(format!("failed querying ops latency histogram: {err}"))
        })?;

    Ok((
        StatusCode::OK,
        Json(OpsLatencyHistogramResponse {
            tenant_id,
            window_secs,
            since,
            buckets: buckets
                .into_iter()
                .map(|bucket| OpsLatencyHistogramBucketResponse {
                    bucket_label: bucket.bucket_label,
                    lower_bound_ms: bucket.lower_bound_ms,
                    upper_bound_exclusive_ms: bucket.upper_bound_exclusive_ms,
                    run_count: bucket.run_count,
                })
                .collect(),
        }),
    ))
}

async fn get_ops_action_latency_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OpsActionLatencyQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let actions = get_tenant_action_latency_summary(&state.pool, tenant_id.as_str(), since)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying ops action latency: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(OpsActionLatencyResponse {
            tenant_id,
            window_secs,
            since,
            actions: actions
                .into_iter()
                .map(|action| OpsActionLatencyEntryResponse {
                    action_type: action.action_type,
                    total_count: action.total_count,
                    avg_duration_ms: action.avg_duration_ms,
                    p95_duration_ms: action.p95_duration_ms,
                    max_duration_ms: action.max_duration_ms,
                    failed_count: action.failed_count,
                    denied_count: action.denied_count,
                })
                .collect(),
        }),
    ))
}

async fn get_ops_action_latency_traces_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OpsActionLatencyTracesQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let limit = query.limit.unwrap_or(500).clamp(1, 5000);
    let action_type = trim_non_empty(query.action_type.as_deref());
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let traces = get_tenant_action_latency_traces(
        &state.pool,
        tenant_id.as_str(),
        since,
        action_type,
        limit,
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed querying ops action latency traces: {err}"))
    })?;

    Ok((
        StatusCode::OK,
        Json(OpsActionLatencyTracesResponse {
            tenant_id,
            window_secs,
            since,
            limit,
            action_type: action_type.map(ToString::to_string),
            traces: traces
                .into_iter()
                .map(|trace| OpsActionLatencyTraceEntryResponse {
                    action_request_id: trace.action_request_id,
                    run_id: trace.run_id,
                    step_id: trace.step_id,
                    action_type: trace.action_type,
                    status: trace.status,
                    duration_ms: trace.duration_ms,
                    created_at: trace.created_at,
                    executed_at: trace.executed_at,
                })
                .collect(),
        }),
    ))
}

async fn get_ops_latency_traces_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OpsLatencyTracesQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let limit = query.limit.unwrap_or(500).clamp(1, 5000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let traces = get_tenant_run_latency_traces(&state.pool, tenant_id.as_str(), since, limit)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying ops latency traces: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(OpsLatencyTracesResponse {
            tenant_id,
            window_secs,
            since,
            limit,
            traces: traces
                .into_iter()
                .map(|trace| OpsLatencyTraceResponse {
                    run_id: trace.run_id,
                    status: trace.status,
                    duration_ms: trace.duration_ms,
                    started_at: trace.started_at,
                    finished_at: trace.finished_at,
                })
                .collect(),
        }),
    ))
}

fn normalize_payment_outcome(
    request_status: &str,
    latest_result_status: Option<&str>,
) -> &'static str {
    if request_status == "duplicate" {
        return "duplicate";
    }
    match latest_result_status {
        Some("executed") => "executed",
        Some("failed") => "failed",
        Some("duplicate") => "duplicate",
        Some(_) => "unknown",
        None => match request_status {
            "executed" => "executed",
            "failed" => "failed",
            "requested" => "requested",
            "duplicate" => "duplicate",
            _ => "unknown",
        },
    }
}

fn clamp_non_negative_duration_ms(duration_ms: Option<f64>) -> f64 {
    duration_ms.unwrap_or(0.0).max(0.0)
}

fn percentile_95_ms(durations: &[f64]) -> Option<f64> {
    if durations.is_empty() {
        return None;
    }
    let mut sorted = durations.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((sorted.len() as f64) * 0.95).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    Some(sorted[index])
}

fn classify_payment_error_code(code: &str) -> &'static str {
    if code.contains("BUDGET_EXCEEDED") {
        "budget_limit"
    } else if code.contains("APPROVAL_REQUIRED") {
        "approval_required"
    } else if code.contains("WALLET_NOT_CONFIGURED")
        || code.contains("MINT_NOT_CONFIGURED")
        || code.contains("MINTS_NOT_CONFIGURED")
        || code.contains("INVALID_DESTINATION")
    {
        "configuration"
    } else if code.contains("HTTP_DISABLED") || code.contains("DISABLED") {
        "disabled"
    } else if code.contains("REQUEST_FAILED")
        || code.contains("HTTP_FAILED")
        || code.contains("RESPONSE_ERROR")
    {
        "upstream_failure"
    } else {
        "unknown"
    }
}

async fn get_payments_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PaymentLedgerQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);

    let status = trim_non_empty(query.status.as_deref());
    let destination = trim_non_empty(query.destination.as_deref());
    let idempotency_key = trim_non_empty(query.idempotency_key.as_deref());
    let rows = list_tenant_payment_ledger(
        &state.pool,
        tenant_id.as_str(),
        query.run_id,
        query.agent_id,
        status,
        destination,
        idempotency_key,
        limit,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed querying payment ledger: {err}")))?;

    let body: Vec<PaymentLedgerResponse> = rows
        .into_iter()
        .map(|row| {
            let settlement_status = row
                .latest_result_json
                .as_ref()
                .and_then(|json| json.get("settlement_status"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let settlement_rail = row
                .latest_result_json
                .as_ref()
                .and_then(|json| json.get("rail"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| Some(row.provider.clone()));
            let normalized_outcome =
                normalize_payment_outcome(row.status.as_str(), row.latest_result_status.as_deref())
                    .to_string();
            let normalized_error_code = row
                .latest_error_json
                .as_ref()
                .and_then(|json| json.get("code"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let normalized_error_class = normalized_error_code
                .as_deref()
                .map(classify_payment_error_code)
                .map(ToString::to_string);
            PaymentLedgerResponse {
                id: row.id,
                action_request_id: row.action_request_id,
                run_id: row.run_id,
                tenant_id: row.tenant_id,
                agent_id: row.agent_id,
                provider: row.provider,
                operation: row.operation,
                destination: row.destination,
                idempotency_key: row.idempotency_key,
                amount_msat: row.amount_msat,
                status: row.status,
                request_json: row.request_json,
                latest_result_status: row.latest_result_status,
                latest_result_json: row.latest_result_json,
                latest_error_json: row.latest_error_json,
                settlement_status,
                settlement_rail,
                normalized_outcome,
                normalized_error_code,
                normalized_error_class,
                created_at: row.created_at,
                updated_at: row.updated_at,
                latest_result_created_at: row.latest_result_created_at,
            }
        })
        .collect();

    Ok((StatusCode::OK, Json(body)))
}

async fn get_payment_summary_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PaymentSummaryQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.map(|value| value.clamp(1, 31_536_000));
    let since = window_secs
        .map(|seconds| OffsetDateTime::now_utc() - time::Duration::seconds(seconds as i64));
    let operation = trim_non_empty(query.operation.as_deref());
    if let Some(value) = operation {
        let is_valid = matches!(value, "pay_invoice" | "make_invoice" | "get_balance");
        if !is_valid {
            return Err(ApiError::bad_request(
                "operation must be one of: pay_invoice, make_invoice, get_balance",
            ));
        }
    }

    let summary = get_tenant_payment_summary(
        &state.pool,
        tenant_id.as_str(),
        since,
        query.agent_id,
        operation,
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed querying payment summary: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(PaymentSummaryResponse {
            tenant_id,
            window_secs,
            since,
            agent_id: query.agent_id,
            operation: operation.map(ToString::to_string),
            total_requests: summary.total_requests,
            requested_count: summary.requested_count,
            executed_count: summary.executed_count,
            failed_count: summary.failed_count,
            duplicate_count: summary.duplicate_count,
            executed_spend_msat: summary.executed_spend_msat,
        }),
    ))
}

fn tenant_from_headers(headers: &HeaderMap) -> ApiResult<String> {
    let raw = headers
        .get(TENANT_HEADER)
        .ok_or_else(|| ApiError::bad_request("missing x-tenant-id header"))?;
    let value = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("x-tenant-id header is not valid UTF-8"))?
        .trim();

    if value.is_empty() {
        return Err(ApiError::bad_request(
            "x-tenant-id header must not be empty",
        ));
    }

    Ok(value.to_string())
}

fn shared_secret_resolver() -> &'static CachedSecretResolver<CliSecretResolver> {
    static RESOLVER: OnceLock<CachedSecretResolver<CliSecretResolver>> = OnceLock::new();
    RESOLVER.get_or_init(|| CachedSecretResolver::from_env_with(CliSecretResolver::from_env()))
}

#[derive(Debug, Clone, Copy)]
enum RolePreset {
    Owner,
    Operator,
    Viewer,
}

impl RolePreset {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "owner" => Some(Self::Owner),
            "operator" => Some(Self::Operator),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
        }
    }
}

fn enforce_trusted_proxy_auth(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    if !state.trusted_proxy_auth_enabled {
        return Ok(());
    }

    if let Some(error) = state.trusted_proxy_auth_error.as_deref() {
        return Err(ApiError::internal(format!(
            "trusted proxy auth is enabled but secret resolution failed: {error}"
        )));
    }

    let expected = state
        .trusted_proxy_auth_secret
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApiError::internal("trusted proxy auth is enabled but no shared secret is configured")
        })?;

    let provided = headers
        .get(AUTH_PROXY_TOKEN_HEADER)
        .ok_or_else(|| ApiError::unauthorized("missing x-auth-proxy-token header"))?
        .to_str()
        .map_err(|_| ApiError::bad_request("x-auth-proxy-token header is not valid UTF-8"))?;

    if provided != expected {
        return Err(ApiError::unauthorized(
            "trusted proxy token validation failed",
        ));
    }

    Ok(())
}

fn role_from_headers(state: &AppState, headers: &HeaderMap) -> ApiResult<RolePreset> {
    enforce_trusted_proxy_auth(state, headers)?;
    let Some(raw) = headers.get(ROLE_HEADER) else {
        return Ok(RolePreset::Owner);
    };
    let value = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("x-user-role header is not valid UTF-8"))?;
    RolePreset::parse(value)
        .ok_or_else(|| ApiError::bad_request("x-user-role must be one of: owner, operator, viewer"))
}

fn enforce_trusted_proxy_auth_sqlite(state: &SqliteAppState, headers: &HeaderMap) -> ApiResult<()> {
    if !state.trusted_proxy_auth_enabled {
        return Ok(());
    }

    if let Some(error) = state.trusted_proxy_auth_error.as_deref() {
        return Err(ApiError::internal(format!(
            "trusted proxy auth is enabled but secret resolution failed: {error}"
        )));
    }

    let expected = state
        .trusted_proxy_auth_secret
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApiError::internal("trusted proxy auth is enabled but no shared secret is configured")
        })?;

    let provided = headers
        .get(AUTH_PROXY_TOKEN_HEADER)
        .ok_or_else(|| ApiError::unauthorized("missing x-auth-proxy-token header"))?
        .to_str()
        .map_err(|_| ApiError::bad_request("x-auth-proxy-token header is not valid UTF-8"))?;

    if provided != expected {
        return Err(ApiError::unauthorized(
            "trusted proxy token validation failed",
        ));
    }

    Ok(())
}

fn role_from_headers_sqlite(state: &SqliteAppState, headers: &HeaderMap) -> ApiResult<RolePreset> {
    enforce_trusted_proxy_auth_sqlite(state, headers)?;
    let Some(raw) = headers.get(ROLE_HEADER) else {
        return Ok(RolePreset::Owner);
    };
    let value = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("x-user-role header is not valid UTF-8"))?;
    RolePreset::parse(value)
        .ok_or_else(|| ApiError::bad_request("x-user-role must be one of: owner, operator, viewer"))
}

fn trim_non_empty(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
}

fn compliance_alert_run_scope(run_id: Option<Uuid>) -> String {
    run_id
        .map(|value| value.to_string())
        .unwrap_or_else(|| "*".to_string())
}

fn user_id_from_headers(state: &AppState, headers: &HeaderMap) -> ApiResult<Option<Uuid>> {
    enforce_trusted_proxy_auth(state, headers)?;
    let Some(raw) = headers.get(USER_ID_HEADER) else {
        return Ok(None);
    };
    let value = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("x-user-id header is not valid UTF-8"))?;
    let parsed = Uuid::parse_str(value.trim())
        .map_err(|_| ApiError::bad_request("x-user-id header must be a valid UUID"))?;
    Ok(Some(parsed))
}

fn ensure_trigger_mutation_role(role_preset: RolePreset) -> ApiResult<()> {
    if matches!(role_preset, RolePreset::Viewer) {
        return Err(ApiError::forbidden("viewer role cannot mutate triggers"));
    }
    Ok(())
}

fn map_trigger_enqueue_unavailable_reason(
    reason: TriggerEventEnqueueUnavailableReason,
) -> ApiError {
    match reason {
        TriggerEventEnqueueUnavailableReason::TriggerNotFound => {
            ApiError::not_found("trigger not found")
        }
        TriggerEventEnqueueUnavailableReason::TriggerDisabled
        | TriggerEventEnqueueUnavailableReason::TriggerScheduleBroken => {
            ApiError::conflict("trigger is not enabled")
        }
        TriggerEventEnqueueUnavailableReason::TriggerTypeMismatch => {
            ApiError::bad_request("trigger does not accept webhook events")
        }
        TriggerEventEnqueueUnavailableReason::PayloadMalformed => {
            ApiError::bad_request("trigger event payload must be a JSON object")
        }
    }
}

fn compute_pressure(active: i64, configured_cap: Option<i64>) -> Option<f64> {
    let cap = configured_cap?;
    if cap <= 0 {
        return None;
    }
    Some(active as f64 / cap as f64)
}

fn ensure_usage_query_role(role_preset: RolePreset) -> ApiResult<()> {
    if matches!(role_preset, RolePreset::Viewer) {
        return Err(ApiError::forbidden(
            "viewer role cannot query reporting endpoints",
        ));
    }
    Ok(())
}

fn ensure_owner_role(role_preset: RolePreset, message: &'static str) -> ApiResult<()> {
    if matches!(role_preset, RolePreset::Owner) {
        return Ok(());
    }
    Err(ApiError::forbidden(message))
}

fn ensure_memory_write_role(role_preset: RolePreset) -> ApiResult<()> {
    if matches!(role_preset, RolePreset::Viewer) {
        return Err(ApiError::forbidden("viewer role cannot write memory"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct BootstrapCompletionRecord {
    completed_at: OffsetDateTime,
    completed_by_user_id: Uuid,
    completion_note: Option<String>,
    updated_files: Vec<String>,
}

fn find_latest_bootstrap_completion(
    session_files: &[agent_core::AgentContextFile],
) -> ApiResult<Option<BootstrapCompletionRecord>> {
    let Some(status_file) = session_files.iter().find(|file| {
        file.relative_path
            .eq_ignore_ascii_case(BOOTSTRAP_STATUS_FILE_PATH)
    }) else {
        return Ok(None);
    };

    let mut latest = None;
    for line in status_file.content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed).map_err(|err| {
            ApiError::internal(format!(
                "failed decoding bootstrap status line in {}: {err}",
                BOOTSTRAP_STATUS_FILE_PATH
            ))
        })?;
        if value.get("status").and_then(Value::as_str) != Some("completed") {
            continue;
        }
        let completed_at_raw = value
            .get("completed_at")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ApiError::internal("bootstrap completion status is missing completed_at timestamp")
            })?;
        let completed_at = OffsetDateTime::parse(
            completed_at_raw,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|err| {
            ApiError::internal(format!(
                "invalid bootstrap completion timestamp `{completed_at_raw}`: {err}"
            ))
        })?;
        let completed_by_user_id_raw = value
            .get("completed_by_user_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ApiError::internal("bootstrap completion status is missing completed_by_user_id")
            })?;
        let completed_by_user_id = Uuid::parse_str(completed_by_user_id_raw).map_err(|_| {
            ApiError::internal(format!(
                "invalid bootstrap completion user id `{completed_by_user_id_raw}`"
            ))
        })?;
        let completion_note = value
            .get("completion_note")
            .and_then(Value::as_str)
            .map(str::to_string);
        let updated_files = value
            .get("updated_files")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        latest = Some(BootstrapCompletionRecord {
            completed_at,
            completed_by_user_id,
            completion_note,
            updated_files,
        });
    }

    Ok(latest)
}

#[derive(Debug, Clone, Copy)]
enum ContextMutationMode {
    Replace,
    Append,
}

impl ContextMutationMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }
}

fn parse_context_mutation_mode(raw: &str) -> ApiResult<ContextMutationMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "replace" => Ok(ContextMutationMode::Replace),
        "append" => Ok(ContextMutationMode::Append),
        _ => Err(ApiError::bad_request(
            "mode must be one of: replace, append",
        )),
    }
}

fn normalize_optional_text(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
}

fn normalize_context_mutation_path(raw: &str) -> ApiResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request("relative_path must not be empty"));
    }
    let candidate = StdPath::new(trimmed);
    if candidate.is_absolute() {
        return Err(ApiError::bad_request(
            "relative_path must be a relative path",
        ));
    }
    let mut parts = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => {
                let value = part.to_string_lossy();
                if value.trim().is_empty() {
                    return Err(ApiError::bad_request(
                        "relative_path contains empty path component",
                    ));
                }
                parts.push(value.to_string());
            }
            _ => {
                return Err(ApiError::bad_request(
                    "relative_path contains invalid path components",
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(ApiError::bad_request("relative_path must not be empty"));
    }
    Ok(parts.join("/"))
}

fn ensure_context_mutation_role(
    role_preset: RolePreset,
    mutability: AgentContextMutability,
) -> ApiResult<()> {
    match mutability {
        AgentContextMutability::Immutable => Err(ApiError::forbidden(
            "immutable agent-context files cannot be mutated via API",
        )),
        AgentContextMutability::HumanPrimary => ensure_owner_role(
            role_preset,
            "only owner role can mutate human-primary agent-context files",
        ),
        AgentContextMutability::AgentManaged => ensure_memory_write_role(role_preset),
    }
}

fn validate_context_mutation_mode(
    mode: ContextMutationMode,
    relative_path: &str,
    mutability: AgentContextMutability,
) -> ApiResult<()> {
    if relative_path.starts_with("sessions/") && !relative_path.ends_with(".jsonl") {
        return Err(ApiError::bad_request(
            "sessions/ context files must use .jsonl extension",
        ));
    }
    if relative_path.starts_with("memory/") && !relative_path.ends_with(".md") {
        return Err(ApiError::bad_request(
            "memory/ context files must use .md extension",
        ));
    }

    if matches!(
        mutability,
        AgentContextMutability::HumanPrimary | AgentContextMutability::Immutable
    ) && matches!(mode, ContextMutationMode::Append)
    {
        return Err(ApiError::bad_request(
            "append mode is not allowed for immutable or human-primary files",
        ));
    }
    if relative_path.starts_with("sessions/") && !matches!(mode, ContextMutationMode::Append) {
        return Err(ApiError::bad_request(
            "sessions/*.jsonl mutations must use append mode",
        ));
    }

    Ok(())
}

fn resolve_or_create_agent_context_source_dir(
    config: &AgentContextLoaderConfig,
    tenant_id: &str,
    agent_id: Uuid,
) -> ApiResult<PathBuf> {
    let tenant_agent = config
        .root_dir
        .join(tenant_id.trim())
        .join(agent_id.to_string());
    if tenant_agent.is_dir() {
        return Ok(tenant_agent);
    }
    let flat_agent = config.root_dir.join(agent_id.to_string());
    if flat_agent.is_dir() {
        return Ok(flat_agent);
    }

    fs::create_dir_all(&tenant_agent).map_err(|err| {
        ApiError::internal(format!(
            "failed creating agent context directory {}: {err}",
            tenant_agent.display()
        ))
    })?;
    Ok(tenant_agent)
}

fn append_context_jsonl_line(
    source_dir: &StdPath,
    relative_path: &str,
    line: &str,
    max_file_bytes: usize,
) -> ApiResult<()> {
    let full_path = source_dir.join(relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ApiError::internal(format!(
                "failed creating context file directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let mut payload = line.to_string();
    if !payload.ends_with('\n') {
        payload.push('\n');
    }
    let current_len = fs::metadata(&full_path)
        .map(|meta| meta.len() as usize)
        .unwrap_or(0usize);
    let projected_len = current_len.saturating_add(payload.as_bytes().len());
    if projected_len > max_file_bytes {
        return Err(ApiError::bad_request(format!(
            "{} exceeds max file size ({} bytes)",
            relative_path, max_file_bytes
        )));
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&full_path)
        .map_err(|err| {
            ApiError::internal(format!(
                "failed opening context file {} for append: {err}",
                full_path.display()
            ))
        })?;
    file.write_all(payload.as_bytes()).map_err(|err| {
        ApiError::internal(format!(
            "failed appending context file {}: {err}",
            full_path.display()
        ))
    })?;
    Ok(())
}

fn map_agent_context_load_error(err: AgentContextLoadError) -> ApiError {
    match err {
        AgentContextLoadError::NotFound { searched_paths } => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "NOT_FOUND",
            message: format!(
                "agent context not found; searched: {}",
                searched_paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
        AgentContextLoadError::InvalidConfig { message } => {
            ApiError::internal(format!("agent context loader is misconfigured: {message}"))
        }
        AgentContextLoadError::Io { path, source } => ApiError::internal(format!(
            "failed reading agent context at {}: {source}",
            path.display()
        )),
    }
}

fn agent_context_precedence_order() -> Vec<String> {
    vec![
        "runtime policy enforcement".to_string(),
        "AGENTS.md + TOOLS.md".to_string(),
        "IDENTITY.md + SOUL.md".to_string(),
        "USER.md".to_string(),
        "MEMORY.md".to_string(),
        "memory/*.md + sessions/*.jsonl".to_string(),
    ]
}

fn agent_context_file_to_response(file: &agent_core::AgentContextFile) -> AgentContextFileResponse {
    AgentContextFileResponse {
        slot: file.slot.clone(),
        relative_path: file.relative_path.clone(),
        sha256: file.sha256.clone(),
        bytes: file.bytes,
        mutability: classify_agent_context_mutability(file.relative_path.as_str()),
    }
}

fn resolve_trigger_actor_for_create(
    role_preset: RolePreset,
    actor_user_id: Option<Uuid>,
    requested_triggered_by_user_id: Option<Uuid>,
) -> ApiResult<Option<Uuid>> {
    match role_preset {
        RolePreset::Owner => Ok(requested_triggered_by_user_id),
        RolePreset::Operator => {
            let actor = actor_user_id.ok_or_else(|| {
                ApiError::forbidden("operator role requires x-user-id for trigger mutation")
            })?;
            if let Some(requested) = requested_triggered_by_user_id {
                if requested != actor {
                    return Err(ApiError::forbidden(
                        "operator can only create triggers for self",
                    ));
                }
            }
            Ok(Some(actor))
        }
        RolePreset::Viewer => Err(ApiError::forbidden("viewer role cannot mutate triggers")),
    }
}

async fn find_existing_heartbeat_trigger_id(
    pool: &PgPool,
    tenant_id: &str,
    agent_id: Uuid,
    candidate: &agent_core::HeartbeatTriggerCandidate,
) -> ApiResult<Option<Uuid>> {
    let existing = match candidate.kind {
        agent_core::HeartbeatIntentKind::Interval => {
            let interval_seconds = candidate.interval_seconds.ok_or_else(|| {
                ApiError::internal("heartbeat interval candidate missing interval seconds")
            })?;
            sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT id
                FROM triggers
                WHERE tenant_id = $1
                  AND agent_id = $2
                  AND trigger_type = 'interval'
                  AND recipe_id = $3
                  AND interval_seconds = $4
                  AND max_inflight_runs = $5
                  AND jitter_seconds = $6
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            )
            .bind(tenant_id)
            .bind(agent_id)
            .bind(candidate.recipe_id.as_str())
            .bind(interval_seconds)
            .bind(candidate.max_inflight_runs)
            .bind(candidate.jitter_seconds)
            .fetch_optional(pool)
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed checking existing interval heartbeat trigger: {err}"
                ))
            })?
        }
        agent_core::HeartbeatIntentKind::Cron => {
            let cron_expression = candidate.cron_expression.as_deref().ok_or_else(|| {
                ApiError::internal("heartbeat cron candidate missing cron expression")
            })?;
            let timezone = candidate.timezone.as_deref().unwrap_or("UTC");
            sqlx::query_scalar::<_, Uuid>(
                r#"
                SELECT id
                FROM triggers
                WHERE tenant_id = $1
                  AND agent_id = $2
                  AND trigger_type = 'cron'
                  AND recipe_id = $3
                  AND cron_expression = $4
                  AND schedule_timezone = $5
                  AND max_inflight_runs = $6
                  AND jitter_seconds = $7
                ORDER BY created_at DESC
                LIMIT 1
                "#,
            )
            .bind(tenant_id)
            .bind(agent_id)
            .bind(candidate.recipe_id.as_str())
            .bind(cron_expression)
            .bind(timezone)
            .bind(candidate.max_inflight_runs)
            .bind(candidate.jitter_seconds)
            .fetch_optional(pool)
            .await
            .map_err(|err| {
                ApiError::internal(format!(
                    "failed checking existing cron heartbeat trigger: {err}"
                ))
            })?
        }
    };

    Ok(existing)
}

fn ensure_trigger_operator_ownership(
    role_preset: RolePreset,
    actor_user_id: Option<Uuid>,
    trigger_owner_user_id: Option<Uuid>,
) -> ApiResult<()> {
    if matches!(role_preset, RolePreset::Operator) {
        let actor = actor_user_id.ok_or_else(|| {
            ApiError::forbidden("operator role requires x-user-id for trigger mutation")
        })?;
        if trigger_owner_user_id != Some(actor) {
            return Err(ApiError::forbidden(
                "operator cannot mutate triggers owned by another user",
            ));
        }
    }
    Ok(())
}

async fn set_trigger_status_handler(
    state: AppState,
    headers: HeaderMap,
    trigger_id: Uuid,
    status: &str,
    audit_event_type: &str,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&state, &headers)?;
    let actor_user_id = user_id_from_headers(&state, &headers)?;
    ensure_trigger_mutation_role(role_preset)?;
    let Some(existing) = get_trigger(&state.pool, tenant_id.as_str(), trigger_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading trigger: {err}")))?
    else {
        return Err(ApiError::not_found("trigger not found"));
    };
    ensure_trigger_operator_ownership(role_preset, actor_user_id, existing.triggered_by_user_id)?;

    let updated = update_trigger_status(&state.pool, tenant_id.as_str(), trigger_id, status)
        .await
        .map_err(|err| ApiError::internal(format!("failed updating trigger status: {err}")))?
        .ok_or_else(|| ApiError::not_found("trigger not found"))?;

    append_trigger_audit(
        &state.pool,
        &tenant_id,
        trigger_id,
        role_preset,
        audit_event_type,
        json!({
            "status": updated.status,
            "trigger_type": updated.trigger_type,
        }),
    )
    .await?;

    Ok((StatusCode::OK, Json(trigger_to_response(updated))))
}

async fn append_trigger_audit(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    role_preset: RolePreset,
    event_type: &str,
    payload_json: Value,
) -> ApiResult<()> {
    append_trigger_audit_event(
        pool,
        &NewTriggerAuditEvent {
            id: Uuid::new_v4(),
            trigger_id,
            tenant_id: tenant_id.to_string(),
            actor: "api".to_string(),
            event_type: event_type.to_string(),
            payload_json: json!({
                "role_preset": role_preset.as_str(),
                "details": payload_json,
            }),
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed appending trigger audit event: {err}")))?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum SiemAdapter {
    SecureAgntNdjson,
    SplunkHec,
    ElasticBulk,
}

impl SiemAdapter {
    fn parse(raw: Option<&str>) -> ApiResult<Self> {
        let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Self::SecureAgntNdjson);
        };

        match value.to_ascii_lowercase().as_str() {
            "secureagnt_ndjson" | "ndjson" => Ok(Self::SecureAgntNdjson),
            "splunk_hec" => Ok(Self::SplunkHec),
            "elastic_bulk" => Ok(Self::ElasticBulk),
            _ => Err(ApiError::bad_request(
                "adapter must be one of: secureagnt_ndjson, splunk_hec, elastic_bulk",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SecureAgntNdjson => "secureagnt_ndjson",
            Self::SplunkHec => "splunk_hec",
            Self::ElasticBulk => "elastic_bulk",
        }
    }
}

fn serialize_siem_adapter_payload(
    events: &[agent_core::ComplianceAuditEventDetailRecord],
    adapter: SiemAdapter,
    elastic_index: &str,
) -> ApiResult<String> {
    match adapter {
        SiemAdapter::SecureAgntNdjson => serialize_compliance_events_as_ndjson(events),
        SiemAdapter::SplunkHec => serialize_compliance_events_as_splunk_hec(events),
        SiemAdapter::ElasticBulk => {
            serialize_compliance_events_as_elastic_bulk(events, elastic_index)
        }
    }
}

fn parse_siem_outbox_status(raw: Option<&str>) -> ApiResult<Option<&str>> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    match value {
        "pending" | "processing" | "failed" | "delivered" | "dead_lettered" => Ok(Some(value)),
        _ => Err(ApiError::bad_request(
            "status must be one of: pending, processing, failed, delivered, dead_lettered",
        )),
    }
}

fn build_replay_manifest(
    tenant_id: &str,
    run_id: Uuid,
    generated_at: OffsetDateTime,
    run_audit_events: &[AuditEventResponse],
    compliance_audit_events: &[ComplianceAuditEventResponse],
    payment_ledger: &[PaymentLedgerResponse],
    correlation: &ComplianceReplayCorrelationSummary,
) -> ApiResult<ComplianceReplayPackageManifest> {
    let manifest_json = json!({
        "tenant_id": tenant_id,
        "run_id": run_id,
        "generated_at": generated_at,
        "run_audit_event_ids": run_audit_events.iter().map(|event| event.id.to_string()).collect::<Vec<_>>(),
        "compliance_audit_event_ids": compliance_audit_events.iter().map(|event| event.id.to_string()).collect::<Vec<_>>(),
        "payment_request_ids": payment_ledger.iter().map(|row| row.id.to_string()).collect::<Vec<_>>(),
        "correlation": correlation,
    });
    let canonical_bytes = serde_json::to_vec(&manifest_json).map_err(|err| {
        ApiError::internal(format!("failed serializing replay manifest payload: {err}"))
    })?;
    let digest_sha256 = digest_sha256_hex(canonical_bytes.as_slice());

    let signing_key = resolve_replay_manifest_signing_key()?;
    let (signing_mode, signature) = match signing_key {
        Some(key) => (
            "hmac-sha256".to_string(),
            Some(hmac_sha256_hex(key.as_bytes(), canonical_bytes.as_slice())),
        ),
        None => ("unsigned".to_string(), None),
    };

    Ok(ComplianceReplayPackageManifest {
        version: "v1".to_string(),
        digest_sha256,
        signing_mode,
        signature,
    })
}

fn resolve_replay_manifest_signing_key() -> ApiResult<Option<String>> {
    let inline = env::var("COMPLIANCE_REPLAY_SIGNING_KEY").ok();
    let reference = env::var("COMPLIANCE_REPLAY_SIGNING_KEY_REF").ok();
    let resolved =
        resolve_secret_value(inline, reference, shared_secret_resolver()).map_err(|err| {
            ApiError::internal(format!(
                "failed resolving replay manifest signing key: {err}"
            ))
        })?;
    Ok(resolved
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

fn digest_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_encode(digest.as_slice())
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = Sha256::digest(key);
        key_block[..hashed.len()].copy_from_slice(hashed.as_slice());
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut o_key_pad = [0x5c_u8; BLOCK_SIZE];
    let mut i_key_pad = [0x36_u8; BLOCK_SIZE];
    for (idx, byte) in key_block.iter().enumerate() {
        o_key_pad[idx] ^= *byte;
        i_key_pad[idx] ^= *byte;
    }

    let mut inner = Sha256::new();
    inner.update(i_key_pad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(o_key_pad);
    outer.update(inner_digest);
    hex_encode(outer.finalize().as_slice())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02x}")).collect()
}

fn compliance_event_to_json_value(event: &agent_core::ComplianceAuditEventDetailRecord) -> Value {
    json!({
        "id": event.id,
        "source_audit_event_id": event.source_audit_event_id,
        "tamper_chain_seq": event.tamper_chain_seq,
        "tamper_prev_hash": event.tamper_prev_hash,
        "tamper_hash": event.tamper_hash,
        "run_id": event.run_id,
        "step_id": event.step_id,
        "tenant_id": event.tenant_id,
        "agent_id": event.agent_id,
        "user_id": event.user_id,
        "actor": event.actor,
        "event_type": event.event_type,
        "payload_json": event.payload_json,
        "created_at": event.created_at,
        "recorded_at": event.recorded_at,
    })
}

fn serialize_compliance_events_as_ndjson(
    events: &[agent_core::ComplianceAuditEventDetailRecord],
) -> ApiResult<String> {
    let mut payload = String::new();
    for event in events {
        let line =
            serde_json::to_string(&compliance_event_to_json_value(event)).map_err(|err| {
                ApiError::internal(format!("failed serializing compliance export row: {err}"))
            })?;
        payload.push_str(&line);
        payload.push('\n');
    }
    Ok(payload)
}

fn serialize_compliance_events_as_splunk_hec(
    events: &[agent_core::ComplianceAuditEventDetailRecord],
) -> ApiResult<String> {
    let mut payload = String::new();
    for event in events {
        let line = serde_json::to_string(&json!({
            "time": event.created_at.unix_timestamp() as f64,
            "host": "secureagnt",
            "source": "secureagnt.compliance",
            "sourcetype": "secureagnt:compliance",
            "event": compliance_event_to_json_value(event),
        }))
        .map_err(|err| {
            ApiError::internal(format!("failed serializing splunk hec export row: {err}"))
        })?;
        payload.push_str(&line);
        payload.push('\n');
    }
    Ok(payload)
}

fn serialize_compliance_events_as_elastic_bulk(
    events: &[agent_core::ComplianceAuditEventDetailRecord],
    index_name: &str,
) -> ApiResult<String> {
    let mut payload = String::new();
    for event in events {
        let action_line = serde_json::to_string(&json!({
            "index": {
                "_index": index_name,
                "_id": event.id,
            }
        }))
        .map_err(|err| {
            ApiError::internal(format!("failed serializing elastic bulk action row: {err}"))
        })?;
        let doc_line =
            serde_json::to_string(&compliance_event_to_json_value(event)).map_err(|err| {
                ApiError::internal(format!("failed serializing elastic bulk doc row: {err}"))
            })?;
        payload.push_str(&action_line);
        payload.push('\n');
        payload.push_str(&doc_line);
        payload.push('\n');
    }
    Ok(payload)
}

fn memory_to_response(record: agent_core::MemoryRecord) -> MemoryRecordResponse {
    MemoryRecordResponse {
        id: record.id,
        tenant_id: record.tenant_id,
        agent_id: record.agent_id,
        run_id: record.run_id,
        step_id: record.step_id,
        memory_kind: record.memory_kind,
        scope: record.scope,
        content_json: record.content_json,
        summary_text: record.summary_text,
        source: record.source,
        redaction_applied: record.redaction_applied,
        expires_at: record.expires_at,
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

fn handoff_packet_from_memory_record(
    record: agent_core::MemoryRecord,
) -> ApiResult<HandoffPacketResponse> {
    let packet_id = record
        .content_json
        .get("packet_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or(record.id);
    let from_agent_id = record
        .content_json
        .get("from_agent_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            ApiError::internal(format!(
                "handoff memory record {} missing from_agent_id",
                record.id
            ))
        })?;
    let to_agent_id = record
        .content_json
        .get("to_agent_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or(record.agent_id);
    let title = record
        .summary_text
        .clone()
        .or_else(|| {
            record
                .content_json
                .get("title")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "handoff".to_string());
    let payload_json = record
        .content_json
        .get("payload_json")
        .cloned()
        .unwrap_or_else(|| record.content_json.clone());

    Ok(HandoffPacketResponse {
        packet_id,
        memory_id: record.id,
        tenant_id: record.tenant_id,
        from_agent_id,
        to_agent_id,
        run_id: record.run_id,
        step_id: record.step_id,
        scope: record.scope,
        title,
        payload_json,
        source: record.source,
        redaction_applied: record.redaction_applied,
        expires_at: record.expires_at,
        created_at: record.created_at,
    })
}

fn compute_memory_retrieval_score(
    record: &agent_core::MemoryRecord,
    query_tokens: &[String],
    recency_index: usize,
    total_candidates: usize,
) -> f64 {
    let recency_score = if total_candidates <= 1 {
        1.0
    } else {
        let denominator = (total_candidates - 1) as f64;
        (1.0 - (recency_index as f64 / denominator)).clamp(0.0, 1.0)
    };
    let mut score = recency_score * 0.3;

    if record
        .summary_text
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        score += 0.1;
    }

    if !query_tokens.is_empty() {
        let mut haystack = String::new();
        if let Some(summary) = &record.summary_text {
            haystack.push_str(summary);
            haystack.push(' ');
        }
        haystack.push_str(record.content_json.to_string().as_str());
        let haystack = haystack.to_ascii_lowercase();
        let hit_count = query_tokens
            .iter()
            .filter(|token| haystack.contains(token.as_str()))
            .count();
        let overlap_ratio = (hit_count as f64) / (query_tokens.len() as f64);
        score += overlap_ratio * 1.0;
    }

    score.clamp(0.0, 2.0)
}

fn tokenize_retrieval_query(raw: &str) -> Vec<String> {
    let mut tokens = raw
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();
    tokens.sort();
    tokens.dedup();
    tokens
}

fn run_to_response(run: agent_core::RunStatusRecord) -> RunResponse {
    RunResponse {
        id: run.id,
        tenant_id: run.tenant_id,
        agent_id: run.agent_id,
        triggered_by_user_id: run.triggered_by_user_id,
        recipe_id: run.recipe_id,
        status: run.status,
        trace_id: run
            .input_json
            .get("_trace")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        requested_capabilities: run.requested_capabilities,
        granted_capabilities: run.granted_capabilities,
        created_at: run.created_at,
        started_at: run.started_at,
        finished_at: run.finished_at,
        error_json: run.error_json,
        attempts: run.attempts,
        lease_owner: run.lease_owner,
        lease_expires_at: run.lease_expires_at,
    }
}

fn trigger_to_response(trigger: agent_core::TriggerRecord) -> TriggerResponse {
    TriggerResponse {
        id: trigger.id,
        tenant_id: trigger.tenant_id,
        agent_id: trigger.agent_id,
        triggered_by_user_id: trigger.triggered_by_user_id,
        recipe_id: trigger.recipe_id,
        status: trigger.status,
        trigger_type: trigger.trigger_type,
        interval_seconds: trigger.interval_seconds,
        cron_expression: trigger.cron_expression,
        schedule_timezone: trigger.schedule_timezone,
        misfire_policy: trigger.misfire_policy,
        max_attempts: trigger.max_attempts,
        max_inflight_runs: trigger.max_inflight_runs,
        jitter_seconds: trigger.jitter_seconds,
        consecutive_failures: trigger.consecutive_failures,
        dead_lettered_at: trigger.dead_lettered_at,
        dead_letter_reason: trigger.dead_letter_reason,
        webhook_secret_configured: trigger.webhook_secret_ref.is_some(),
        input_json: trigger.input_json,
        requested_capabilities: trigger.requested_capabilities,
        granted_capabilities: trigger.granted_capabilities,
        next_fire_at: trigger.next_fire_at,
        last_fired_at: trigger.last_fired_at,
        created_at: trigger.created_at,
        updated_at: trigger.updated_at,
    }
}

fn default_json_array() -> Value {
    json!([])
}

fn default_trigger_max_attempts() -> i32 {
    3
}

fn default_trigger_max_inflight_runs() -> i32 {
    1
}

fn default_trigger_timezone() -> String {
    "UTC".to_string()
}

#[derive(Debug, Clone)]
struct RequestedCapability {
    capability: &'static str,
    scope: String,
    requested_max_payload_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
struct BundleCapability {
    capability: &'static str,
    scope: &'static str,
    max_payload_bytes: Option<u64>,
}

fn resolve_granted_capabilities(
    recipe_id: &str,
    role_preset: RolePreset,
    requested_capabilities: &Value,
) -> ApiResult<Value> {
    let requested = parse_requested_capabilities(requested_capabilities)?;
    let Some(bundle) = resolve_recipe_capability_bundle(recipe_id) else {
        return Ok(Value::Array(
            requested
                .iter()
                .filter(|item| {
                    role_allows_capability(role_preset, item.capability, item.scope.as_str())
                })
                .map(|item| {
                    capability_json(
                        item.capability,
                        item.scope.as_str(),
                        item.requested_max_payload_bytes,
                        None,
                    )
                })
                .collect(),
        ));
    };
    let bundle: Vec<BundleCapability> = bundle
        .into_iter()
        .filter(|item| role_allows_capability(role_preset, item.capability, item.scope))
        .collect();

    if requested.is_empty() {
        return Ok(Value::Array(
            bundle
                .iter()
                .map(|item| {
                    capability_json(item.capability, item.scope, None, item.max_payload_bytes)
                })
                .collect(),
        ));
    }

    let mut granted = Vec::new();
    for item in requested {
        if let Some(bundle_entry) = bundle.iter().find(|entry| {
            entry.capability == item.capability && scope_within(entry.scope, item.scope.as_str())
        }) {
            granted.push(capability_json(
                item.capability,
                item.scope.as_str(),
                item.requested_max_payload_bytes,
                bundle_entry.max_payload_bytes,
            ));
        }
    }
    Ok(Value::Array(granted))
}

fn parse_requested_capabilities(
    requested_capabilities: &Value,
) -> ApiResult<Vec<RequestedCapability>> {
    let requested_items = requested_capabilities
        .as_array()
        .ok_or_else(|| ApiError::bad_request("requested_capabilities must be an array"))?;

    let mut parsed = Vec::with_capacity(requested_items.len());
    for item in requested_items {
        let capability_raw = item
            .get("capability")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request("capability entry missing string `capability`"))?;
        let scope = item
            .get("scope")
            .and_then(Value::as_str)
            .map(str::trim)
            .ok_or_else(|| ApiError::bad_request("capability entry missing string `scope`"))?;
        if scope.is_empty() {
            return Err(ApiError::bad_request("capability scope must not be empty"));
        }

        let Some(capability) = normalize_capability(capability_raw) else {
            continue;
        };
        if !is_scope_allowed_for_capability(capability, scope) {
            continue;
        }

        parsed.push(RequestedCapability {
            capability,
            scope: scope.to_string(),
            requested_max_payload_bytes: item
                .get("limits")
                .and_then(|v| v.get("max_payload_bytes"))
                .and_then(Value::as_u64),
        });
    }
    Ok(parsed)
}

fn normalize_capability(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "object.read" | "object_read" => Some("object.read"),
        "object.write" | "object_write" => Some("object.write"),
        "memory.read" | "memory_read" => Some("memory.read"),
        "memory.write" | "memory_write" => Some("memory.write"),
        "message.send" | "message_send" => Some("message.send"),
        "payment.send" | "payment_send" => Some("payment.send"),
        "llm.infer" | "llm_infer" => Some("llm.infer"),
        "local.exec" | "local_exec" => Some("local.exec"),
        "db.query" | "db_query" => Some("db.query"),
        "http.request" | "http_request" => Some("http.request"),
        _ => None,
    }
}

fn normalize_memory_kind(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "session" => Some("session"),
        "semantic" => Some("semantic"),
        "procedural" => Some("procedural"),
        "handoff" => Some("handoff"),
        _ => None,
    }
}

fn is_scope_allowed_for_capability(capability: &str, scope: &str) -> bool {
    if scope.contains("..") || scope.contains('\0') {
        return false;
    }

    match capability {
        "object.read" => scope.starts_with("podcasts/"),
        "object.write" => scope.starts_with("shownotes/"),
        "memory.read" | "memory.write" => scope.starts_with("memory:"),
        "message.send" => scope.starts_with("whitenoise:") || scope.starts_with("slack:"),
        "payment.send" => scope.starts_with("nwc:") || scope.starts_with("cashu:"),
        "llm.infer" => scope.starts_with("local:") || scope.starts_with("remote:"),
        "local.exec" => scope.starts_with("local.exec:"),
        // Disabled in MVP.
        "db.query" | "http.request" => false,
        _ => false,
    }
}

fn resolve_max_payload_bytes(capability: &str, limits: Option<&Value>) -> u64 {
    let hard_max = match capability {
        "object.read" => MAX_OBJECT_READ_PAYLOAD_BYTES,
        "object.write" => MAX_OBJECT_WRITE_PAYLOAD_BYTES,
        "memory.read" => MAX_MEMORY_READ_PAYLOAD_BYTES,
        "memory.write" => MAX_MEMORY_WRITE_PAYLOAD_BYTES,
        "message.send" => MAX_MESSAGE_SEND_PAYLOAD_BYTES,
        "payment.send" => MAX_PAYMENT_SEND_PAYLOAD_BYTES,
        "llm.infer" => MAX_LLM_INFER_PAYLOAD_BYTES,
        "local.exec" => MAX_LOCAL_EXEC_PAYLOAD_BYTES,
        _ => 0,
    };
    let requested_max = limits.and_then(Value::as_u64);
    match requested_max {
        Some(value) if value > 0 => value.min(hard_max),
        _ => hard_max,
    }
}

fn capability_json(
    capability: &str,
    scope: &str,
    requested_max_payload_bytes: Option<u64>,
    bundle_max_payload_bytes: Option<u64>,
) -> Value {
    let hard_max = resolve_max_payload_bytes(capability, None);
    let bundle_cap = bundle_max_payload_bytes
        .map(|max| max.min(hard_max))
        .unwrap_or(hard_max);
    let capped_max = requested_max_payload_bytes
        .filter(|value| *value > 0)
        .map(|value| value.min(bundle_cap))
        .unwrap_or(bundle_cap);

    json!({
        "capability": capability,
        "scope": scope,
        "limits": {
            "max_payload_bytes": capped_max
        }
    })
}

fn scope_within(grant_scope: &str, requested_scope: &str) -> bool {
    match grant_scope.strip_suffix('*') {
        Some(prefix) => requested_scope.starts_with(prefix),
        None => grant_scope == requested_scope,
    }
}

fn role_allows_capability(role: RolePreset, capability: &str, scope: &str) -> bool {
    match role {
        RolePreset::Owner => true,
        RolePreset::Operator => capability != "local.exec",
        RolePreset::Viewer => {
            capability == "object.read"
                || (capability == "llm.infer" && scope.starts_with("local:"))
        }
    }
}

fn resolve_recipe_capability_bundle(recipe_id: &str) -> Option<Vec<BundleCapability>> {
    let bundle = match recipe_id {
        "show_notes_v1" => vec![
            BundleCapability {
                capability: "object.read",
                scope: "podcasts/*",
                max_payload_bytes: Some(MAX_OBJECT_READ_PAYLOAD_BYTES),
            },
            BundleCapability {
                capability: "object.write",
                scope: "shownotes/*",
                max_payload_bytes: Some(MAX_OBJECT_WRITE_PAYLOAD_BYTES),
            },
            BundleCapability {
                capability: "message.send",
                scope: "whitenoise:*",
                max_payload_bytes: Some(MAX_MESSAGE_SEND_PAYLOAD_BYTES),
            },
            BundleCapability {
                capability: "llm.infer",
                scope: "local:*",
                max_payload_bytes: Some(MAX_LLM_INFER_PAYLOAD_BYTES),
            },
            BundleCapability {
                capability: "local.exec",
                scope: "local.exec:file.head",
                max_payload_bytes: Some(MAX_LOCAL_EXEC_PAYLOAD_BYTES),
            },
        ],
        "notify_v1" => vec![
            BundleCapability {
                capability: "message.send",
                scope: "whitenoise:*",
                max_payload_bytes: Some(MAX_MESSAGE_SEND_PAYLOAD_BYTES),
            },
            BundleCapability {
                capability: "llm.infer",
                scope: "local:*",
                max_payload_bytes: Some(MAX_LLM_INFER_PAYLOAD_BYTES),
            },
        ],
        "operator_reply_v1" => vec![BundleCapability {
            capability: "message.send",
            scope: "whitenoise:*",
            max_payload_bytes: Some(MAX_MESSAGE_SEND_PAYLOAD_BYTES),
        }],
        "payments_v1" => vec![BundleCapability {
            capability: "payment.send",
            scope: "nwc:*",
            max_payload_bytes: Some(MAX_PAYMENT_SEND_PAYLOAD_BYTES),
        }],
        "payments_cashu_v1" => vec![BundleCapability {
            capability: "payment.send",
            scope: "cashu:*",
            max_payload_bytes: Some(MAX_PAYMENT_SEND_PAYLOAD_BYTES),
        }],
        "memory_v1" => vec![
            BundleCapability {
                capability: "memory.read",
                scope: "memory:*",
                max_payload_bytes: Some(MAX_MEMORY_READ_PAYLOAD_BYTES),
            },
            BundleCapability {
                capability: "memory.write",
                scope: "memory:*",
                max_payload_bytes: Some(MAX_MEMORY_WRITE_PAYLOAD_BYTES),
            },
        ],
        "llm_local_v1" => vec![BundleCapability {
            capability: "llm.infer",
            scope: "local:*",
            max_payload_bytes: Some(MAX_LLM_INFER_PAYLOAD_BYTES),
        }],
        "llm_remote_v1" => vec![BundleCapability {
            capability: "llm.infer",
            scope: "remote:*",
            max_payload_bytes: Some(MAX_LLM_INFER_PAYLOAD_BYTES),
        }],
        "local_exec_v1" => vec![
            BundleCapability {
                capability: "local.exec",
                scope: "local.exec:file.head",
                max_payload_bytes: Some(MAX_LOCAL_EXEC_PAYLOAD_BYTES),
            },
            BundleCapability {
                capability: "local.exec",
                scope: "local.exec:file.word_count",
                max_payload_bytes: Some(MAX_LOCAL_EXEC_PAYLOAD_BYTES),
            },
        ],
        _ => return None,
    };
    Some(bundle)
}
