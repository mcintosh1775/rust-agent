use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde_json::{json, Map, Value};
use sqlx::{PgPool, Row};
use std::str::FromStr;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;
use sha2::{Digest, Sha256};

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
pub struct NewPaymentRequest {
    pub id: Uuid,
    pub action_request_id: Uuid,
    pub run_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub provider: String,
    pub operation: String,
    pub destination: String,
    pub idempotency_key: String,
    pub amount_msat: Option<i64>,
    pub request_json: Value,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PaymentRequestRecord {
    pub id: Uuid,
    pub action_request_id: Uuid,
    pub run_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub provider: String,
    pub operation: String,
    pub destination: String,
    pub idempotency_key: String,
    pub amount_msat: Option<i64>,
    pub request_json: Value,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewPaymentResult {
    pub id: Uuid,
    pub payment_request_id: Uuid,
    pub status: String,
    pub result_json: Option<Value>,
    pub error_json: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct PaymentResultRecord {
    pub id: Uuid,
    pub payment_request_id: Uuid,
    pub status: String,
    pub result_json: Option<Value>,
    pub error_json: Option<Value>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct PaymentLedgerRecord {
    pub id: Uuid,
    pub action_request_id: Uuid,
    pub run_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub provider: String,
    pub operation: String,
    pub destination: String,
    pub idempotency_key: String,
    pub amount_msat: Option<i64>,
    pub request_json: Value,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub latest_result_status: Option<String>,
    pub latest_result_json: Option<Value>,
    pub latest_error_json: Option<Value>,
    pub latest_result_created_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct PaymentSummaryRecord {
    pub total_requests: i64,
    pub requested_count: i64,
    pub executed_count: i64,
    pub failed_count: i64,
    pub duplicate_count: i64,
    pub executed_spend_msat: i64,
}

#[derive(Debug, Clone)]
pub struct TenantOpsSummaryRecord {
    pub queued_runs: i64,
    pub running_runs: i64,
    pub succeeded_runs_window: i64,
    pub failed_runs_window: i64,
    pub dead_letter_trigger_events_window: i64,
    pub avg_run_duration_ms: Option<f64>,
    pub p95_run_duration_ms: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct TenantRunLatencyHistogramBucket {
    pub bucket_label: String,
    pub lower_bound_ms: i64,
    pub upper_bound_exclusive_ms: Option<i64>,
    pub run_count: i64,
}

#[derive(Debug, Clone)]
pub struct TenantRunLatencyTraceRecord {
    pub run_id: Uuid,
    pub status: String,
    pub duration_ms: i64,
    pub started_at: OffsetDateTime,
    pub finished_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct TenantActionLatencyRecord {
    pub action_type: String,
    pub total_count: i64,
    pub avg_duration_ms: Option<f64>,
    pub p95_duration_ms: Option<f64>,
    pub max_duration_ms: Option<i64>,
    pub failed_count: i64,
    pub denied_count: i64,
}

#[derive(Debug, Clone)]
pub struct TenantLlmGatewayLaneSummaryRecord {
    pub request_class: String,
    pub total_count: i64,
    pub p95_duration_ms: Option<f64>,
    pub avg_duration_ms: Option<f64>,
    pub cache_hit_count: i64,
    pub distributed_cache_hit_count: i64,
    pub verifier_escalated_count: i64,
    pub slo_warn_count: i64,
    pub slo_breach_count: i64,
    pub distributed_fail_open_count: i64,
}

#[derive(Debug, Clone)]
pub struct TenantActionLatencyTraceRecord {
    pub action_request_id: Uuid,
    pub run_id: Uuid,
    pub step_id: Uuid,
    pub action_type: String,
    pub status: String,
    pub duration_ms: i64,
    pub created_at: OffsetDateTime,
    pub executed_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct NewMemoryRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub run_id: Option<Uuid>,
    pub step_id: Option<Uuid>,
    pub memory_kind: String,
    pub scope: String,
    pub content_json: Value,
    pub summary_text: Option<String>,
    pub source: String,
    pub redaction_applied: bool,
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct MemoryRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub run_id: Option<Uuid>,
    pub step_id: Option<Uuid>,
    pub memory_kind: String,
    pub scope: String,
    pub content_json: Value,
    pub summary_text: Option<String>,
    pub source: String,
    pub redaction_applied: bool,
    pub expires_at: Option<OffsetDateTime>,
    pub compacted_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewMemoryCompactionRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Option<Uuid>,
    pub memory_kind: String,
    pub scope: String,
    pub source_count: i32,
    pub source_entry_ids: Value,
    pub summary_json: Value,
}

#[derive(Debug, Clone)]
pub struct MemoryCompactionRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Option<Uuid>,
    pub memory_kind: String,
    pub scope: String,
    pub source_count: i32,
    pub source_entry_ids: Value,
    pub summary_json: Value,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct MemoryPurgeOutcome {
    pub tenant_id: String,
    pub deleted_count: i64,
    pub as_of: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct MemoryCompactionRunStats {
    pub processed_groups: i64,
    pub compacted_source_records: i64,
    pub groups: Vec<MemoryCompactionGroupOutcome>,
}

#[derive(Debug, Clone)]
pub struct MemoryCompactionGroupOutcome {
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub memory_kind: String,
    pub scope: String,
    pub source_count: i64,
    pub source_entry_ids: Value,
    pub representative_run_id: Option<Uuid>,
    pub representative_step_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct MemoryCompactionStatsRecord {
    pub compacted_groups_window: i64,
    pub compacted_source_records_window: i64,
    pub pending_uncompacted_records: i64,
    pub last_compacted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct NewLlmTokenUsageRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub action_request_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub route: String,
    pub model_key: String,
    pub consumed_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub window_started_at: OffsetDateTime,
    pub window_duration_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct LlmTokenUsageRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub action_request_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub route: String,
    pub model_key: String,
    pub consumed_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub window_started_at: OffsetDateTime,
    pub window_duration_seconds: i64,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct LlmGatewayAdmissionLeaseAcquireParams {
    pub namespace: String,
    pub lane: String,
    pub max_inflight: i32,
    pub lease_id: Uuid,
    pub lease_owner: String,
    pub lease_for: Duration,
}

#[derive(Debug, Clone)]
pub struct LlmGatewayAdmissionLeaseRecord {
    pub namespace: String,
    pub lane: String,
    pub slot_index: i32,
    pub lease_id: Uuid,
    pub lease_owner: String,
    pub lease_expires_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewLlmGatewayCacheEntry {
    pub cache_key_sha256: String,
    pub namespace: String,
    pub route: String,
    pub model: String,
    pub response_json: Value,
    pub ttl: Duration,
}

#[derive(Debug, Clone)]
pub struct LlmGatewayCacheEntryRecord {
    pub cache_key_sha256: String,
    pub namespace: String,
    pub route: String,
    pub model: String,
    pub response_json: Value,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
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
pub struct ComplianceAuditEventDetailRecord {
    pub id: Uuid,
    pub source_audit_event_id: Uuid,
    pub tamper_chain_seq: i64,
    pub tamper_prev_hash: Option<String>,
    pub tamper_hash: String,
    pub run_id: Uuid,
    pub step_id: Option<Uuid>,
    pub tenant_id: String,
    pub agent_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub actor: String,
    pub event_type: String,
    pub payload_json: Value,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub action_request_id: Option<Uuid>,
    pub payment_request_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub recorded_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct ComplianceAuditTamperVerificationRecord {
    pub tenant_id: String,
    pub checked_events: i64,
    pub verified: bool,
    pub first_invalid_event_id: Option<Uuid>,
    pub latest_chain_seq: Option<i64>,
    pub latest_tamper_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ComplianceAuditPolicyRecord {
    pub tenant_id: String,
    pub compliance_hot_retention_days: i32,
    pub compliance_archive_retention_days: i32,
    pub legal_hold: bool,
    pub legal_hold_reason: Option<String>,
    pub updated_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct ComplianceAuditPurgeOutcome {
    pub tenant_id: String,
    pub deleted_count: i64,
    pub legal_hold: bool,
    pub cutoff_at: OffsetDateTime,
    pub compliance_hot_retention_days: i32,
    pub compliance_archive_retention_days: i32,
}

#[derive(Debug, Clone)]
pub struct NewComplianceSiemDeliveryRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub run_id: Option<Uuid>,
    pub adapter: String,
    pub delivery_target: String,
    pub content_type: String,
    pub payload_ndjson: String,
    pub max_attempts: i32,
}

#[derive(Debug, Clone)]
pub struct ComplianceSiemDeliveryRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub run_id: Option<Uuid>,
    pub adapter: String,
    pub delivery_target: String,
    pub content_type: String,
    pub payload_ndjson: String,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_attempt_at: OffsetDateTime,
    pub leased_by: Option<String>,
    pub lease_expires_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub last_http_status: Option<i32>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub delivered_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct ComplianceSiemDeliverySummaryRecord {
    pub pending_count: i64,
    pub processing_count: i64,
    pub failed_count: i64,
    pub delivered_count: i64,
    pub dead_lettered_count: i64,
    pub oldest_pending_age_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ComplianceSiemDeliverySloRecord {
    pub total_count: i64,
    pub pending_count: i64,
    pub processing_count: i64,
    pub failed_count: i64,
    pub delivered_count: i64,
    pub dead_lettered_count: i64,
    pub delivery_success_rate_pct: Option<f64>,
    pub hard_failure_rate_pct: Option<f64>,
    pub dead_letter_rate_pct: Option<f64>,
    pub oldest_pending_age_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ComplianceSiemDeliveryTargetSummaryRecord {
    pub delivery_target: String,
    pub total_count: i64,
    pub pending_count: i64,
    pub processing_count: i64,
    pub failed_count: i64,
    pub delivered_count: i64,
    pub dead_lettered_count: i64,
    pub last_error: Option<String>,
    pub last_http_status: Option<i32>,
    pub last_attempt_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone)]
pub struct NewComplianceSiemDeliveryAlertAckRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub run_scope: String,
    pub delivery_target: String,
    pub acknowledged_by_user_id: Uuid,
    pub acknowledged_by_role: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ComplianceSiemDeliveryAlertAckRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub run_scope: String,
    pub delivery_target: String,
    pub acknowledged_by_user_id: Uuid,
    pub acknowledged_by_role: String,
    pub note: Option<String>,
    pub created_at: OffsetDateTime,
    pub acknowledged_at: OffsetDateTime,
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

#[derive(Debug, Clone)]
pub struct NewIntervalTrigger {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub interval_seconds: i64,
    pub input_json: Value,
    pub requested_capabilities: Value,
    pub granted_capabilities: Value,
    pub next_fire_at: OffsetDateTime,
    pub status: String,
    pub misfire_policy: String,
    pub max_attempts: i32,
    pub max_inflight_runs: i32,
    pub jitter_seconds: i32,
    pub webhook_secret_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewWebhookTrigger {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub input_json: Value,
    pub requested_capabilities: Value,
    pub granted_capabilities: Value,
    pub status: String,
    pub max_attempts: i32,
    pub max_inflight_runs: i32,
    pub jitter_seconds: i32,
    pub webhook_secret_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewCronTrigger {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub cron_expression: String,
    pub schedule_timezone: String,
    pub input_json: Value,
    pub requested_capabilities: Value,
    pub granted_capabilities: Value,
    pub status: String,
    pub misfire_policy: String,
    pub max_attempts: i32,
    pub max_inflight_runs: i32,
    pub jitter_seconds: i32,
}

#[derive(Debug, Clone)]
pub struct TriggerRecord {
    pub id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub status: String,
    pub trigger_type: String,
    pub interval_seconds: Option<i64>,
    pub cron_expression: Option<String>,
    pub schedule_timezone: String,
    pub misfire_policy: String,
    pub max_attempts: i32,
    pub max_inflight_runs: i32,
    pub jitter_seconds: i32,
    pub consecutive_failures: i32,
    pub dead_lettered_at: Option<OffsetDateTime>,
    pub dead_letter_reason: Option<String>,
    pub webhook_secret_ref: Option<String>,
    pub input_json: Value,
    pub requested_capabilities: Value,
    pub granted_capabilities: Value,
    pub next_fire_at: OffsetDateTime,
    pub last_fired_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct TriggerDispatchRecord {
    pub trigger_id: Uuid,
    pub trigger_type: String,
    pub trigger_event_id: Option<String>,
    pub run_id: Uuid,
    pub tenant_id: String,
    pub agent_id: Uuid,
    pub triggered_by_user_id: Option<Uuid>,
    pub recipe_id: String,
    pub scheduled_for: OffsetDateTime,
    pub next_fire_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEventEnqueueOutcome {
    Enqueued,
    Duplicate,
    TriggerUnavailable {
        reason: TriggerEventEnqueueUnavailableReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEventEnqueueUnavailableReason {
    TriggerNotFound,
    TriggerDisabled,
    TriggerTypeMismatch,
    TriggerScheduleBroken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerEventReplayOutcome {
    Requeued,
    NotFound,
    NotDeadLettered { status: String },
}

#[derive(Debug, Clone)]
pub enum ManualTriggerFireOutcome {
    Created(TriggerDispatchRecord),
    Duplicate { run_id: Option<Uuid> },
    InflightLimited,
    TriggerUnavailable,
}

#[derive(Debug, Clone)]
pub struct UpdateTriggerParams {
    pub interval_seconds: Option<i64>,
    pub cron_expression: Option<String>,
    pub schedule_timezone: Option<String>,
    pub misfire_policy: Option<String>,
    pub max_attempts: Option<i32>,
    pub max_inflight_runs: Option<i32>,
    pub jitter_seconds: Option<i32>,
    pub webhook_secret_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewTriggerAuditEvent {
    pub id: Uuid,
    pub trigger_id: Uuid,
    pub tenant_id: String,
    pub actor: String,
    pub event_type: String,
    pub payload_json: Value,
}

#[derive(Debug, Clone)]
pub struct SchedulerLeaseParams {
    pub lease_name: String,
    pub lease_owner: String,
    pub lease_for: Duration,
}

const DEFAULT_TENANT_MAX_INFLIGHT_RUNS: i64 = 100;
const TRIGGER_STATUS_ENABLED: &str = "enabled";
const TRIGGER_TYPE_WEBHOOK: &str = "webhook";
const TRIGGER_EVENT_STATUS_PENDING: &str = "pending";
const TRIGGER_EVENT_STATUS_DEAD_LETTERED: &str = "dead_lettered";
const TRIGGER_ERROR_CLASS_TRIGGER_POLICY: &str = "trigger_policy";
const TRIGGER_ERROR_CLASS_SCHEDULE: &str = "schedule";
const TRIGGER_ERROR_CLASS_EVENT_PAYLOAD: &str = "event_payload";

fn trigger_error_payload(code: &str, message: impl Into<String>, reason_class: &str) -> Value {
    json!({
        "code": code,
        "message": message.into(),
        "reason_class": reason_class,
    })
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

pub async fn create_run_with_semantic_dedupe_key(
    pool: &PgPool,
    new_run: &NewRun,
    semantic_dedupe_key: &str,
) -> Result<Option<RunRecord>, sqlx::Error> {
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
            error_json,
            semantic_dedupe_key
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (tenant_id, semantic_dedupe_key)
            WHERE status IN ('queued', 'running')
            DO NOTHING
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
    .bind(semantic_dedupe_key)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(RunRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        triggered_by_user_id: row.get("triggered_by_user_id"),
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    }))
}

pub async fn get_active_run_id_by_semantic_dedupe_key(
    pool: &PgPool,
    tenant_id: &str,
    semantic_dedupe_key: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let run_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM runs
        WHERE tenant_id = $1
          AND semantic_dedupe_key = $2
          AND status IN ('queued', 'running')
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(tenant_id)
    .bind(semantic_dedupe_key)
    .fetch_optional(pool)
    .await?;

    Ok(run_id)
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

pub async fn count_tenant_inflight_runs(
    pool: &PgPool,
    tenant_id: &str,
) -> Result<i64, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM runs
        WHERE tenant_id = $1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn count_inflight_runs(pool: &PgPool) -> Result<i64, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM runs
        WHERE status IN ('queued', 'running')
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn count_tenant_triggers(pool: &PgPool, tenant_id: &str) -> Result<i64, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM triggers
        WHERE tenant_id = $1
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
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

pub async fn create_or_get_payment_request(
    pool: &PgPool,
    new_request: &NewPaymentRequest,
) -> Result<PaymentRequestRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        WITH inserted AS (
            INSERT INTO payment_requests (
                id,
                action_request_id,
                run_id,
                tenant_id,
                agent_id,
                provider,
                operation,
                destination,
                idempotency_key,
                amount_msat,
                request_json,
                status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (tenant_id, idempotency_key) DO NOTHING
            RETURNING id,
                      action_request_id,
                      run_id,
                      tenant_id,
                      agent_id,
                      provider,
                      operation,
                      destination,
                      idempotency_key,
                      amount_msat,
                      request_json,
                      status,
                      created_at,
                      updated_at
        )
        SELECT id,
               action_request_id,
               run_id,
               tenant_id,
               agent_id,
               provider,
               operation,
               destination,
               idempotency_key,
               amount_msat,
               request_json,
               status,
               created_at,
               updated_at
        FROM inserted
        UNION ALL
        SELECT id,
               action_request_id,
               run_id,
               tenant_id,
               agent_id,
               provider,
               operation,
               destination,
               idempotency_key,
               amount_msat,
               request_json,
               status,
               created_at,
               updated_at
        FROM payment_requests
        WHERE tenant_id = $4
          AND idempotency_key = $9
          AND NOT EXISTS (SELECT 1 FROM inserted)
        LIMIT 1
        "#,
    )
    .bind(new_request.id)
    .bind(new_request.action_request_id)
    .bind(new_request.run_id)
    .bind(&new_request.tenant_id)
    .bind(new_request.agent_id)
    .bind(&new_request.provider)
    .bind(&new_request.operation)
    .bind(&new_request.destination)
    .bind(&new_request.idempotency_key)
    .bind(new_request.amount_msat)
    .bind(&new_request.request_json)
    .bind(&new_request.status)
    .fetch_one(pool)
    .await?;

    Ok(PaymentRequestRecord {
        id: row.get("id"),
        action_request_id: row.get("action_request_id"),
        run_id: row.get("run_id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        provider: row.get("provider"),
        operation: row.get("operation"),
        destination: row.get("destination"),
        idempotency_key: row.get("idempotency_key"),
        amount_msat: row.get("amount_msat"),
        request_json: row.get("request_json"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn create_payment_result(
    pool: &PgPool,
    new_result: &NewPaymentResult,
) -> Result<PaymentResultRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO payment_results (
            id,
            payment_request_id,
            status,
            result_json,
            error_json
        )
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id,
                  payment_request_id,
                  status,
                  result_json,
                  error_json,
                  created_at
        "#,
    )
    .bind(new_result.id)
    .bind(new_result.payment_request_id)
    .bind(&new_result.status)
    .bind(&new_result.result_json)
    .bind(&new_result.error_json)
    .fetch_one(pool)
    .await?;

    Ok(PaymentResultRecord {
        id: row.get("id"),
        payment_request_id: row.get("payment_request_id"),
        status: row.get("status"),
        result_json: row.get("result_json"),
        error_json: row.get("error_json"),
        created_at: row.get("created_at"),
    })
}

pub async fn get_latest_payment_result(
    pool: &PgPool,
    payment_request_id: Uuid,
) -> Result<Option<PaymentResultRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id,
               payment_request_id,
               status,
               result_json,
               error_json,
               created_at
        FROM payment_results
        WHERE payment_request_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(payment_request_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| PaymentResultRecord {
        id: row.get("id"),
        payment_request_id: row.get("payment_request_id"),
        status: row.get("status"),
        result_json: row.get("result_json"),
        error_json: row.get("error_json"),
        created_at: row.get("created_at"),
    }))
}

pub async fn list_tenant_payment_ledger(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    status: Option<&str>,
    destination: Option<&str>,
    idempotency_key: Option<&str>,
    limit: i64,
) -> Result<Vec<PaymentLedgerRecord>, sqlx::Error> {
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
        LEFT JOIN LATERAL (
            SELECT status, result_json, error_json, created_at
            FROM payment_results
            WHERE payment_request_id = pr.id
            ORDER BY created_at DESC, id DESC
            LIMIT 1
        ) latest ON true
        WHERE pr.tenant_id = $1
          AND ($2::uuid IS NULL OR pr.run_id = $2)
          AND ($3::uuid IS NULL OR pr.agent_id = $3)
          AND ($4::text IS NULL OR pr.status = $4)
          AND ($5::text IS NULL OR pr.destination = $5)
          AND ($6::text IS NULL OR pr.idempotency_key = $6)
        ORDER BY pr.created_at DESC, pr.id DESC
        LIMIT $7
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .bind(agent_id)
    .bind(status)
    .bind(destination)
    .bind(idempotency_key)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| PaymentLedgerRecord {
            id: row.get("id"),
            action_request_id: row.get("action_request_id"),
            run_id: row.get("run_id"),
            tenant_id: row.get("tenant_id"),
            agent_id: row.get("agent_id"),
            provider: row.get("provider"),
            operation: row.get("operation"),
            destination: row.get("destination"),
            idempotency_key: row.get("idempotency_key"),
            amount_msat: row.get("amount_msat"),
            request_json: row.get("request_json"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            latest_result_status: row.get("latest_result_status"),
            latest_result_json: row.get("latest_result_json"),
            latest_error_json: row.get("latest_error_json"),
            latest_result_created_at: row.get("latest_result_created_at"),
        })
        .collect())
}

pub async fn get_tenant_payment_summary(
    pool: &PgPool,
    tenant_id: &str,
    since: Option<OffsetDateTime>,
    agent_id: Option<Uuid>,
    operation: Option<&str>,
) -> Result<PaymentSummaryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT COUNT(*)::bigint AS total_requests,
               COUNT(*) FILTER (WHERE status = 'requested')::bigint AS requested_count,
               COUNT(*) FILTER (WHERE status = 'executed')::bigint AS executed_count,
               COUNT(*) FILTER (WHERE status = 'failed')::bigint AS failed_count,
               COUNT(*) FILTER (WHERE status = 'duplicate')::bigint AS duplicate_count,
               COALESCE(SUM(
                 CASE
                   WHEN status = 'executed' AND operation = 'pay_invoice'
                   THEN amount_msat
                   ELSE 0
                 END
               ), 0)::bigint AS executed_spend_msat
        FROM payment_requests
        WHERE tenant_id = $1
          AND ($2::timestamptz IS NULL OR created_at >= $2)
          AND ($3::uuid IS NULL OR agent_id = $3)
          AND ($4::text IS NULL OR operation = $4)
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .bind(agent_id)
    .bind(operation)
    .fetch_one(pool)
    .await?;

    Ok(PaymentSummaryRecord {
        total_requests: row.get("total_requests"),
        requested_count: row.get("requested_count"),
        executed_count: row.get("executed_count"),
        failed_count: row.get("failed_count"),
        duplicate_count: row.get("duplicate_count"),
        executed_spend_msat: row.get("executed_spend_msat"),
    })
}

pub async fn get_tenant_ops_summary(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<TenantOpsSummaryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        WITH duration_window AS (
            SELECT EXTRACT(EPOCH FROM (finished_at - started_at)) * 1000.0 AS duration_ms
            FROM runs
            WHERE tenant_id = $1
              AND finished_at IS NOT NULL
              AND started_at IS NOT NULL
              AND finished_at >= $2
        )
        SELECT
          (SELECT COUNT(*)::bigint
           FROM runs
           WHERE tenant_id = $1 AND status = 'queued') AS queued_runs,
          (SELECT COUNT(*)::bigint
           FROM runs
           WHERE tenant_id = $1 AND status = 'running') AS running_runs,
          (SELECT COUNT(*)::bigint
           FROM runs
           WHERE tenant_id = $1
             AND status = 'succeeded'
             AND finished_at >= $2) AS succeeded_runs_window,
          (SELECT COUNT(*)::bigint
           FROM runs
           WHERE tenant_id = $1
             AND status = 'failed'
             AND finished_at >= $2) AS failed_runs_window,
          (SELECT COUNT(*)::bigint
           FROM trigger_events
           WHERE tenant_id = $1
             AND status = 'dead_lettered'
             AND created_at >= $2) AS dead_letter_trigger_events_window,
          (SELECT AVG(duration_ms)::double precision FROM duration_window) AS avg_run_duration_ms,
          (SELECT percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms)::double precision
           FROM duration_window) AS p95_run_duration_ms
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_one(pool)
    .await?;

    Ok(TenantOpsSummaryRecord {
        queued_runs: row.get("queued_runs"),
        running_runs: row.get("running_runs"),
        succeeded_runs_window: row.get("succeeded_runs_window"),
        failed_runs_window: row.get("failed_runs_window"),
        dead_letter_trigger_events_window: row.get("dead_letter_trigger_events_window"),
        avg_run_duration_ms: row.get("avg_run_duration_ms"),
        p95_run_duration_ms: row.get("p95_run_duration_ms"),
    })
}

pub async fn get_tenant_run_latency_histogram(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<Vec<TenantRunLatencyHistogramBucket>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH duration_window AS (
            SELECT GREATEST(
                (EXTRACT(EPOCH FROM (finished_at - started_at)) * 1000.0)::bigint,
                0
            ) AS duration_ms
            FROM runs
            WHERE tenant_id = $1
              AND finished_at IS NOT NULL
              AND started_at IS NOT NULL
              AND finished_at >= $2
        ),
        buckets AS (
            SELECT 1::int AS bucket_order, '0-499ms'::text AS bucket_label, 0::bigint AS lower_bound_ms, 500::bigint AS upper_bound_exclusive_ms
            UNION ALL SELECT 2, '500-999ms', 500, 1000
            UNION ALL SELECT 3, '1000-1999ms', 1000, 2000
            UNION ALL SELECT 4, '2000-4999ms', 2000, 5000
            UNION ALL SELECT 5, '5000-9999ms', 5000, 10000
            UNION ALL SELECT 6, '10000ms+', 10000, NULL::bigint
        )
        SELECT b.bucket_label,
               b.lower_bound_ms,
               b.upper_bound_exclusive_ms,
               COUNT(dw.duration_ms)::bigint AS run_count
        FROM buckets b
        LEFT JOIN duration_window dw
          ON dw.duration_ms >= b.lower_bound_ms
         AND (b.upper_bound_exclusive_ms IS NULL OR dw.duration_ms < b.upper_bound_exclusive_ms)
        GROUP BY b.bucket_order, b.bucket_label, b.lower_bound_ms, b.upper_bound_exclusive_ms
        ORDER BY b.bucket_order ASC
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TenantRunLatencyHistogramBucket {
            bucket_label: row.get("bucket_label"),
            lower_bound_ms: row.get("lower_bound_ms"),
            upper_bound_exclusive_ms: row.get("upper_bound_exclusive_ms"),
            run_count: row.get("run_count"),
        })
        .collect())
}

pub async fn get_tenant_run_latency_traces(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
    limit: i64,
) -> Result<Vec<TenantRunLatencyTraceRecord>, sqlx::Error> {
    let safe_limit = limit.clamp(1, 5000);
    let rows = sqlx::query(
        r#"
        SELECT id AS run_id,
               status,
               GREATEST(
                 (EXTRACT(EPOCH FROM (finished_at - started_at)) * 1000.0)::bigint,
                 0
               ) AS duration_ms,
               started_at,
               finished_at
        FROM runs
        WHERE tenant_id = $1
          AND finished_at IS NOT NULL
          AND started_at IS NOT NULL
          AND finished_at >= $2
        ORDER BY finished_at DESC
        LIMIT $3
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .bind(safe_limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TenantRunLatencyTraceRecord {
            run_id: row.get("run_id"),
            status: row.get("status"),
            duration_ms: row.get("duration_ms"),
            started_at: row.get("started_at"),
            finished_at: row.get("finished_at"),
        })
        .collect())
}

pub async fn get_tenant_action_latency_summary(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<Vec<TenantActionLatencyRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH action_window AS (
            SELECT ar.id AS action_request_id,
                   ar.action_type AS action_type,
                   ar.status AS action_status,
                   GREATEST(
                     (
                       EXTRACT(
                         EPOCH FROM (
                           COALESCE(ar_latest.executed_at, ar.created_at) - ar.created_at
                         )
                       ) * 1000.0
                     )::bigint,
                     0
                   ) AS duration_ms
            FROM action_requests ar
            JOIN steps s ON s.id = ar.step_id
            JOIN runs r ON r.id = s.run_id
            LEFT JOIN LATERAL (
              SELECT executed_at
              FROM action_results
              WHERE action_request_id = ar.id
              ORDER BY executed_at DESC
              LIMIT 1
            ) ar_latest ON true
            WHERE r.tenant_id = $1
              AND ar.created_at >= $2
        )
        SELECT action_type,
               COUNT(action_request_id)::bigint AS total_count,
               AVG(duration_ms)::double precision AS avg_duration_ms,
               percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms)::double precision AS p95_duration_ms,
               MAX(duration_ms)::bigint AS max_duration_ms,
               SUM(CASE WHEN action_status = 'failed' THEN 1 ELSE 0 END)::bigint AS failed_count,
               SUM(CASE WHEN action_status = 'denied' THEN 1 ELSE 0 END)::bigint AS denied_count
        FROM action_window
        GROUP BY action_type
        ORDER BY total_count DESC, action_type ASC
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TenantActionLatencyRecord {
            action_type: row.get("action_type"),
            total_count: row.get("total_count"),
            avg_duration_ms: row.get("avg_duration_ms"),
            p95_duration_ms: row.get("p95_duration_ms"),
            max_duration_ms: row.get("max_duration_ms"),
            failed_count: row.get("failed_count"),
            denied_count: row.get("denied_count"),
        })
        .collect())
}

pub async fn get_tenant_llm_gateway_lane_summary(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<Vec<TenantLlmGatewayLaneSummaryRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH llm_window AS (
            SELECT COALESCE(
                     NULLIF(ar.result_json #>> '{gateway,request_class}', ''),
                     'interactive'
                   ) AS request_class,
                   COALESCE(
                     NULLIF(ar.result_json #>> '{gateway,cache_status}', ''),
                     'unknown'
                   ) AS cache_status,
                   COALESCE(
                     NULLIF(ar.result_json #>> '{gateway,admission_status}', ''),
                     'unknown'
                   ) AS admission_status,
                   COALESCE(
                     NULLIF(ar.result_json #>> '{gateway,slo_status}', ''),
                     'not_configured'
                   ) AS slo_status,
                   COALESCE(
                     NULLIF(ar.result_json #>> '{gateway,verifier_escalated}', ''),
                     'false'
                   )::boolean AS verifier_escalated,
                   GREATEST(
                     (
                       EXTRACT(
                         EPOCH FROM (
                           COALESCE(ar.executed_at, req.created_at) - req.created_at
                         )
                       ) * 1000.0
                     )::bigint,
                     0
                   ) AS duration_ms
            FROM action_requests req
            JOIN action_results ar ON ar.action_request_id = req.id
            JOIN steps s ON s.id = req.step_id
            JOIN runs r ON r.id = s.run_id
            WHERE r.tenant_id = $1
              AND req.action_type = 'llm.infer'
              AND req.created_at >= $2
              AND ar.status = 'executed'
              AND ar.result_json IS NOT NULL
        )
        SELECT request_class,
               COUNT(*)::bigint AS total_count,
               percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms)::double precision AS p95_duration_ms,
               AVG(duration_ms)::double precision AS avg_duration_ms,
               SUM(CASE WHEN cache_status = 'hit' OR cache_status = 'distributed_hit' THEN 1 ELSE 0 END)::bigint AS cache_hit_count,
               SUM(CASE WHEN cache_status = 'distributed_hit' THEN 1 ELSE 0 END)::bigint AS distributed_cache_hit_count,
               SUM(CASE WHEN verifier_escalated THEN 1 ELSE 0 END)::bigint AS verifier_escalated_count,
               SUM(CASE WHEN slo_status = 'warn' THEN 1 ELSE 0 END)::bigint AS slo_warn_count,
               SUM(CASE WHEN slo_status = 'breach' THEN 1 ELSE 0 END)::bigint AS slo_breach_count,
               SUM(CASE WHEN admission_status = 'distributed_fail_open_local' THEN 1 ELSE 0 END)::bigint AS distributed_fail_open_count
        FROM llm_window
        GROUP BY request_class
        ORDER BY request_class ASC
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TenantLlmGatewayLaneSummaryRecord {
            request_class: row.get("request_class"),
            total_count: row.get("total_count"),
            p95_duration_ms: row.get("p95_duration_ms"),
            avg_duration_ms: row.get("avg_duration_ms"),
            cache_hit_count: row.get("cache_hit_count"),
            distributed_cache_hit_count: row.get("distributed_cache_hit_count"),
            verifier_escalated_count: row.get("verifier_escalated_count"),
            slo_warn_count: row.get("slo_warn_count"),
            slo_breach_count: row.get("slo_breach_count"),
            distributed_fail_open_count: row.get("distributed_fail_open_count"),
        })
        .collect())
}

pub async fn get_tenant_action_latency_traces(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
    action_type: Option<&str>,
    limit: i64,
) -> Result<Vec<TenantActionLatencyTraceRecord>, sqlx::Error> {
    let safe_limit = limit.clamp(1, 5000);
    let rows = sqlx::query(
        r#"
        SELECT ar.id AS action_request_id,
               s.run_id AS run_id,
               ar.step_id AS step_id,
               ar.action_type AS action_type,
               ar.status AS status,
               GREATEST(
                 (
                   EXTRACT(
                     EPOCH FROM (
                       COALESCE(ar_latest.executed_at, ar.created_at) - ar.created_at
                     )
                   ) * 1000.0
                 )::bigint,
                 0
               ) AS duration_ms,
               ar.created_at AS created_at,
               ar_latest.executed_at AS executed_at
        FROM action_requests ar
        JOIN steps s ON s.id = ar.step_id
        JOIN runs r ON r.id = s.run_id
        LEFT JOIN LATERAL (
          SELECT executed_at
          FROM action_results
          WHERE action_request_id = ar.id
          ORDER BY executed_at DESC
          LIMIT 1
        ) ar_latest ON true
        WHERE r.tenant_id = $1
          AND ar.created_at >= $2
          AND ($3::text IS NULL OR ar.action_type = $3)
        ORDER BY ar.created_at DESC, ar.id DESC
        LIMIT $4
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .bind(action_type)
    .bind(safe_limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TenantActionLatencyTraceRecord {
            action_request_id: row.get("action_request_id"),
            run_id: row.get("run_id"),
            step_id: row.get("step_id"),
            action_type: row.get("action_type"),
            status: row.get("status"),
            duration_ms: row.get("duration_ms"),
            created_at: row.get("created_at"),
            executed_at: row.get("executed_at"),
        })
        .collect())
}

pub async fn create_memory_record(
    pool: &PgPool,
    new_record: &NewMemoryRecord,
) -> Result<MemoryRecord, sqlx::Error> {
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
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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
    .bind(new_record.id)
    .bind(&new_record.tenant_id)
    .bind(new_record.agent_id)
    .bind(new_record.run_id)
    .bind(new_record.step_id)
    .bind(&new_record.memory_kind)
    .bind(&new_record.scope)
    .bind(&new_record.content_json)
    .bind(&new_record.summary_text)
    .bind(&new_record.source)
    .bind(new_record.redaction_applied)
    .bind(new_record.expires_at)
    .fetch_one(pool)
    .await?;

    Ok(MemoryRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        run_id: row.get("run_id"),
        step_id: row.get("step_id"),
        memory_kind: row.get("memory_kind"),
        scope: row.get("scope"),
        content_json: row.get("content_json"),
        summary_text: row.get("summary_text"),
        source: row.get("source"),
        redaction_applied: row.get("redaction_applied"),
        expires_at: row.get("expires_at"),
        compacted_at: row.get("compacted_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn list_tenant_memory_records(
    pool: &PgPool,
    tenant_id: &str,
    agent_id: Option<Uuid>,
    memory_kind: Option<&str>,
    scope_prefix: Option<&str>,
    limit: i64,
) -> Result<Vec<MemoryRecord>, sqlx::Error> {
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
        WHERE tenant_id = $1
          AND compacted_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
          AND ($2::uuid IS NULL OR agent_id = $2)
          AND ($3::text IS NULL OR memory_kind = $3)
          AND ($4::text IS NULL OR scope LIKE ($4 || '%'))
        ORDER BY created_at DESC, id DESC
        LIMIT $5
        "#,
    )
    .bind(tenant_id)
    .bind(agent_id)
    .bind(memory_kind)
    .bind(scope_prefix)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| MemoryRecord {
            id: row.get("id"),
            tenant_id: row.get("tenant_id"),
            agent_id: row.get("agent_id"),
            run_id: row.get("run_id"),
            step_id: row.get("step_id"),
            memory_kind: row.get("memory_kind"),
            scope: row.get("scope"),
            content_json: row.get("content_json"),
            summary_text: row.get("summary_text"),
            source: row.get("source"),
            redaction_applied: row.get("redaction_applied"),
            expires_at: row.get("expires_at"),
            compacted_at: row.get("compacted_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

pub async fn list_tenant_handoff_memory_records(
    pool: &PgPool,
    tenant_id: &str,
    to_agent_id: Option<Uuid>,
    from_agent_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<MemoryRecord>, sqlx::Error> {
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
        WHERE tenant_id = $1
          AND memory_kind = 'handoff'
          AND scope LIKE 'memory:handoff/%'
          AND compacted_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
          AND ($2::uuid IS NULL OR agent_id = $2)
          AND (
            $3::uuid IS NULL
            OR content_json->>'from_agent_id' = $3::text
          )
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(tenant_id)
    .bind(to_agent_id)
    .bind(from_agent_id)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| MemoryRecord {
            id: row.get("id"),
            tenant_id: row.get("tenant_id"),
            agent_id: row.get("agent_id"),
            run_id: row.get("run_id"),
            step_id: row.get("step_id"),
            memory_kind: row.get("memory_kind"),
            scope: row.get("scope"),
            content_json: row.get("content_json"),
            summary_text: row.get("summary_text"),
            source: row.get("source"),
            redaction_applied: row.get("redaction_applied"),
            expires_at: row.get("expires_at"),
            compacted_at: row.get("compacted_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

pub async fn create_memory_compaction_record(
    pool: &PgPool,
    new_record: &NewMemoryCompactionRecord,
) -> Result<MemoryCompactionRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO memory_compactions (
            id,
            tenant_id,
            agent_id,
            memory_kind,
            scope,
            source_count,
            source_entry_ids,
            summary_json
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id,
                  tenant_id,
                  agent_id,
                  memory_kind,
                  scope,
                  source_count,
                  source_entry_ids,
                  summary_json,
                  created_at
        "#,
    )
    .bind(new_record.id)
    .bind(&new_record.tenant_id)
    .bind(new_record.agent_id)
    .bind(&new_record.memory_kind)
    .bind(&new_record.scope)
    .bind(new_record.source_count)
    .bind(&new_record.source_entry_ids)
    .bind(&new_record.summary_json)
    .fetch_one(pool)
    .await?;

    Ok(MemoryCompactionRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        memory_kind: row.get("memory_kind"),
        scope: row.get("scope"),
        source_count: row.get("source_count"),
        source_entry_ids: row.get("source_entry_ids"),
        summary_json: row.get("summary_json"),
        created_at: row.get("created_at"),
    })
}

#[derive(Debug, Clone)]
struct MemoryCompactionCandidate {
    tenant_id: String,
    agent_id: Uuid,
    memory_kind: String,
    scope: String,
    source_count: i64,
    source_entry_ids: Value,
    representative_run_id: Option<Uuid>,
    representative_step_id: Option<Uuid>,
}

pub async fn compact_memory_records(
    pool: &PgPool,
    older_than_or_equal: OffsetDateTime,
    min_records: i64,
    max_groups: i64,
) -> Result<MemoryCompactionRunStats, sqlx::Error> {
    let min_records = min_records.max(2);
    let max_groups = max_groups.max(1);

    let candidate_rows = sqlx::query(
        r#"
        SELECT tenant_id,
               agent_id,
               memory_kind,
               scope,
               COUNT(*)::bigint AS source_count,
               jsonb_agg(id ORDER BY created_at ASC, id ASC) AS source_entry_ids,
               (ARRAY_AGG(run_id ORDER BY created_at DESC, id DESC))[1] AS representative_run_id,
               (ARRAY_AGG(step_id ORDER BY created_at DESC, id DESC))[1] AS representative_step_id
        FROM memory_records
        WHERE compacted_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
          AND created_at <= $1
        GROUP BY tenant_id, agent_id, memory_kind, scope
        HAVING COUNT(*) >= $2
        ORDER BY MIN(created_at) ASC
        LIMIT $3
        "#,
    )
    .bind(older_than_or_equal)
    .bind(min_records)
    .bind(max_groups)
    .fetch_all(pool)
    .await?;

    let candidates: Vec<MemoryCompactionCandidate> = candidate_rows
        .into_iter()
        .map(|row| MemoryCompactionCandidate {
            tenant_id: row.get("tenant_id"),
            agent_id: row.get("agent_id"),
            memory_kind: row.get("memory_kind"),
            scope: row.get("scope"),
            source_count: row.get("source_count"),
            source_entry_ids: row.get("source_entry_ids"),
            representative_run_id: row.get("representative_run_id"),
            representative_step_id: row.get("representative_step_id"),
        })
        .collect();

    let mut processed_groups = 0i64;
    let mut compacted_source_records = 0i64;
    let mut outcomes = Vec::new();

    for candidate in candidates {
        let source_ids = parse_uuid_json_array(&candidate.source_entry_ids);
        if source_ids.len() < min_records as usize {
            continue;
        }

        let updated_rows = sqlx::query(
            r#"
            UPDATE memory_records
            SET compacted_at = now(),
                updated_at = now()
            WHERE id = ANY($1::uuid[])
              AND compacted_at IS NULL
            RETURNING id
            "#,
        )
        .bind(source_ids.as_slice())
        .fetch_all(pool)
        .await?;

        if updated_rows.is_empty() {
            continue;
        }

        let compacted_ids: Vec<Uuid> = updated_rows.into_iter().map(|row| row.get("id")).collect();
        let compacted_count = compacted_ids.len() as i64;
        if compacted_count < min_records {
            continue;
        }

        let source_entry_ids = Value::Array(
            compacted_ids
                .iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        );
        let summary_json = json!({
            "memory_kind": candidate.memory_kind,
            "scope": candidate.scope,
            "candidate_source_count": candidate.source_count,
            "source_count": compacted_count,
            "generated_at": OffsetDateTime::now_utc(),
        });

        let _ = create_memory_compaction_record(
            pool,
            &NewMemoryCompactionRecord {
                id: Uuid::new_v4(),
                tenant_id: candidate.tenant_id.clone(),
                agent_id: Some(candidate.agent_id),
                memory_kind: candidate.memory_kind.clone(),
                scope: candidate.scope.clone(),
                source_count: compacted_count.clamp(1, i32::MAX as i64) as i32,
                source_entry_ids: source_entry_ids.clone(),
                summary_json,
            },
        )
        .await?;

        processed_groups += 1;
        compacted_source_records += compacted_count;
        outcomes.push(MemoryCompactionGroupOutcome {
            tenant_id: candidate.tenant_id,
            agent_id: candidate.agent_id,
            memory_kind: candidate.memory_kind,
            scope: candidate.scope,
            source_count: compacted_count,
            source_entry_ids,
            representative_run_id: candidate.representative_run_id,
            representative_step_id: candidate.representative_step_id,
        });
    }

    Ok(MemoryCompactionRunStats {
        processed_groups,
        compacted_source_records,
        groups: outcomes,
    })
}

pub async fn get_tenant_memory_compaction_stats(
    pool: &PgPool,
    tenant_id: &str,
    since: Option<OffsetDateTime>,
) -> Result<MemoryCompactionStatsRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
          (SELECT COUNT(*)::bigint
           FROM memory_compactions
           WHERE tenant_id = $1
             AND ($2::timestamptz IS NULL OR created_at >= $2)) AS compacted_groups_window,
          (SELECT COALESCE(SUM(source_count), 0)::bigint
           FROM memory_compactions
           WHERE tenant_id = $1
             AND ($2::timestamptz IS NULL OR created_at >= $2)) AS compacted_source_records_window,
          (SELECT COUNT(*)::bigint
           FROM memory_records
           WHERE tenant_id = $1
             AND compacted_at IS NULL) AS pending_uncompacted_records,
          (SELECT MAX(created_at)
           FROM memory_compactions
           WHERE tenant_id = $1) AS last_compacted_at
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_one(pool)
    .await?;

    Ok(MemoryCompactionStatsRecord {
        compacted_groups_window: row.get("compacted_groups_window"),
        compacted_source_records_window: row.get("compacted_source_records_window"),
        pending_uncompacted_records: row.get("pending_uncompacted_records"),
        last_compacted_at: row.get("last_compacted_at"),
    })
}

pub async fn purge_expired_tenant_memory_records(
    pool: &PgPool,
    tenant_id: &str,
    as_of: OffsetDateTime,
) -> Result<MemoryPurgeOutcome, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT tenant_id,
               deleted_count,
               as_of
        FROM purge_expired_memory_records($1, $2)
        "#,
    )
    .bind(tenant_id)
    .bind(as_of)
    .fetch_one(pool)
    .await?;

    Ok(MemoryPurgeOutcome {
        tenant_id: row.get("tenant_id"),
        deleted_count: row.get("deleted_count"),
        as_of: row.get("as_of"),
    })
}

fn parse_uuid_json_array(value: &Value) -> Vec<Uuid> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|raw| Uuid::parse_str(raw).ok())
        .collect()
}

pub async fn sum_executed_payment_amount_msat_for_tenant(
    pool: &PgPool,
    tenant_id: &str,
) -> Result<i64, sqlx::Error> {
    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(amount_msat), 0)::bigint
        FROM payment_requests
        WHERE tenant_id = $1
          AND operation = 'pay_invoice'
          AND status = 'executed'
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    Ok(total)
}

pub async fn sum_executed_payment_amount_msat_for_agent(
    pool: &PgPool,
    tenant_id: &str,
    agent_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(amount_msat), 0)::bigint
        FROM payment_requests
        WHERE tenant_id = $1
          AND agent_id = $2
          AND operation = 'pay_invoice'
          AND status = 'executed'
        "#,
    )
    .bind(tenant_id)
    .bind(agent_id)
    .fetch_one(pool)
    .await?;
    Ok(total)
}

pub async fn update_payment_request_status(
    pool: &PgPool,
    payment_request_id: Uuid,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE payment_requests
        SET status = $2,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(payment_request_id)
    .bind(status)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn create_llm_token_usage_record(
    pool: &PgPool,
    new_record: &NewLlmTokenUsageRecord,
) -> Result<LlmTokenUsageRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO llm_token_usage (
            id,
            run_id,
            action_request_id,
            tenant_id,
            agent_id,
            route,
            model_key,
            consumed_tokens,
            estimated_cost_usd,
            window_started_at,
            window_duration_seconds
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING id,
                  run_id,
                  action_request_id,
                  tenant_id,
                  agent_id,
                  route,
                  model_key,
                  consumed_tokens,
                  estimated_cost_usd,
                  window_started_at,
                  window_duration_seconds,
                  created_at
        "#,
    )
    .bind(new_record.id)
    .bind(new_record.run_id)
    .bind(new_record.action_request_id)
    .bind(&new_record.tenant_id)
    .bind(new_record.agent_id)
    .bind(&new_record.route)
    .bind(&new_record.model_key)
    .bind(new_record.consumed_tokens)
    .bind(new_record.estimated_cost_usd)
    .bind(new_record.window_started_at)
    .bind(new_record.window_duration_seconds)
    .fetch_one(pool)
    .await?;

    Ok(LlmTokenUsageRecord {
        id: row.get("id"),
        run_id: row.get("run_id"),
        action_request_id: row.get("action_request_id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        route: row.get("route"),
        model_key: row.get("model_key"),
        consumed_tokens: row.get("consumed_tokens"),
        estimated_cost_usd: row.get("estimated_cost_usd"),
        window_started_at: row.get("window_started_at"),
        window_duration_seconds: row.get("window_duration_seconds"),
        created_at: row.get("created_at"),
    })
}

pub async fn sum_llm_consumed_tokens_for_tenant_since(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0)::bigint
        FROM llm_token_usage
        WHERE tenant_id = $1
          AND route = 'remote'
          AND created_at >= $2
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_one(pool)
    .await
}

pub async fn sum_llm_consumed_tokens_for_agent_since(
    pool: &PgPool,
    tenant_id: &str,
    agent_id: Uuid,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0)::bigint
        FROM llm_token_usage
        WHERE tenant_id = $1
          AND agent_id = $2
          AND route = 'remote'
          AND created_at >= $3
        "#,
    )
    .bind(tenant_id)
    .bind(agent_id)
    .bind(since)
    .fetch_one(pool)
    .await
}

pub async fn sum_llm_consumed_tokens_for_model_since(
    pool: &PgPool,
    tenant_id: &str,
    model_key: &str,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0)::bigint
        FROM llm_token_usage
        WHERE tenant_id = $1
          AND model_key = $2
          AND route = 'remote'
          AND created_at >= $3
        "#,
    )
    .bind(tenant_id)
    .bind(model_key)
    .bind(since)
    .fetch_one(pool)
    .await
}

pub async fn get_llm_usage_totals_since(
    pool: &PgPool,
    tenant_id: &str,
    since: OffsetDateTime,
    agent_id: Option<Uuid>,
    model_key: Option<&str>,
) -> Result<(i64, f64), sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0)::bigint AS tokens,
               COALESCE(SUM(estimated_cost_usd), 0)::double precision AS estimated_cost_usd
        FROM llm_token_usage
        WHERE tenant_id = $1
          AND route = 'remote'
          AND created_at >= $2
          AND ($3::uuid IS NULL OR agent_id = $3)
          AND ($4::text IS NULL OR model_key = $4)
        "#,
    )
    .bind(tenant_id)
    .bind(since)
    .bind(agent_id)
    .bind(model_key)
    .fetch_one(pool)
    .await?;
    Ok((row.get("tokens"), row.get("estimated_cost_usd")))
}

pub async fn try_acquire_llm_gateway_admission_lease(
    pool: &PgPool,
    params: &LlmGatewayAdmissionLeaseAcquireParams,
) -> Result<Option<LlmGatewayAdmissionLeaseRecord>, sqlx::Error> {
    if params.max_inflight <= 0 {
        return Ok(None);
    }
    let lease_ms = clamp_lease_ms(params.lease_for);
    let row = sqlx::query(
        r#"
        WITH candidate AS (
            SELECT slot_idx
            FROM generate_series(1, $3::int) AS slot_idx
            WHERE NOT EXISTS (
                SELECT 1
                FROM llm_gateway_admission_leases active
                WHERE active.namespace = $1
                  AND active.lane = $2
                  AND active.slot_index = slot_idx
                  AND active.lease_expires_at > now()
            )
            ORDER BY slot_idx
            LIMIT 1
        )
        INSERT INTO llm_gateway_admission_leases (
            namespace,
            lane,
            slot_index,
            lease_id,
            lease_owner,
            lease_expires_at
        )
        SELECT
            $1,
            $2,
            candidate.slot_idx,
            $4,
            $5,
            now() + ($6::bigint * interval '1 millisecond')
        FROM candidate
        ON CONFLICT (namespace, lane, slot_index) DO UPDATE
            SET lease_id = EXCLUDED.lease_id,
                lease_owner = EXCLUDED.lease_owner,
                lease_expires_at = EXCLUDED.lease_expires_at,
                updated_at = now()
        WHERE llm_gateway_admission_leases.lease_expires_at <= now()
        RETURNING namespace, lane, slot_index, lease_id, lease_owner, lease_expires_at
        "#,
    )
    .bind(&params.namespace)
    .bind(&params.lane)
    .bind(params.max_inflight)
    .bind(params.lease_id)
    .bind(&params.lease_owner)
    .bind(lease_ms)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| LlmGatewayAdmissionLeaseRecord {
        namespace: row.get("namespace"),
        lane: row.get("lane"),
        slot_index: row.get("slot_index"),
        lease_id: row.get("lease_id"),
        lease_owner: row.get("lease_owner"),
        lease_expires_at: row.get("lease_expires_at"),
    }))
}

pub async fn release_llm_gateway_admission_lease(
    pool: &PgPool,
    lease: &LlmGatewayAdmissionLeaseRecord,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM llm_gateway_admission_leases
        WHERE namespace = $1
          AND lane = $2
          AND slot_index = $3
          AND lease_id = $4
        "#,
    )
    .bind(&lease.namespace)
    .bind(&lease.lane)
    .bind(lease.slot_index)
    .bind(lease.lease_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn get_llm_gateway_cache_entry(
    pool: &PgPool,
    namespace: &str,
    cache_key_sha256: &str,
) -> Result<Option<LlmGatewayCacheEntryRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT cache_key_sha256,
               namespace,
               route,
               model,
               response_json,
               expires_at,
               created_at,
               updated_at
        FROM llm_gateway_cache_entries
        WHERE namespace = $1
          AND cache_key_sha256 = $2
          AND expires_at > now()
        "#,
    )
    .bind(namespace)
    .bind(cache_key_sha256)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| LlmGatewayCacheEntryRecord {
        cache_key_sha256: row.get("cache_key_sha256"),
        namespace: row.get("namespace"),
        route: row.get("route"),
        model: row.get("model"),
        response_json: row.get("response_json"),
        expires_at: row.get("expires_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }))
}

pub async fn upsert_llm_gateway_cache_entry(
    pool: &PgPool,
    entry: &NewLlmGatewayCacheEntry,
) -> Result<LlmGatewayCacheEntryRecord, sqlx::Error> {
    let ttl_ms = clamp_lease_ms(entry.ttl);
    let row = sqlx::query(
        r#"
        INSERT INTO llm_gateway_cache_entries (
            cache_key_sha256,
            namespace,
            route,
            model,
            response_json,
            expires_at
        )
        VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            now() + ($6::bigint * interval '1 millisecond')
        )
        ON CONFLICT (cache_key_sha256) DO UPDATE
            SET namespace = EXCLUDED.namespace,
                route = EXCLUDED.route,
                model = EXCLUDED.model,
                response_json = EXCLUDED.response_json,
                expires_at = EXCLUDED.expires_at,
                updated_at = now()
        RETURNING cache_key_sha256,
                  namespace,
                  route,
                  model,
                  response_json,
                  expires_at,
                  created_at,
                  updated_at
        "#,
    )
    .bind(&entry.cache_key_sha256)
    .bind(&entry.namespace)
    .bind(&entry.route)
    .bind(&entry.model)
    .bind(&entry.response_json)
    .bind(ttl_ms)
    .fetch_one(pool)
    .await?;

    Ok(LlmGatewayCacheEntryRecord {
        cache_key_sha256: row.get("cache_key_sha256"),
        namespace: row.get("namespace"),
        route: row.get("route"),
        model: row.get("model"),
        response_json: row.get("response_json"),
        expires_at: row.get("expires_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn prune_llm_gateway_cache_namespace(
    pool: &PgPool,
    namespace: &str,
    max_entries: i64,
) -> Result<u64, sqlx::Error> {
    let expired = sqlx::query(
        r#"
        DELETE FROM llm_gateway_cache_entries
        WHERE namespace = $1
          AND expires_at <= now()
        "#,
    )
    .bind(namespace)
    .execute(pool)
    .await?;
    if max_entries <= 0 {
        return Ok(expired.rows_affected());
    }

    let overflow = sqlx::query(
        r#"
        DELETE FROM llm_gateway_cache_entries
        WHERE namespace = $1
          AND cache_key_sha256 IN (
              SELECT cache_key_sha256
              FROM llm_gateway_cache_entries
              WHERE namespace = $1
              ORDER BY updated_at DESC
              OFFSET $2
          )
        "#,
    )
    .bind(namespace)
    .bind(max_entries)
    .execute(pool)
    .await?;

    Ok(expired.rows_affected() + overflow.rows_affected())
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

pub async fn append_trigger_audit_event(
    pool: &PgPool,
    new_event: &NewTriggerAuditEvent,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO trigger_audit_events (
            id,
            trigger_id,
            tenant_id,
            actor,
            event_type,
            payload_json
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(new_event.id)
    .bind(new_event.trigger_id)
    .bind(&new_event.tenant_id)
    .bind(&new_event.actor)
    .bind(&new_event.event_type)
    .bind(&new_event.payload_json)
    .execute(pool)
    .await?;
    Ok(())
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

pub async fn list_tenant_compliance_audit_events(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    event_type: Option<&str>,
    limit: i64,
) -> Result<Vec<ComplianceAuditEventDetailRecord>, sqlx::Error> {
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
               created_at,
               recorded_at
        FROM compliance_audit_events
        WHERE tenant_id = $1
          AND ($2::uuid IS NULL OR run_id = $2)
          AND ($3::text IS NULL OR event_type = $3)
        ORDER BY tamper_chain_seq ASC
        LIMIT $4
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .bind(event_type)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?;

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

    Ok(rows
        .into_iter()
        .map(|row| {
            let payload_json: Value = row.get("payload_json");
            ComplianceAuditEventDetailRecord {
                id: row.get("id"),
                source_audit_event_id: row.get("source_audit_event_id"),
                tamper_chain_seq: row.get("tamper_chain_seq"),
                tamper_prev_hash: row.get("tamper_prev_hash"),
                tamper_hash: row.get("tamper_hash"),
                run_id: row.get("run_id"),
                step_id: row.get("step_id"),
                tenant_id: row.get("tenant_id"),
                agent_id: row.get("agent_id"),
                user_id: row.get("user_id"),
                actor: row.get("actor"),
                event_type: row.get("event_type"),
                request_id: payload_string_field(
                    &payload_json,
                    &["request_id", "http_request_id", "correlation_request_id"],
                ),
                session_id: payload_string_field(
                    &payload_json,
                    &["session_id", "correlation_session_id"],
                ),
                action_request_id: payload_uuid_field(&payload_json, &["action_request_id"]),
                payment_request_id: payload_uuid_field(&payload_json, &["payment_request_id"]),
                payload_json,
                created_at: row.get("created_at"),
                recorded_at: row.get("recorded_at"),
            }
        })
        .collect())
}

pub async fn verify_tenant_compliance_audit_chain(
    pool: &PgPool,
    tenant_id: &str,
) -> Result<ComplianceAuditTamperVerificationRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT tenant_id,
               checked_events,
               verified,
               first_invalid_event_id,
               latest_chain_seq,
               latest_tamper_hash
        FROM verify_compliance_audit_chain($1)
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;

    Ok(ComplianceAuditTamperVerificationRecord {
        tenant_id: row.get("tenant_id"),
        checked_events: row.get("checked_events"),
        verified: row.get("verified"),
        first_invalid_event_id: row.get("first_invalid_event_id"),
        latest_chain_seq: row.get("latest_chain_seq"),
        latest_tamper_hash: row.get("latest_tamper_hash"),
    })
}

pub async fn get_tenant_compliance_audit_policy(
    pool: &PgPool,
    tenant_id: &str,
) -> Result<ComplianceAuditPolicyRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT $1::text AS tenant_id,
               COALESCE(policy.compliance_hot_retention_days, 180) AS compliance_hot_retention_days,
               COALESCE(policy.compliance_archive_retention_days, 2555) AS compliance_archive_retention_days,
               COALESCE(policy.legal_hold, false) AS legal_hold,
               policy.legal_hold_reason,
               policy.updated_at
        FROM (SELECT 1) AS seed
        LEFT JOIN compliance_audit_policies policy
          ON policy.tenant_id = $1
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;

    Ok(ComplianceAuditPolicyRecord {
        tenant_id: row.get("tenant_id"),
        compliance_hot_retention_days: row.get("compliance_hot_retention_days"),
        compliance_archive_retention_days: row.get("compliance_archive_retention_days"),
        legal_hold: row.get("legal_hold"),
        legal_hold_reason: row.get("legal_hold_reason"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn upsert_tenant_compliance_audit_policy(
    pool: &PgPool,
    tenant_id: &str,
    compliance_hot_retention_days: i32,
    compliance_archive_retention_days: i32,
    legal_hold: bool,
    legal_hold_reason: Option<&str>,
) -> Result<ComplianceAuditPolicyRecord, sqlx::Error> {
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
        VALUES ($1, $2, $3, $4, $5, now())
        ON CONFLICT (tenant_id)
        DO UPDATE SET
            compliance_hot_retention_days = EXCLUDED.compliance_hot_retention_days,
            compliance_archive_retention_days = EXCLUDED.compliance_archive_retention_days,
            legal_hold = EXCLUDED.legal_hold,
            legal_hold_reason = EXCLUDED.legal_hold_reason,
            updated_at = now()
        RETURNING tenant_id,
                  compliance_hot_retention_days,
                  compliance_archive_retention_days,
                  legal_hold,
                  legal_hold_reason,
                  updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(compliance_hot_retention_days)
    .bind(compliance_archive_retention_days)
    .bind(legal_hold)
    .bind(legal_hold_reason)
    .fetch_one(pool)
    .await?;

    Ok(ComplianceAuditPolicyRecord {
        tenant_id: row.get("tenant_id"),
        compliance_hot_retention_days: row.get("compliance_hot_retention_days"),
        compliance_archive_retention_days: row.get("compliance_archive_retention_days"),
        legal_hold: row.get("legal_hold"),
        legal_hold_reason: row.get("legal_hold_reason"),
        updated_at: row.get("updated_at"),
    })
}

pub async fn purge_expired_tenant_compliance_audit_events(
    pool: &PgPool,
    tenant_id: &str,
    as_of: OffsetDateTime,
) -> Result<ComplianceAuditPurgeOutcome, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT tenant_id,
               deleted_count,
               legal_hold,
               cutoff_at,
               compliance_hot_retention_days,
               compliance_archive_retention_days
        FROM purge_expired_compliance_audit_events($1, $2)
        "#,
    )
    .bind(tenant_id)
    .bind(as_of)
    .fetch_one(pool)
    .await?;

    Ok(ComplianceAuditPurgeOutcome {
        tenant_id: row.get("tenant_id"),
        deleted_count: row.get("deleted_count"),
        legal_hold: row.get("legal_hold"),
        cutoff_at: row.get("cutoff_at"),
        compliance_hot_retention_days: row.get("compliance_hot_retention_days"),
        compliance_archive_retention_days: row.get("compliance_archive_retention_days"),
    })
}

fn compliance_siem_delivery_from_row(row: sqlx::postgres::PgRow) -> ComplianceSiemDeliveryRecord {
    ComplianceSiemDeliveryRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        run_id: row.get("run_id"),
        adapter: row.get("adapter"),
        delivery_target: row.get("delivery_target"),
        content_type: row.get("content_type"),
        payload_ndjson: row.get("payload_ndjson"),
        status: row.get("status"),
        attempts: row.get("attempts"),
        max_attempts: row.get("max_attempts"),
        next_attempt_at: row.get("next_attempt_at"),
        leased_by: row.get("leased_by"),
        lease_expires_at: row.get("lease_expires_at"),
        last_error: row.get("last_error"),
        last_http_status: row.get("last_http_status"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        delivered_at: row.get("delivered_at"),
    }
}

pub async fn create_compliance_siem_delivery_record(
    pool: &PgPool,
    new_record: &NewComplianceSiemDeliveryRecord,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
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
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id,
                  tenant_id,
                  run_id,
                  adapter,
                  delivery_target,
                  content_type,
                  payload_ndjson,
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
    .bind(new_record.id)
    .bind(&new_record.tenant_id)
    .bind(new_record.run_id)
    .bind(&new_record.adapter)
    .bind(&new_record.delivery_target)
    .bind(&new_record.content_type)
    .bind(&new_record.payload_ndjson)
    .bind(new_record.max_attempts.clamp(1, 100))
    .fetch_one(pool)
    .await?;

    Ok(compliance_siem_delivery_from_row(row))
}

pub async fn list_tenant_compliance_siem_delivery_records(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<ComplianceSiemDeliveryRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               run_id,
               adapter,
               delivery_target,
               content_type,
               payload_ndjson,
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
        WHERE tenant_id = $1
          AND ($2::uuid IS NULL OR run_id = $2)
          AND ($3::text IS NULL OR status = $3)
        ORDER BY created_at DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .bind(status)
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(compliance_siem_delivery_from_row)
        .collect())
}

pub async fn get_tenant_compliance_siem_delivery_summary(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Option<Uuid>,
) -> Result<ComplianceSiemDeliverySummaryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
          COUNT(*) FILTER (WHERE status = 'pending')::bigint AS pending_count,
          COUNT(*) FILTER (WHERE status = 'processing')::bigint AS processing_count,
          COUNT(*) FILTER (WHERE status = 'failed')::bigint AS failed_count,
          COUNT(*) FILTER (WHERE status = 'delivered')::bigint AS delivered_count,
          COUNT(*) FILTER (WHERE status = 'dead_lettered')::bigint AS dead_lettered_count,
          (
            SELECT EXTRACT(EPOCH FROM (now() - MIN(created_at)))
            FROM compliance_siem_delivery_outbox
            WHERE tenant_id = $1
              AND status = 'pending'
              AND ($2::uuid IS NULL OR run_id = $2)
          )::double precision AS oldest_pending_age_seconds
        FROM compliance_siem_delivery_outbox
        WHERE tenant_id = $1
          AND ($2::uuid IS NULL OR run_id = $2)
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .fetch_one(pool)
    .await?;

    Ok(ComplianceSiemDeliverySummaryRecord {
        pending_count: row.get("pending_count"),
        processing_count: row.get("processing_count"),
        failed_count: row.get("failed_count"),
        delivered_count: row.get("delivered_count"),
        dead_lettered_count: row.get("dead_lettered_count"),
        oldest_pending_age_seconds: row.get("oldest_pending_age_seconds"),
    })
}

pub async fn get_tenant_compliance_siem_delivery_slo(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    since: OffsetDateTime,
) -> Result<ComplianceSiemDeliverySloRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        WITH filtered AS (
          SELECT status, created_at
          FROM compliance_siem_delivery_outbox
          WHERE tenant_id = $1
            AND ($2::uuid IS NULL OR run_id = $2)
            AND created_at >= $3
        )
        SELECT
          COUNT(*)::bigint AS total_count,
          COUNT(*) FILTER (WHERE status = 'pending')::bigint AS pending_count,
          COUNT(*) FILTER (WHERE status = 'processing')::bigint AS processing_count,
          COUNT(*) FILTER (WHERE status = 'failed')::bigint AS failed_count,
          COUNT(*) FILTER (WHERE status = 'delivered')::bigint AS delivered_count,
          COUNT(*) FILTER (WHERE status = 'dead_lettered')::bigint AS dead_lettered_count,
          CASE
            WHEN COUNT(*) = 0 THEN NULL
            ELSE (COUNT(*) FILTER (WHERE status = 'delivered')::double precision * 100.0) / COUNT(*)::double precision
          END AS delivery_success_rate_pct,
          CASE
            WHEN COUNT(*) = 0 THEN NULL
            ELSE ((COUNT(*) FILTER (WHERE status IN ('failed', 'dead_lettered'))::double precision) * 100.0) / COUNT(*)::double precision
          END AS hard_failure_rate_pct,
          CASE
            WHEN COUNT(*) = 0 THEN NULL
            ELSE (COUNT(*) FILTER (WHERE status = 'dead_lettered')::double precision * 100.0) / COUNT(*)::double precision
          END AS dead_letter_rate_pct,
          (
            SELECT EXTRACT(EPOCH FROM (now() - MIN(created_at)))
            FROM filtered
            WHERE status = 'pending'
          )::double precision AS oldest_pending_age_seconds
        FROM filtered
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .bind(since)
    .fetch_one(pool)
    .await?;

    Ok(ComplianceSiemDeliverySloRecord {
        total_count: row.get("total_count"),
        pending_count: row.get("pending_count"),
        processing_count: row.get("processing_count"),
        failed_count: row.get("failed_count"),
        delivered_count: row.get("delivered_count"),
        dead_lettered_count: row.get("dead_lettered_count"),
        delivery_success_rate_pct: row.get("delivery_success_rate_pct"),
        hard_failure_rate_pct: row.get("hard_failure_rate_pct"),
        dead_letter_rate_pct: row.get("dead_letter_rate_pct"),
        oldest_pending_age_seconds: row.get("oldest_pending_age_seconds"),
    })
}

pub async fn list_tenant_compliance_siem_delivery_target_summaries(
    pool: &PgPool,
    tenant_id: &str,
    run_id: Option<Uuid>,
    since: Option<OffsetDateTime>,
    limit: i64,
) -> Result<Vec<ComplianceSiemDeliveryTargetSummaryRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH latest_target_attempt AS (
          SELECT DISTINCT ON (delivery_target)
            delivery_target,
            updated_at
          FROM compliance_siem_delivery_outbox
          WHERE tenant_id = $1
            AND ($2::uuid IS NULL OR run_id = $2)
            AND ($3::timestamptz IS NULL OR created_at >= $3)
          ORDER BY delivery_target, updated_at DESC, id DESC
        ),
        latest_target_error AS (
          SELECT DISTINCT ON (delivery_target)
            delivery_target,
            last_error,
            last_http_status
          FROM compliance_siem_delivery_outbox
          WHERE tenant_id = $1
            AND ($2::uuid IS NULL OR run_id = $2)
            AND ($3::timestamptz IS NULL OR created_at >= $3)
            AND last_error IS NOT NULL
          ORDER BY delivery_target, updated_at DESC, id DESC
        )
        SELECT
          outbox.delivery_target,
          COUNT(*)::bigint AS total_count,
          COUNT(*) FILTER (WHERE outbox.status = 'pending')::bigint AS pending_count,
          COUNT(*) FILTER (WHERE outbox.status = 'processing')::bigint AS processing_count,
          COUNT(*) FILTER (WHERE outbox.status = 'failed')::bigint AS failed_count,
          COUNT(*) FILTER (WHERE outbox.status = 'delivered')::bigint AS delivered_count,
          COUNT(*) FILTER (WHERE outbox.status = 'dead_lettered')::bigint AS dead_lettered_count,
          latest_error.last_error,
          latest_error.last_http_status,
          latest_attempt.updated_at AS last_attempt_at
        FROM compliance_siem_delivery_outbox outbox
        LEFT JOIN latest_target_attempt latest_attempt
          ON latest_attempt.delivery_target = outbox.delivery_target
        LEFT JOIN latest_target_error latest_error
          ON latest_error.delivery_target = outbox.delivery_target
        WHERE outbox.tenant_id = $1
          AND ($2::uuid IS NULL OR outbox.run_id = $2)
          AND ($3::timestamptz IS NULL OR outbox.created_at >= $3)
        GROUP BY outbox.delivery_target, latest_error.last_error, latest_error.last_http_status, latest_attempt.updated_at
        ORDER BY failed_count DESC, dead_lettered_count DESC, total_count DESC, outbox.delivery_target ASC
        LIMIT $4
        "#,
    )
    .bind(tenant_id)
    .bind(run_id)
    .bind(since)
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ComplianceSiemDeliveryTargetSummaryRecord {
            delivery_target: row.get("delivery_target"),
            total_count: row.get("total_count"),
            pending_count: row.get("pending_count"),
            processing_count: row.get("processing_count"),
            failed_count: row.get("failed_count"),
            delivered_count: row.get("delivered_count"),
            dead_lettered_count: row.get("dead_lettered_count"),
            last_error: row.get("last_error"),
            last_http_status: row.get("last_http_status"),
            last_attempt_at: row.get("last_attempt_at"),
        })
        .collect())
}

fn compliance_siem_delivery_alert_ack_from_row(
    row: sqlx::postgres::PgRow,
) -> ComplianceSiemDeliveryAlertAckRecord {
    ComplianceSiemDeliveryAlertAckRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        run_scope: row.get("run_scope"),
        delivery_target: row.get("delivery_target"),
        acknowledged_by_user_id: row.get("acknowledged_by_user_id"),
        acknowledged_by_role: row.get("acknowledged_by_role"),
        note: row.get("note"),
        created_at: row.get("created_at"),
        acknowledged_at: row.get("acknowledged_at"),
    }
}

pub async fn upsert_tenant_compliance_siem_delivery_alert_ack(
    pool: &PgPool,
    new_record: &NewComplianceSiemDeliveryAlertAckRecord,
) -> Result<ComplianceSiemDeliveryAlertAckRecord, sqlx::Error> {
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
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (tenant_id, run_scope, delivery_target)
        DO UPDATE SET
            acknowledged_by_user_id = EXCLUDED.acknowledged_by_user_id,
            acknowledged_by_role = EXCLUDED.acknowledged_by_role,
            note = EXCLUDED.note,
            acknowledged_at = now()
        RETURNING id,
                  tenant_id,
                  run_scope,
                  delivery_target,
                  acknowledged_by_user_id,
                  acknowledged_by_role,
                  note,
                  created_at,
                  acknowledged_at
        "#,
    )
    .bind(new_record.id)
    .bind(&new_record.tenant_id)
    .bind(&new_record.run_scope)
    .bind(&new_record.delivery_target)
    .bind(new_record.acknowledged_by_user_id)
    .bind(&new_record.acknowledged_by_role)
    .bind(&new_record.note)
    .fetch_one(pool)
    .await?;

    Ok(compliance_siem_delivery_alert_ack_from_row(row))
}

pub async fn list_tenant_compliance_siem_delivery_alert_acks(
    pool: &PgPool,
    tenant_id: &str,
    run_scope: &str,
    limit: i64,
) -> Result<Vec<ComplianceSiemDeliveryAlertAckRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               run_scope,
               delivery_target,
               acknowledged_by_user_id,
               acknowledged_by_role,
               note,
               created_at,
               acknowledged_at
        FROM compliance_siem_delivery_alert_acks
        WHERE tenant_id = $1
          AND run_scope = $2
        ORDER BY acknowledged_at DESC, id DESC
        LIMIT $3
        "#,
    )
    .bind(tenant_id)
    .bind(run_scope)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(compliance_siem_delivery_alert_ack_from_row)
        .collect())
}

pub async fn claim_pending_compliance_siem_delivery_records(
    pool: &PgPool,
    lease_owner: &str,
    lease_for: Duration,
    limit: i64,
) -> Result<Vec<ComplianceSiemDeliveryRecord>, sqlx::Error> {
    let lease_ms = lease_for
        .as_millis()
        .clamp(1, i64::MAX as u128)
        .try_into()
        .unwrap_or(i64::MAX);

    let rows = sqlx::query(
        r#"
        WITH claimable AS (
            SELECT id
            FROM compliance_siem_delivery_outbox
            WHERE status IN ('pending', 'failed')
              AND next_attempt_at <= now()
              AND (lease_expires_at IS NULL OR lease_expires_at <= now())
            ORDER BY next_attempt_at ASC, created_at ASC, id ASC
            LIMIT $3
            FOR UPDATE SKIP LOCKED
        )
        UPDATE compliance_siem_delivery_outbox outbox
        SET status = 'processing',
            leased_by = $1,
            lease_expires_at = now() + ($2::bigint * interval '1 millisecond'),
            updated_at = now()
        FROM claimable
        WHERE outbox.id = claimable.id
        RETURNING outbox.id,
                  outbox.tenant_id,
                  outbox.run_id,
                  outbox.adapter,
                  outbox.delivery_target,
                  outbox.content_type,
                  outbox.payload_ndjson,
                  outbox.status,
                  outbox.attempts,
                  outbox.max_attempts,
                  outbox.next_attempt_at,
                  outbox.leased_by,
                  outbox.lease_expires_at,
                  outbox.last_error,
                  outbox.last_http_status,
                  outbox.created_at,
                  outbox.updated_at,
                  outbox.delivered_at
        "#,
    )
    .bind(lease_owner)
    .bind(lease_ms)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(compliance_siem_delivery_from_row)
        .collect())
}

pub async fn mark_compliance_siem_delivery_record_delivered(
    pool: &PgPool,
    record_id: Uuid,
    http_status: Option<i32>,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET status = 'delivered',
            attempts = attempts + 1,
            last_error = NULL,
            last_http_status = $2,
            leased_by = NULL,
            lease_expires_at = NULL,
            delivered_at = now(),
            updated_at = now()
        WHERE id = $1
        RETURNING id,
                  tenant_id,
                  run_id,
                  adapter,
                  delivery_target,
                  content_type,
                  payload_ndjson,
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
    .bind(record_id)
    .bind(http_status)
    .fetch_one(pool)
    .await?;

    Ok(compliance_siem_delivery_from_row(row))
}

pub async fn mark_compliance_siem_delivery_record_failed(
    pool: &PgPool,
    record_id: Uuid,
    error_message: &str,
    http_status: Option<i32>,
    retry_at: OffsetDateTime,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET attempts = attempts + 1,
            status = CASE
              WHEN attempts + 1 >= max_attempts THEN 'dead_lettered'
              ELSE 'failed'
            END,
            last_error = $2,
            last_http_status = $3,
            leased_by = NULL,
            lease_expires_at = NULL,
            next_attempt_at = CASE
              WHEN attempts + 1 >= max_attempts THEN now()
              ELSE $4
            END,
            updated_at = now()
        WHERE id = $1
        RETURNING id,
                  tenant_id,
                  run_id,
                  adapter,
                  delivery_target,
                  content_type,
                  payload_ndjson,
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
    .bind(record_id)
    .bind(error_message)
    .bind(http_status)
    .bind(retry_at)
    .fetch_one(pool)
    .await?;

    Ok(compliance_siem_delivery_from_row(row))
}

pub async fn mark_compliance_siem_delivery_record_dead_lettered(
    pool: &PgPool,
    record_id: Uuid,
    error_message: &str,
    http_status: Option<i32>,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET attempts = attempts + 1,
            status = 'dead_lettered',
            last_error = $2,
            last_http_status = $3,
            leased_by = NULL,
            lease_expires_at = NULL,
            next_attempt_at = now(),
            updated_at = now()
        WHERE id = $1
        RETURNING id,
                  tenant_id,
                  run_id,
                  adapter,
                  delivery_target,
                  content_type,
                  payload_ndjson,
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
    .bind(record_id)
    .bind(error_message)
    .bind(http_status)
    .fetch_one(pool)
    .await?;

    Ok(compliance_siem_delivery_from_row(row))
}

pub async fn requeue_dead_letter_compliance_siem_delivery_record(
    pool: &PgPool,
    tenant_id: &str,
    record_id: Uuid,
    retry_at: OffsetDateTime,
) -> Result<Option<ComplianceSiemDeliveryRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET status = 'pending',
            attempts = 0,
            next_attempt_at = $3,
            leased_by = NULL,
            lease_expires_at = NULL,
            last_error = NULL,
            last_http_status = NULL,
            updated_at = now()
        WHERE id = $1
          AND tenant_id = $2
          AND status = 'dead_lettered'
        RETURNING id,
                  tenant_id,
                  run_id,
                  adapter,
                  delivery_target,
                  content_type,
                  payload_ndjson,
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
    .bind(record_id)
    .bind(tenant_id)
    .bind(retry_at)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(compliance_siem_delivery_from_row))
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
    claim_next_queued_run_with_limits(
        pool,
        worker_id,
        lease_for,
        i64::MAX,
        i64::MAX,
    )
    .await
}

pub async fn claim_next_queued_run_with_limits(
    pool: &PgPool,
    worker_id: &str,
    lease_for: Duration,
    global_max_inflight_runs: i64,
    tenant_max_inflight_runs: i64,
) -> Result<Option<RunLeaseRecord>, sqlx::Error> {
    let lease_ms = clamp_lease_ms(lease_for);
    let row = sqlx::query(
        r#"
        WITH candidate AS (
            SELECT id,
                   tenant_id,
                   CASE
                     WHEN lower(COALESCE(input_json->>'queue_class', input_json->>'llm_queue_class', 'interactive')) = 'batch'
                       AND created_at <= now() - interval '15 minutes' THEN 0
                     WHEN lower(COALESCE(input_json->>'queue_class', input_json->>'llm_queue_class', 'interactive')) = 'interactive' THEN 0
                     WHEN lower(COALESCE(input_json->>'queue_class', input_json->>'llm_queue_class', 'interactive')) = 'batch' THEN 1
                     ELSE 0
                   END AS queue_priority,
                   created_at
            FROM runs
            WHERE status = 'queued'
              AND (lease_expires_at IS NULL OR lease_expires_at < now())
            ORDER BY queue_priority, created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 64
        ),
        eligible AS (
            SELECT c.id
            FROM candidate c
            WHERE (SELECT COUNT(*)
                 FROM runs
              WHERE status = 'running') < $2
              AND (SELECT COUNT(*)
                   FROM runs
                   WHERE tenant_id = c.tenant_id
                     AND status = 'running') < $3
            ORDER BY c.queue_priority, c.created_at
            LIMIT 1
        )
        UPDATE runs
        SET status = 'running',
            started_at = COALESCE(started_at, now()),
            attempts = attempts + 1,
            lease_owner = $1,
            lease_expires_at = now() + ($4::bigint * interval '1 millisecond')
        WHERE id IN (SELECT id FROM eligible)
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
    .bind(global_max_inflight_runs.max(1))
    .bind(tenant_max_inflight_runs.max(1))
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

pub async fn try_acquire_scheduler_lease(
    pool: &PgPool,
    params: &SchedulerLeaseParams,
) -> Result<bool, sqlx::Error> {
    let lease_ms = clamp_lease_ms(params.lease_for);
    let acquired_owner: Option<String> = sqlx::query_scalar(
        r#"
        INSERT INTO scheduler_leases (
            lease_name,
            lease_owner,
            lease_expires_at
        )
        VALUES (
            $1,
            $2,
            now() + ($3::bigint * interval '1 millisecond')
        )
        ON CONFLICT (lease_name) DO UPDATE
            SET lease_owner = EXCLUDED.lease_owner,
                lease_expires_at = EXCLUDED.lease_expires_at,
                updated_at = now()
        WHERE scheduler_leases.lease_expires_at < now()
           OR scheduler_leases.lease_owner = EXCLUDED.lease_owner
        RETURNING lease_owner
        "#,
    )
    .bind(&params.lease_name)
    .bind(&params.lease_owner)
    .bind(lease_ms)
    .fetch_optional(pool)
    .await?;

    Ok(acquired_owner.as_deref() == Some(params.lease_owner.as_str()))
}

pub async fn create_interval_trigger(
    pool: &PgPool,
    new_trigger: &NewIntervalTrigger,
) -> Result<TriggerRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO triggers (
            id,
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
            webhook_secret_ref,
            input_json,
            requested_capabilities,
            granted_capabilities,
            next_fire_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'interval', $7, NULL, 'UTC', $8, $9, $10, $11, $12, $13, $14, $15, $16)
        RETURNING id,
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
        "#,
    )
    .bind(new_trigger.id)
    .bind(&new_trigger.tenant_id)
    .bind(new_trigger.agent_id)
    .bind(new_trigger.triggered_by_user_id)
    .bind(&new_trigger.recipe_id)
    .bind(&new_trigger.status)
    .bind(new_trigger.interval_seconds)
    .bind(&new_trigger.misfire_policy)
    .bind(new_trigger.max_attempts)
    .bind(new_trigger.max_inflight_runs)
    .bind(new_trigger.jitter_seconds)
    .bind(&new_trigger.webhook_secret_ref)
    .bind(&new_trigger.input_json)
    .bind(&new_trigger.requested_capabilities)
    .bind(&new_trigger.granted_capabilities)
    .bind(new_trigger.next_fire_at)
    .fetch_one(pool)
    .await?;

    Ok(trigger_from_row(&row))
}

pub async fn create_webhook_trigger(
    pool: &PgPool,
    new_trigger: &NewWebhookTrigger,
) -> Result<TriggerRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        INSERT INTO triggers (
            id,
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
            webhook_secret_ref,
            input_json,
            requested_capabilities,
            granted_capabilities,
            next_fire_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'webhook', NULL, NULL, 'UTC', 'fire_now', $7, $8, $9, $10, $11, $12, $13, now())
        RETURNING id,
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
        "#,
    )
    .bind(new_trigger.id)
    .bind(&new_trigger.tenant_id)
    .bind(new_trigger.agent_id)
    .bind(new_trigger.triggered_by_user_id)
    .bind(&new_trigger.recipe_id)
    .bind(&new_trigger.status)
    .bind(new_trigger.max_attempts)
    .bind(new_trigger.max_inflight_runs)
    .bind(new_trigger.jitter_seconds)
    .bind(&new_trigger.webhook_secret_ref)
    .bind(&new_trigger.input_json)
    .bind(&new_trigger.requested_capabilities)
    .bind(&new_trigger.granted_capabilities)
    .fetch_one(pool)
    .await?;

    Ok(trigger_from_row(&row))
}

pub async fn create_cron_trigger(
    pool: &PgPool,
    new_trigger: &NewCronTrigger,
) -> Result<TriggerRecord, sqlx::Error> {
    let next_fire_at = next_cron_fire_at(
        &new_trigger.cron_expression,
        &new_trigger.schedule_timezone,
        OffsetDateTime::now_utc(),
    )
    .map_err(sqlx::Error::Protocol)?;
    let next_fire_at = apply_jitter(
        next_fire_at,
        new_trigger.id,
        new_trigger.jitter_seconds,
        OffsetDateTime::now_utc(),
    );

    let row = sqlx::query(
        r#"
        INSERT INTO triggers (
            id,
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
            webhook_secret_ref,
            input_json,
            requested_capabilities,
            granted_capabilities,
            next_fire_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'cron', NULL, $7, $8, $9, $10, $11, $12, NULL, $13, $14, $15, $16)
        RETURNING id,
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
        "#,
    )
    .bind(new_trigger.id)
    .bind(&new_trigger.tenant_id)
    .bind(new_trigger.agent_id)
    .bind(new_trigger.triggered_by_user_id)
    .bind(&new_trigger.recipe_id)
    .bind(&new_trigger.status)
    .bind(&new_trigger.cron_expression)
    .bind(&new_trigger.schedule_timezone)
    .bind(&new_trigger.misfire_policy)
    .bind(new_trigger.max_attempts)
    .bind(new_trigger.max_inflight_runs)
    .bind(new_trigger.jitter_seconds)
    .bind(&new_trigger.input_json)
    .bind(&new_trigger.requested_capabilities)
    .bind(&new_trigger.granted_capabilities)
    .bind(next_fire_at)
    .fetch_one(pool)
    .await?;

    Ok(trigger_from_row(&row))
}

pub async fn get_trigger(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
) -> Result<Option<TriggerRecord>, sqlx::Error> {
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
        WHERE tenant_id = $1
          AND id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(trigger_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| trigger_from_row(&row)))
}

pub async fn update_trigger_status(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    status: &str,
) -> Result<Option<TriggerRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE triggers
        SET status = $3,
            updated_at = now()
        WHERE tenant_id = $1
          AND id = $2
        RETURNING id,
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
        "#,
    )
    .bind(tenant_id)
    .bind(trigger_id)
    .bind(status)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| trigger_from_row(&row)))
}

pub async fn update_trigger_config(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    params: &UpdateTriggerParams,
) -> Result<Option<TriggerRecord>, sqlx::Error> {
    let Some(existing) = get_trigger(pool, tenant_id, trigger_id).await? else {
        return Ok(None);
    };

    let interval_seconds = params.interval_seconds.or(existing.interval_seconds);
    let cron_expression = params
        .cron_expression
        .clone()
        .or(existing.cron_expression.clone());
    let schedule_timezone = params
        .schedule_timezone
        .clone()
        .unwrap_or(existing.schedule_timezone.clone());
    let misfire_policy = params
        .misfire_policy
        .clone()
        .unwrap_or(existing.misfire_policy.clone());
    let max_attempts = params.max_attempts.unwrap_or(existing.max_attempts);
    let max_inflight_runs = params
        .max_inflight_runs
        .unwrap_or(existing.max_inflight_runs);
    let jitter_seconds = params.jitter_seconds.unwrap_or(existing.jitter_seconds);
    let webhook_secret_ref = params
        .webhook_secret_ref
        .clone()
        .or(existing.webhook_secret_ref.clone());

    let next_fire_at = match existing.trigger_type.as_str() {
        "interval" => {
            let interval = interval_seconds.unwrap_or(60);
            OffsetDateTime::now_utc() + time::Duration::seconds(interval)
        }
        "cron" => {
            let expression = cron_expression.clone().ok_or_else(|| {
                sqlx::Error::Protocol("cron trigger requires cron_expression".into())
            })?;
            next_cron_fire_at(&expression, &schedule_timezone, OffsetDateTime::now_utc())
                .map_err(sqlx::Error::Protocol)?
        }
        _ => existing.next_fire_at,
    };
    let next_fire_at = apply_jitter(
        next_fire_at,
        existing.id,
        jitter_seconds,
        OffsetDateTime::now_utc(),
    );

    let row = sqlx::query(
        r#"
        UPDATE triggers
        SET interval_seconds = $3,
            cron_expression = $4,
            schedule_timezone = $5,
            misfire_policy = $6,
            max_attempts = $7,
            max_inflight_runs = $8,
            jitter_seconds = $9,
            webhook_secret_ref = $10,
            next_fire_at = $11,
            updated_at = now()
        WHERE tenant_id = $1
          AND id = $2
        RETURNING id,
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
        "#,
    )
    .bind(tenant_id)
    .bind(trigger_id)
    .bind(interval_seconds)
    .bind(cron_expression)
    .bind(schedule_timezone)
    .bind(misfire_policy)
    .bind(max_attempts)
    .bind(max_inflight_runs)
    .bind(jitter_seconds)
    .bind(webhook_secret_ref)
    .bind(next_fire_at)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| trigger_from_row(&row)))
}

pub async fn enqueue_trigger_event(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    event_id: &str,
    payload_json: Value,
) -> Result<TriggerEventEnqueueOutcome, sqlx::Error> {
    let trigger_row = sqlx::query(
        r#"
        SELECT status, trigger_type, dead_lettered_at
        FROM triggers
        WHERE id = $1
          AND tenant_id = $2
        "#,
    )
    .bind(trigger_id)
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;

    let Some(trigger_row) = trigger_row else {
        return Ok(TriggerEventEnqueueOutcome::TriggerUnavailable {
            reason: TriggerEventEnqueueUnavailableReason::TriggerNotFound,
        });
    };

    let trigger_status: String = trigger_row.get("status");
    if trigger_status != TRIGGER_STATUS_ENABLED {
        return Ok(TriggerEventEnqueueOutcome::TriggerUnavailable {
            reason: TriggerEventEnqueueUnavailableReason::TriggerDisabled,
        });
    }

    let trigger_type: String = trigger_row.get("trigger_type");
    if trigger_type != TRIGGER_TYPE_WEBHOOK {
        return Ok(TriggerEventEnqueueOutcome::TriggerUnavailable {
            reason: TriggerEventEnqueueUnavailableReason::TriggerTypeMismatch,
        });
    }

    let schedule_broken_at: Option<OffsetDateTime> = trigger_row.get("dead_lettered_at");
    if schedule_broken_at.is_some() {
        return Ok(TriggerEventEnqueueOutcome::TriggerUnavailable {
            reason: TriggerEventEnqueueUnavailableReason::TriggerScheduleBroken,
        });
    }

    let semantic_dedupe_key =
        compute_trigger_event_semantic_dedupe_key(tenant_id, &trigger_id, &payload_json);

    let result = sqlx::query(
        r#"
        INSERT INTO trigger_events (
            id,
            trigger_id,
            tenant_id,
            event_id,
            semantic_dedupe_key,
            payload_json,
            status
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(trigger_id)
    .bind(tenant_id)
    .bind(event_id)
    .bind(&semantic_dedupe_key)
    .bind(payload_json)
    .bind(TRIGGER_EVENT_STATUS_PENDING)
    .execute(pool)
    .await?;

    if result.rows_affected() == 1 {
        Ok(TriggerEventEnqueueOutcome::Enqueued)
    } else {
        Ok(TriggerEventEnqueueOutcome::Duplicate)
    }
}

pub fn compute_trigger_event_semantic_dedupe_key(
    tenant_id: &str,
    trigger_id: &Uuid,
    payload_json: &Value,
) -> String {
    let dedupe_payload = json!({
        "tenant_id": tenant_id,
        "trigger_id": trigger_id.to_string(),
        "payload": canonicalize_json_for_trigger_event_semantic_dedupe(payload_json),
    });

    let dedupe_bytes = serde_json::to_vec(&dedupe_payload)
        .expect("failed serializing trigger event semantic dedupe payload");
    format!("{:x}", Sha256::digest(&dedupe_bytes))
}

fn canonicalize_json_for_trigger_event_semantic_dedupe(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut fields: Vec<(String, Value)> = map
                .iter()
                .map(|(key, value)| {
                    (key.clone(), canonicalize_json_for_trigger_event_semantic_dedupe(value))
                })
                .collect();
            fields.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut canonical = Map::new();
            for (key, value) in fields {
                canonical.insert(key, value);
            }

            Value::Object(canonical)
        }
        Value::Array(values) => {
            Value::Array(
                values
                    .iter()
                    .map(canonicalize_json_for_trigger_event_semantic_dedupe)
                    .collect(),
            )
        }
        _ => value.clone(),
    }
}

pub async fn requeue_dead_letter_trigger_event(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    event_id: &str,
) -> Result<TriggerEventReplayOutcome, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let existing_status: Option<String> = sqlx::query_scalar(
        r#"
        SELECT status
        FROM trigger_events
        WHERE trigger_id = $1
          AND tenant_id = $2
          AND event_id = $3
        FOR UPDATE
        "#,
    )
    .bind(trigger_id)
    .bind(tenant_id)
    .bind(event_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(status) = existing_status else {
        tx.commit().await?;
        return Ok(TriggerEventReplayOutcome::NotFound);
    };

    if status != TRIGGER_EVENT_STATUS_DEAD_LETTERED {
        tx.commit().await?;
        return Ok(TriggerEventReplayOutcome::NotDeadLettered { status });
    }

    let result = sqlx::query(
        r#"
        UPDATE trigger_events
        SET status = 'pending',
            attempts = 0,
            next_attempt_at = now(),
            last_error_json = NULL,
            processed_at = NULL,
            dead_lettered_at = NULL
        WHERE trigger_id = $1
          AND tenant_id = $2
          AND event_id = $3
        "#,
    )
    .bind(trigger_id)
    .bind(tenant_id)
    .bind(event_id)
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 1 {
        tx.commit().await?;
        Ok(TriggerEventReplayOutcome::Requeued)
    } else {
        Err(sqlx::Error::Protocol(
            "failed to requeue locked dead-letter trigger event row".into(),
        ))
    }
}

pub async fn fire_trigger_manually(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    idempotency_key: &str,
    payload_json: Option<Value>,
) -> Result<ManualTriggerFireOutcome, sqlx::Error> {
    fire_trigger_manually_with_limits(
        pool,
        tenant_id,
        trigger_id,
        idempotency_key,
        payload_json,
        DEFAULT_TENANT_MAX_INFLIGHT_RUNS,
    )
    .await
}

pub async fn fire_trigger_manually_with_limits(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    idempotency_key: &str,
    payload_json: Option<Value>,
    tenant_max_inflight_runs: i64,
) -> Result<ManualTriggerFireOutcome, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let candidate = sqlx::query(
        r#"
        SELECT id,
               tenant_id,
               agent_id,
               triggered_by_user_id,
               recipe_id,
               input_json,
               requested_capabilities,
               granted_capabilities,
               next_fire_at,
               max_inflight_runs
        FROM triggers
        WHERE id = $1
          AND tenant_id = $2
          AND status = 'enabled'
          AND dead_lettered_at IS NULL
        FOR UPDATE
        "#,
    )
    .bind(trigger_id)
    .bind(tenant_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(candidate) = candidate else {
        tx.commit().await?;
        return Ok(ManualTriggerFireOutcome::TriggerUnavailable);
    };

    let dedupe_key = format!("manual:{idempotency_key}");
    let existing_run_id: Option<Option<Uuid>> = sqlx::query_scalar(
        r#"
        SELECT run_id
        FROM trigger_runs
        WHERE trigger_id = $1
          AND dedupe_key = $2
        LIMIT 1
        "#,
    )
    .bind(trigger_id)
    .bind(&dedupe_key)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(run_id) = existing_run_id {
        tx.commit().await?;
        return Ok(ManualTriggerFireOutcome::Duplicate { run_id });
    }

    let tenant_id: String = candidate.get("tenant_id");
    let agent_id: Uuid = candidate.get("agent_id");
    let triggered_by_user_id: Option<Uuid> = candidate.get("triggered_by_user_id");
    let recipe_id: String = candidate.get("recipe_id");
    let input_json: Value = candidate.get("input_json");
    let requested_capabilities: Value = candidate.get("requested_capabilities");
    let granted_capabilities: Value = candidate.get("granted_capabilities");
    let next_fire_at: OffsetDateTime = candidate.get("next_fire_at");
    let trigger_max_inflight_runs: i32 = candidate.get("max_inflight_runs");

    let trigger_inflight = count_inflight_runs_for_trigger_tx(&mut tx, trigger_id).await?;
    let tenant_inflight = count_inflight_runs_for_tenant_tx(&mut tx, &tenant_id).await?;
    if trigger_inflight >= i64::from(trigger_max_inflight_runs)
        || tenant_inflight >= tenant_max_inflight_runs.max(1)
    {
        tx.commit().await?;
        return Ok(ManualTriggerFireOutcome::InflightLimited);
    }

    let run_id = Uuid::new_v4();
    let run_trace_id = run_id.to_string();
    let scheduled_for = OffsetDateTime::now_utc();
    let trigger_envelope = json!({
        "_trigger": {
            "type": "manual",
            "trigger_id": trigger_id,
            "idempotency_key": idempotency_key,
        },
        "manual_payload": payload_json,
    });
    let run_input = inject_trace_id(
        merge_json_objects(input_json, trigger_envelope),
        &run_trace_id,
    );
    sqlx::query(
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
        VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $8, NULL)
        "#,
    )
    .bind(run_id)
    .bind(&tenant_id)
    .bind(agent_id)
    .bind(triggered_by_user_id)
    .bind(&recipe_id)
    .bind(run_input)
    .bind(requested_capabilities)
    .bind(granted_capabilities)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE triggers
        SET last_fired_at = now(),
            consecutive_failures = 0,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(trigger_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO trigger_runs (
            id,
            trigger_id,
            run_id,
            scheduled_for,
            status,
            dedupe_key,
            error_json
        )
        VALUES ($1, $2, $3, $4, 'created', $5, NULL)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(trigger_id)
    .bind(run_id)
    .bind(scheduled_for)
    .bind(&dedupe_key)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ManualTriggerFireOutcome::Created(TriggerDispatchRecord {
        trigger_id,
        trigger_type: "manual".to_string(),
        trigger_event_id: Some(idempotency_key.to_string()),
        run_id,
        tenant_id,
        agent_id,
        triggered_by_user_id,
        recipe_id,
        scheduled_for,
        next_fire_at,
    }))
}

pub async fn dispatch_next_due_trigger(
    pool: &PgPool,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    dispatch_next_due_trigger_with_limits(pool, DEFAULT_TENANT_MAX_INFLIGHT_RUNS).await
}

pub async fn dispatch_next_due_trigger_with_limits(
    pool: &PgPool,
    tenant_max_inflight_runs: i64,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    if let Some(dispatch) = dispatch_next_due_webhook_event(pool, tenant_max_inflight_runs).await? {
        return Ok(Some(dispatch));
    }
    if let Some(dispatch) = dispatch_next_due_cron_trigger(pool, tenant_max_inflight_runs).await? {
        return Ok(Some(dispatch));
    }
    dispatch_next_due_interval_trigger_with_limits(pool, tenant_max_inflight_runs).await
}

pub async fn dispatch_next_due_interval_trigger(
    pool: &PgPool,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    dispatch_next_due_interval_trigger_with_limits(pool, DEFAULT_TENANT_MAX_INFLIGHT_RUNS).await
}

pub async fn dispatch_next_due_interval_trigger_with_limits(
    pool: &PgPool,
    tenant_max_inflight_runs: i64,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let candidate = sqlx::query(
        r#"
        SELECT t.id,
               t.tenant_id,
               t.agent_id,
               t.triggered_by_user_id,
               t.recipe_id,
               t.input_json,
               t.requested_capabilities,
               t.granted_capabilities,
               t.interval_seconds,
               t.misfire_policy,
               t.jitter_seconds,
               t.next_fire_at AS scheduled_for
        FROM triggers t
        WHERE t.status = 'enabled'
          AND t.trigger_type = 'interval'
          AND t.dead_lettered_at IS NULL
          AND t.next_fire_at <= now()
          AND (
              SELECT COUNT(*)
              FROM trigger_runs tr
              JOIN runs r ON r.id = tr.run_id
              WHERE tr.trigger_id = t.id
                AND r.status IN ('queued', 'running')
          ) < t.max_inflight_runs
          AND (
              SELECT COUNT(*)
              FROM runs r2
              WHERE r2.tenant_id = t.tenant_id
                AND r2.status IN ('queued', 'running')
          ) < $1
        ORDER BY CASE
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'batch'
                   AND now() - t.next_fire_at >= interval '15 minutes' THEN 0
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'interactive' THEN 0
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'batch' THEN 1
                 ELSE 0
                 END,
                 t.next_fire_at ASC,
                 t.created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
    .bind(tenant_max_inflight_runs.max(1))
    .fetch_optional(&mut *tx)
    .await?;

    let Some(candidate) = candidate else {
        tx.commit().await?;
        return Ok(None);
    };
    let trigger_id: Uuid = candidate.get("id");
    let tenant_id: String = candidate.get("tenant_id");
    let agent_id: Uuid = candidate.get("agent_id");
    let triggered_by_user_id: Option<Uuid> = candidate.get("triggered_by_user_id");
    let recipe_id: String = candidate.get("recipe_id");
    let input_json: Value = candidate.get("input_json");
    let requested_capabilities: Value = candidate.get("requested_capabilities");
    let granted_capabilities: Value = candidate.get("granted_capabilities");
    let interval_seconds: i64 = candidate.get("interval_seconds");
    let misfire_policy: String = candidate.get("misfire_policy");
    let jitter_seconds: i32 = candidate.get("jitter_seconds");
    let scheduled_for: OffsetDateTime = candidate.get("scheduled_for");
    let now = OffsetDateTime::now_utc();
    let dedupe_key = scheduled_for.unix_timestamp_nanos().to_string();
    let interval = time::Duration::seconds(interval_seconds);

    if misfire_policy == "skip" && (now - scheduled_for) >= interval {
        let next_fire_at = apply_jitter(now + interval, trigger_id, jitter_seconds, now);
        sqlx::query(
            r#"
            UPDATE triggers
            SET next_fire_at = $2,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(trigger_id)
        .bind(next_fire_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO trigger_runs (
                id,
                trigger_id,
                run_id,
                scheduled_for,
                status,
                dedupe_key,
                error_json
            )
            VALUES ($1, $2, NULL, $3, 'failed', $4, $5)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(trigger_id)
        .bind(scheduled_for)
        .bind(dedupe_key)
        .bind(trigger_error_payload(
            "MISFIRE_SKIPPED",
            "interval trigger misfire skipped",
            TRIGGER_ERROR_CLASS_TRIGGER_POLICY,
        ))
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        return Ok(None);
    }

    let run_id = Uuid::new_v4();
    sqlx::query(
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
        VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $8, NULL)
        "#,
    )
    .bind(run_id)
    .bind(&tenant_id)
    .bind(agent_id)
    .bind(triggered_by_user_id)
    .bind(&recipe_id)
    .bind(input_json)
    .bind(requested_capabilities)
    .bind(granted_capabilities)
    .execute(&mut *tx)
    .await?;

    let next_fire_at = apply_jitter(scheduled_for + interval, trigger_id, jitter_seconds, now);
    sqlx::query(
        r#"
        UPDATE triggers
        SET next_fire_at = $2,
            last_fired_at = now(),
            consecutive_failures = 0,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(trigger_id)
    .bind(next_fire_at)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO trigger_runs (
            id,
            trigger_id,
            run_id,
            scheduled_for,
            status,
            dedupe_key,
            error_json
        )
        VALUES ($1, $2, $3, $4, 'created', $5, NULL)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(trigger_id)
    .bind(run_id)
    .bind(scheduled_for)
    .bind(dedupe_key)
    .execute(&mut *tx)
    .await?;

    let dispatch = TriggerDispatchRecord {
        trigger_id,
        trigger_type: "interval".to_string(),
        trigger_event_id: None,
        run_id,
        tenant_id,
        agent_id,
        triggered_by_user_id,
        recipe_id,
        scheduled_for,
        next_fire_at,
    };

    tx.commit().await?;
    Ok(Some(dispatch))
}

async fn dispatch_next_due_cron_trigger(
    pool: &PgPool,
    tenant_max_inflight_runs: i64,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let candidate = sqlx::query(
        r#"
        SELECT t.id,
               t.tenant_id,
               t.agent_id,
               t.triggered_by_user_id,
               t.recipe_id,
               t.input_json,
               t.requested_capabilities,
               t.granted_capabilities,
               t.cron_expression,
               t.schedule_timezone,
               t.jitter_seconds,
               t.next_fire_at AS scheduled_for
        FROM triggers t
        WHERE t.status = 'enabled'
          AND t.trigger_type = 'cron'
          AND t.dead_lettered_at IS NULL
          AND t.next_fire_at <= now()
          AND (
              SELECT COUNT(*)
              FROM trigger_runs tr
              JOIN runs r ON r.id = tr.run_id
              WHERE tr.trigger_id = t.id
                AND r.status IN ('queued', 'running')
          ) < t.max_inflight_runs
          AND (
              SELECT COUNT(*)
              FROM runs r2
              WHERE r2.tenant_id = t.tenant_id
                AND r2.status IN ('queued', 'running')
          ) < $1
        ORDER BY CASE
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'batch'
                   AND now() - t.next_fire_at >= interval '15 minutes' THEN 0
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'interactive' THEN 0
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'batch' THEN 1
                 ELSE 0
                 END,
                 t.next_fire_at ASC,
                 t.created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
    .bind(tenant_max_inflight_runs.max(1))
    .fetch_optional(&mut *tx)
    .await?;

    let Some(candidate) = candidate else {
        tx.commit().await?;
        return Ok(None);
    };

    let trigger_id: Uuid = candidate.get("id");
    let tenant_id: String = candidate.get("tenant_id");
    let agent_id: Uuid = candidate.get("agent_id");
    let triggered_by_user_id: Option<Uuid> = candidate.get("triggered_by_user_id");
    let recipe_id: String = candidate.get("recipe_id");
    let input_json: Value = candidate.get("input_json");
    let requested_capabilities: Value = candidate.get("requested_capabilities");
    let granted_capabilities: Value = candidate.get("granted_capabilities");
    let cron_expression: String = candidate.get("cron_expression");
    let schedule_timezone: String = candidate.get("schedule_timezone");
    let jitter_seconds: i32 = candidate.get("jitter_seconds");
    let scheduled_for: OffsetDateTime = candidate.get("scheduled_for");
    let dedupe_key = scheduled_for.unix_timestamp_nanos().to_string();

    let next_fire_at = match next_cron_fire_at(&cron_expression, &schedule_timezone, scheduled_for)
    {
        Ok(value) => apply_jitter(value, trigger_id, jitter_seconds, scheduled_for),
        Err(error_message) => {
            sqlx::query(
                r#"
                UPDATE triggers
                SET dead_lettered_at = now(),
                    dead_letter_reason = $2,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(trigger_id)
            .bind(format!("SCHEDULE_COMPUTE_FAILED: {error_message}"))
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO trigger_runs (
                    id,
                    trigger_id,
                    run_id,
                    scheduled_for,
                    status,
                    dedupe_key,
                    error_json
                )
                VALUES ($1, $2, NULL, $3, 'failed', $4, $5)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(trigger_id)
            .bind(scheduled_for)
            .bind(dedupe_key)
            .bind(trigger_error_payload(
                "CRON_COMPUTE_FAILED",
                &error_message,
                TRIGGER_ERROR_CLASS_SCHEDULE,
            ))
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            return Ok(None);
        }
    };

    let run_id = Uuid::new_v4();
    sqlx::query(
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
        VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $8, NULL)
        "#,
    )
    .bind(run_id)
    .bind(&tenant_id)
    .bind(agent_id)
    .bind(triggered_by_user_id)
    .bind(&recipe_id)
    .bind(input_json)
    .bind(requested_capabilities)
    .bind(granted_capabilities)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE triggers
        SET next_fire_at = $2,
            last_fired_at = now(),
            consecutive_failures = 0,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(trigger_id)
    .bind(next_fire_at)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO trigger_runs (
            id,
            trigger_id,
            run_id,
            scheduled_for,
            status,
            dedupe_key,
            error_json
        )
        VALUES ($1, $2, $3, $4, 'created', $5, NULL)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(trigger_id)
    .bind(run_id)
    .bind(scheduled_for)
    .bind(dedupe_key)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(TriggerDispatchRecord {
        trigger_id,
        trigger_type: "cron".to_string(),
        trigger_event_id: None,
        run_id,
        tenant_id,
        agent_id,
        triggered_by_user_id,
        recipe_id,
        scheduled_for,
        next_fire_at,
    }))
}

async fn dispatch_next_due_webhook_event(
    pool: &PgPool,
    tenant_max_inflight_runs: i64,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    const MAX_EVENT_PAYLOAD_BYTES: usize = 64 * 1024;
    let mut tx = pool.begin().await?;
    let candidate = sqlx::query(
        r#"
        SELECT e.id AS trigger_event_row_id,
               e.event_id,
               e.payload_json,
               e.attempts,
               t.id AS trigger_id,
               t.tenant_id,
               t.agent_id,
               t.triggered_by_user_id,
               t.recipe_id,
               t.max_attempts,
               t.input_json,
               t.requested_capabilities,
               t.granted_capabilities
        FROM trigger_events e
        JOIN triggers t ON t.id = e.trigger_id
        WHERE e.status = 'pending'
          AND e.next_attempt_at <= now()
          AND t.status = 'enabled'
          AND t.trigger_type = 'webhook'
          AND t.dead_lettered_at IS NULL
          AND (
              SELECT COUNT(*)
              FROM trigger_runs tr
              JOIN runs r ON r.id = tr.run_id
              WHERE tr.trigger_id = t.id
                AND r.status IN ('queued', 'running')
          ) < t.max_inflight_runs
          AND (
              SELECT COUNT(*)
              FROM runs r2
              WHERE r2.tenant_id = t.tenant_id
                AND r2.status IN ('queued', 'running')
          ) < $1
        ORDER BY CASE
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'batch'
                   AND now() - e.created_at >= interval '15 minutes' THEN 0
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'interactive' THEN 0
                 WHEN lower(
                     COALESCE(
                         t.input_json->>'queue_class',
                         t.input_json->>'llm_queue_class',
                         'interactive'
                     )
                 ) = 'batch' THEN 1
                 ELSE 0
                 END,
                 e.created_at ASC,
                 e.id ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
    .bind(tenant_max_inflight_runs.max(1))
    .fetch_optional(&mut *tx)
    .await?;

    let Some(candidate) = candidate else {
        tx.commit().await?;
        return Ok(None);
    };
    let trigger_event_row_id: Uuid = candidate.get("trigger_event_row_id");
    let trigger_id: Uuid = candidate.get("trigger_id");
    let tenant_id: String = candidate.get("tenant_id");
    let agent_id: Uuid = candidate.get("agent_id");
    let triggered_by_user_id: Option<Uuid> = candidate.get("triggered_by_user_id");
    let recipe_id: String = candidate.get("recipe_id");
    let event_id: String = candidate.get("event_id");
    let payload_json: Value = candidate.get("payload_json");
    let attempts: i32 = candidate.get("attempts");
    let max_attempts: i32 = candidate.get("max_attempts");
    let input_json: Value = candidate.get("input_json");
    let requested_capabilities: Value = candidate.get("requested_capabilities");
    let granted_capabilities: Value = candidate.get("granted_capabilities");
    let scheduled_for = OffsetDateTime::now_utc();
    let event_size = serde_json::to_vec(&payload_json)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);

    if event_size > MAX_EVENT_PAYLOAD_BYTES {
        let next_attempt = attempts + 1;
        let dead_letter = next_attempt >= max_attempts;
        let event_error = trigger_error_payload(
            "EVENT_PAYLOAD_TOO_LARGE",
            "webhook trigger event payload exceeded 64KB",
            TRIGGER_ERROR_CLASS_EVENT_PAYLOAD,
        );
        sqlx::query(
            r#"
            UPDATE trigger_events
            SET attempts = attempts + 1,
                status = CASE WHEN $2 THEN 'dead_lettered' ELSE 'pending' END,
                next_attempt_at = CASE WHEN $2 THEN now() ELSE now() + interval '30 seconds' END,
                last_error_json = $3,
                dead_lettered_at = CASE WHEN $2 THEN now() ELSE NULL END
            WHERE id = $1
            "#,
        )
        .bind(trigger_event_row_id)
        .bind(dead_letter)
        .bind(&event_error)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO trigger_runs (
                id,
                trigger_id,
                run_id,
                scheduled_for,
                status,
                dedupe_key,
                error_json
            )
            VALUES ($1, $2, NULL, $3, 'failed', $4, $5)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(trigger_id)
        .bind(scheduled_for)
        .bind(&event_id)
        .bind(&event_error)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        return Ok(None);
    }

    let run_id = Uuid::new_v4();
    let trigger_envelope = json!({
        "_trigger": {
            "type": "webhook",
            "trigger_id": trigger_id,
            "event_id": event_id,
        },
        "event_payload": payload_json,
    });
    let run_input = inject_trace_id(
        merge_json_objects(input_json, trigger_envelope),
        &run_id.to_string(),
    );
    sqlx::query(
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
        VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $8, NULL)
        "#,
    )
    .bind(run_id)
    .bind(&tenant_id)
    .bind(agent_id)
    .bind(triggered_by_user_id)
    .bind(&recipe_id)
    .bind(run_input)
    .bind(requested_capabilities)
    .bind(granted_capabilities)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE trigger_events
        SET attempts = attempts + 1,
            status = 'processed',
            processed_at = now(),
            next_attempt_at = now()
        WHERE id = $1
        "#,
    )
    .bind(trigger_event_row_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE triggers
        SET last_fired_at = now(),
            consecutive_failures = 0,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(trigger_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO trigger_runs (
            id,
            trigger_id,
            run_id,
            scheduled_for,
            status,
            dedupe_key,
            error_json
        )
        VALUES ($1, $2, $3, $4, 'created', $5, NULL)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(trigger_id)
    .bind(run_id)
    .bind(scheduled_for)
    .bind(&event_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(TriggerDispatchRecord {
        trigger_id,
        trigger_type: "webhook".to_string(),
        trigger_event_id: Some(event_id),
        run_id,
        tenant_id,
        agent_id,
        triggered_by_user_id,
        recipe_id,
        scheduled_for,
        next_fire_at: scheduled_for,
    }))
}

fn trigger_from_row(row: &sqlx::postgres::PgRow) -> TriggerRecord {
    TriggerRecord {
        id: row.get("id"),
        tenant_id: row.get("tenant_id"),
        agent_id: row.get("agent_id"),
        triggered_by_user_id: row.get("triggered_by_user_id"),
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
        dead_lettered_at: row.get("dead_lettered_at"),
        dead_letter_reason: row.get("dead_letter_reason"),
        webhook_secret_ref: row.get("webhook_secret_ref"),
        input_json: row.get("input_json"),
        requested_capabilities: row.get("requested_capabilities"),
        granted_capabilities: row.get("granted_capabilities"),
        next_fire_at: row.get("next_fire_at"),
        last_fired_at: row.get("last_fired_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn merge_json_objects(primary: Value, overlay: Value) -> Value {
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

fn inject_trace_id(input: Value, trace_id: &str) -> Value {
    match input {
        Value::Object(mut map) => {
            map.insert("_trace".to_string(), Value::String(trace_id.to_string()));
            Value::Object(map)
        }
        other => {
            let mut map = serde_json::Map::new();
            map.insert("_trace".to_string(), Value::String(trace_id.to_string()));
            map.insert("input".to_string(), other);
            Value::Object(map)
        }
    }
}

async fn count_inflight_runs_for_trigger_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    trigger_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM trigger_runs tr
        JOIN runs r ON r.id = tr.run_id
        WHERE tr.trigger_id = $1
          AND r.status IN ('queued', 'running')
        "#,
    )
    .bind(trigger_id)
    .fetch_one(&mut **tx)
    .await
}

async fn count_inflight_runs_for_tenant_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM runs
        WHERE tenant_id = $1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(tenant_id)
    .fetch_one(&mut **tx)
    .await
}

fn next_cron_fire_at(
    cron_expression: &str,
    schedule_timezone: &str,
    after: OffsetDateTime,
) -> Result<OffsetDateTime, String> {
    let timezone = Tz::from_str(schedule_timezone)
        .map_err(|err| format!("invalid schedule_timezone `{schedule_timezone}`: {err}"))?;
    let schedule = Schedule::from_str(cron_expression)
        .map_err(|err| format!("invalid cron_expression `{cron_expression}`: {err}"))?;

    let after_utc = DateTime::<Utc>::from_timestamp(after.unix_timestamp(), after.nanosecond())
        .ok_or_else(|| "invalid reference timestamp".to_string())?;
    let after_local = timezone.from_utc_datetime(&after_utc.naive_utc());
    let next_local = schedule
        .after(&after_local)
        .next()
        .ok_or_else(|| "cron schedule has no next fire time".to_string())?;
    let next_utc = next_local.with_timezone(&Utc);

    OffsetDateTime::from_unix_timestamp(next_utc.timestamp())
        .map_err(|err| format!("invalid computed next_fire_at timestamp: {err}"))
}

fn apply_jitter(
    scheduled_for: OffsetDateTime,
    trigger_id: Uuid,
    jitter_seconds: i32,
    entropy_time: OffsetDateTime,
) -> OffsetDateTime {
    if jitter_seconds <= 0 {
        return scheduled_for;
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    trigger_id.hash(&mut hasher);
    entropy_time.unix_timestamp_nanos().hash(&mut hasher);
    let max = u64::try_from(jitter_seconds).unwrap_or(0);
    if max == 0 {
        return scheduled_for;
    }
    let offset = (hasher.finish() % (max + 1)) as i64;
    scheduled_for + time::Duration::seconds(offset)
}

fn clamp_lease_ms(lease_for: Duration) -> i64 {
    lease_for.as_millis().clamp(1, i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::{compute_trigger_event_semantic_dedupe_key, canonicalize_json_for_trigger_event_semantic_dedupe};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn semantic_dedupe_canonicalizes_object_field_order() {
        let trigger_id = Uuid::nil();
        let tenant_id = "tenant-1";
        let canonicalized_a = compute_trigger_event_semantic_dedupe_key(
            tenant_id,
            &trigger_id,
            &json!({"outer":{"z":"v","a":"k"},"inner":1}),
        );
        let canonicalized_b = compute_trigger_event_semantic_dedupe_key(
            tenant_id,
            &trigger_id,
            &json!({"inner":1,"outer":{"a":"k","z":"v"}}),
        );

        assert_eq!(canonicalized_a, canonicalized_b);
    }

    #[test]
    fn semantic_dedupe_distinguishes_array_order() {
        let trigger_id = Uuid::nil();
        let tenant_id = "tenant-1";
        let deduped_a = compute_trigger_event_semantic_dedupe_key(
            tenant_id,
            &trigger_id,
            &json!({"tags":[1, 2, 3]}),
        );
        let deduped_b = compute_trigger_event_semantic_dedupe_key(
            tenant_id,
            &trigger_id,
            &json!({"tags":[3, 2, 1]}),
        );

        assert_ne!(deduped_a, deduped_b);
    }

    #[test]
    fn semantic_dedupe_distinguishes_json_type_changes() {
        let trigger_id = Uuid::nil();
        let tenant_id = "tenant-1";
        let deduped_number = compute_trigger_event_semantic_dedupe_key(
            tenant_id,
            &trigger_id,
            &json!({"status": 1}),
        );
        let deduped_string = compute_trigger_event_semantic_dedupe_key(
            tenant_id,
            &trigger_id,
            &json!({"status":"1"}),
        );

        assert_ne!(deduped_number, deduped_string);
    }

    #[test]
    fn semantic_dedupe_canonicalize_json_is_private_functionally_stable() {
        let canonical = canonicalize_json_for_trigger_event_semantic_dedupe(&json!({
            "b": {"x":1, "y":[{"k":2,"a":1}]},
            "a": [2,1]
        }));
        let expected = json!({
            "a": [2,1],
            "b": {"x":1, "y":[{"k":2,"a":1}]}
        });

        assert_eq!(canonical, expected);
    }
}
