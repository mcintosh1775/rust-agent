use agent_core::{
    append_audit_event, append_trigger_audit_event, count_tenant_inflight_runs,
    count_tenant_triggers, create_compliance_siem_delivery_record, create_cron_trigger,
    create_interval_trigger, create_memory_record, create_run, create_webhook_trigger,
    enqueue_trigger_event, fire_trigger_manually, get_llm_usage_totals_since, get_run_status,
    get_tenant_action_latency_summary, get_tenant_action_latency_traces,
    get_tenant_compliance_audit_policy, get_tenant_compliance_siem_delivery_slo,
    get_tenant_compliance_siem_delivery_summary, get_tenant_memory_compaction_stats,
    get_tenant_ops_summary, get_tenant_payment_summary, get_tenant_run_latency_histogram,
    get_tenant_run_latency_traces, get_trigger, list_run_audit_events,
    list_tenant_compliance_audit_events, list_tenant_compliance_siem_delivery_records,
    list_tenant_compliance_siem_delivery_target_summaries, list_tenant_handoff_memory_records,
    list_tenant_memory_records, list_tenant_payment_ledger,
    purge_expired_tenant_compliance_audit_events, purge_expired_tenant_memory_records,
    redact_memory_content, requeue_dead_letter_compliance_siem_delivery_record,
    requeue_dead_letter_trigger_event, resolve_secret_value, update_trigger_config,
    update_trigger_status, upsert_tenant_compliance_audit_policy,
    verify_tenant_compliance_audit_chain, CachedSecretResolver, CliSecretResolver,
    ManualTriggerFireOutcome, NewAuditEvent, NewComplianceSiemDeliveryRecord, NewCronTrigger,
    NewIntervalTrigger, NewMemoryRecord, NewRun, NewTriggerAuditEvent, NewWebhookTrigger,
    TriggerEventEnqueueOutcome, TriggerEventReplayOutcome, UpdateTriggerParams,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use core as agent_core;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{env, sync::OnceLock};
use time::OffsetDateTime;
use uuid::Uuid;

const TENANT_HEADER: &str = "x-tenant-id";
const ROLE_HEADER: &str = "x-user-role";
const USER_ID_HEADER: &str = "x-user-id";
const TRIGGER_SECRET_HEADER: &str = "x-trigger-secret";
const MAX_OBJECT_WRITE_PAYLOAD_BYTES: u64 = 500_000;
const MAX_MESSAGE_SEND_PAYLOAD_BYTES: u64 = 20_000;
const MAX_OBJECT_READ_PAYLOAD_BYTES: u64 = 128_000;
const MAX_LOCAL_EXEC_PAYLOAD_BYTES: u64 = 4_096;
const MAX_LLM_INFER_PAYLOAD_BYTES: u64 = 32_000;
const MAX_PAYMENT_SEND_PAYLOAD_BYTES: u64 = 16_000;
const MAX_MEMORY_READ_PAYLOAD_BYTES: u64 = 64_000;
const MAX_MEMORY_WRITE_PAYLOAD_BYTES: u64 = 64_000;
const CONSOLE_INDEX_HTML: &str = include_str!("../static/console.html");

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub tenant_max_inflight_runs: Option<i64>,
    pub tenant_max_triggers: Option<i64>,
    pub tenant_max_memory_records: Option<i64>,
}

pub fn app_router(pool: PgPool) -> Router {
    let tenant_max_inflight_runs = parse_positive_i64_env("API_TENANT_MAX_INFLIGHT_RUNS");
    let tenant_max_triggers = parse_positive_i64_env("API_TENANT_MAX_TRIGGERS");
    let tenant_max_memory_records = parse_positive_i64_env("API_TENANT_MAX_MEMORY_RECORDS");
    app_router_with_all_limits(
        pool,
        tenant_max_inflight_runs,
        tenant_max_triggers,
        tenant_max_memory_records,
    )
}

pub fn app_router_with_tenant_limit(pool: PgPool, tenant_max_inflight_runs: Option<i64>) -> Router {
    app_router_with_all_limits(pool, tenant_max_inflight_runs, None, None)
}

pub fn app_router_with_limits(
    pool: PgPool,
    tenant_max_inflight_runs: Option<i64>,
    tenant_max_triggers: Option<i64>,
) -> Router {
    app_router_with_all_limits(pool, tenant_max_inflight_runs, tenant_max_triggers, None)
}

pub fn app_router_with_memory_limit(
    pool: PgPool,
    tenant_max_memory_records: Option<i64>,
) -> Router {
    app_router_with_all_limits(pool, None, None, tenant_max_memory_records)
}

fn app_router_with_all_limits(
    pool: PgPool,
    tenant_max_inflight_runs: Option<i64>,
    tenant_max_triggers: Option<i64>,
    tenant_max_memory_records: Option<i64>,
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
            pool,
            tenant_max_inflight_runs,
            tenant_max_triggers,
            tenant_max_memory_records,
        })
}

async fn console_index_handler() -> impl IntoResponse {
    Html(CONSOLE_INDEX_HTML)
}

fn parse_positive_i64_env(key: &str) -> Option<i64> {
    env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
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

#[derive(Debug, Serialize)]
struct RunResponse {
    id: Uuid,
    tenant_id: String,
    agent_id: Uuid,
    triggered_by_user_id: Option<Uuid>,
    recipe_id: String,
    status: String,
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
    succeeded_runs_window: i64,
    failed_runs_window: i64,
    dead_letter_trigger_events_window: i64,
    avg_run_duration_ms: Option<f64>,
    p95_run_duration_ms: Option<f64>,
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

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN",
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
    let role_preset = role_from_headers(&headers)?;
    if let Some(limit) = state.tenant_max_inflight_runs {
        let inflight = count_tenant_inflight_runs(&state.pool, tenant_id.as_str())
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
    let run_id = Uuid::new_v4();
    let requested_capabilities = req.requested_capabilities;
    let granted_capabilities =
        resolve_granted_capabilities(req.recipe_id.as_str(), role_preset, &requested_capabilities)?;

    let created = create_run(
        &state.pool,
        &NewRun {
            id: run_id,
            tenant_id: tenant_id.clone(),
            agent_id: req.agent_id,
            triggered_by_user_id: req.triggered_by_user_id,
            recipe_id: req.recipe_id,
            status: "queued".to_string(),
            input_json: req.input,
            requested_capabilities: requested_capabilities.clone(),
            granted_capabilities: granted_capabilities.clone(),
            error_json: None,
        },
    )
    .await
    .map_err(|err| ApiError::internal(format!("failed creating run: {err}")))?;

    append_audit_event(
        &state.pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: created.id,
            step_id: None,
            tenant_id,
            agent_id: Some(created.agent_id),
            user_id: created.triggered_by_user_id,
            actor: "api".to_string(),
            event_type: "run.created".to_string(),
            payload_json: json!({
                "recipe_id": created.recipe_id,
                "role_preset": role_preset.as_str(),
                "requested_capability_count": requested_capabilities.as_array().map_or(0, |v| v.len()),
                "granted_capability_count": granted_capabilities.as_array().map_or(0, |v| v.len()),
            }),
        },
    )
    .await
    .map_err(|err| {
        ApiError::internal(format!("failed appending run.created audit event: {err}"))
    })?;

    let run = get_run_status(&state.pool, &created.tenant_id, created.id)
        .await
        .map_err(|err| ApiError::internal(format!("failed loading created run: {err}")))?
        .ok_or_else(|| ApiError::internal("created run could not be reloaded"))?;

    Ok((StatusCode::CREATED, Json(run_to_response(run))))
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

    let Some(run) = get_run_status(&state.pool, &tenant_id, run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed fetching run: {err}")))?
    else {
        return Err(ApiError::not_found("run not found"));
    };

    Ok((StatusCode::OK, Json(run_to_response(run))))
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
    let role_preset = role_from_headers(&headers)?;
    let actor_user_id = user_id_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
    let actor_user_id = user_id_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
    let actor_user_id = user_id_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
    let actor_user_id = user_id_from_headers(&headers)?;
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
        return Err(ApiError::bad_request("trigger is not enabled"));
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
    let role_preset = role_from_headers(&headers)?;
    let actor_user_id = user_id_from_headers(&headers)?;
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
        return Err(ApiError::bad_request("trigger is not enabled"));
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
            Err(ApiError::bad_request("trigger is not enabled"))
        }
    }
}

async fn replay_trigger_event_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((trigger_id, event_id)): Path<(Uuid, String)>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&headers)?;
    let actor_user_id = user_id_from_headers(&headers)?;
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
        return Err(ApiError::bad_request("trigger is not enabled"));
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

    let run_exists = get_run_status(&state.pool, &tenant_id, run_id)
        .await
        .map_err(|err| ApiError::internal(format!("failed checking run existence: {err}")))?
        .is_some();
    if !run_exists {
        return Err(ApiError::not_found("run not found"));
    }

    let events = list_run_audit_events(&state.pool, &tenant_id, run_id, limit)
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

async fn create_memory_record_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateMemoryRecordRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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

async fn get_compliance_audit_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ComplianceAuditQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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

    let mut alerts = rows
        .into_iter()
        .filter_map(|row| {
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

async fn replay_compliance_audit_siem_delivery_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(record_id): Path<Uuid>,
    Json(req): Json<ReplayComplianceAuditSiemDeliveryRequest>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
    ensure_usage_query_role(role_preset)?;

    let window_secs = query.window_secs.unwrap_or(86_400).clamp(1, 31_536_000);
    let since = OffsetDateTime::now_utc() - time::Duration::seconds(window_secs as i64);
    let summary = get_tenant_ops_summary(&state.pool, tenant_id.as_str(), since)
        .await
        .map_err(|err| ApiError::internal(format!("failed querying ops summary: {err}")))?;

    Ok((
        StatusCode::OK,
        Json(OpsSummaryResponse {
            tenant_id,
            window_secs,
            since,
            queued_runs: summary.queued_runs,
            running_runs: summary.running_runs,
            succeeded_runs_window: summary.succeeded_runs_window,
            failed_runs_window: summary.failed_runs_window,
            dead_letter_trigger_events_window: summary.dead_letter_trigger_events_window,
            avg_run_duration_ms: summary.avg_run_duration_ms,
            p95_run_duration_ms: summary.p95_run_duration_ms,
        }),
    ))
}

async fn get_ops_latency_histogram_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OpsLatencyHistogramQuery>,
) -> ApiResult<impl IntoResponse> {
    let tenant_id = tenant_from_headers(&headers)?;
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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
    let role_preset = role_from_headers(&headers)?;
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

fn role_from_headers(headers: &HeaderMap) -> ApiResult<RolePreset> {
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

fn user_id_from_headers(headers: &HeaderMap) -> ApiResult<Option<Uuid>> {
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
    let role_preset = role_from_headers(&headers)?;
    let actor_user_id = user_id_from_headers(&headers)?;
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
