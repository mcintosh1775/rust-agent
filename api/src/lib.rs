use agent_core::{
    append_audit_event, append_trigger_audit_event, count_tenant_inflight_runs,
    count_tenant_triggers, create_cron_trigger, create_interval_trigger, create_run,
    create_webhook_trigger, enqueue_trigger_event, fire_trigger_manually,
    get_llm_usage_totals_since, get_run_status, get_tenant_compliance_audit_policy,
    get_tenant_ops_summary, get_tenant_payment_summary, get_trigger, list_run_audit_events,
    list_tenant_compliance_audit_events, list_tenant_payment_ledger,
    purge_expired_tenant_compliance_audit_events, requeue_dead_letter_trigger_event,
    resolve_secret_value, update_trigger_config, update_trigger_status,
    upsert_tenant_compliance_audit_policy, verify_tenant_compliance_audit_chain,
    CachedSecretResolver, CliSecretResolver, ManualTriggerFireOutcome, NewAuditEvent,
    NewCronTrigger, NewIntervalTrigger, NewRun, NewTriggerAuditEvent, NewWebhookTrigger,
    TriggerEventEnqueueOutcome, TriggerEventReplayOutcome, UpdateTriggerParams,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use core as agent_core;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
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

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub tenant_max_inflight_runs: Option<i64>,
    pub tenant_max_triggers: Option<i64>,
}

pub fn app_router(pool: PgPool) -> Router {
    let tenant_max_inflight_runs = parse_positive_i64_env("API_TENANT_MAX_INFLIGHT_RUNS");
    let tenant_max_triggers = parse_positive_i64_env("API_TENANT_MAX_TRIGGERS");
    app_router_with_limits(pool, tenant_max_inflight_runs, tenant_max_triggers)
}

pub fn app_router_with_tenant_limit(pool: PgPool, tenant_max_inflight_runs: Option<i64>) -> Router {
    app_router_with_limits(pool, tenant_max_inflight_runs, None)
}

pub fn app_router_with_limits(
    pool: PgPool,
    tenant_max_inflight_runs: Option<i64>,
    tenant_max_triggers: Option<i64>,
) -> Router {
    Router::new()
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
        .route("/v1/audit/compliance", get(get_compliance_audit_handler))
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
        .route("/v1/payments/summary", get(get_payment_summary_handler))
        .route("/v1/payments", get(get_payments_handler))
        .route("/v1/usage/llm/tokens", get(get_llm_usage_tokens_handler))
        .route("/v1/ops/summary", get(get_ops_summary_handler))
        .with_state(AppState {
            pool,
            tenant_max_inflight_runs,
            tenant_max_triggers,
        })
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
struct OpsSummaryQuery {
    window_secs: Option<u64>,
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

    let mut ndjson = String::new();
    for event in events {
        let line = serde_json::to_string(&json!({
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
        }))
        .map_err(|err| {
            ApiError::internal(format!("failed serializing compliance export row: {err}"))
        })?;
        ndjson.push_str(&line);
        ndjson.push('\n');
    }

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        ndjson,
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
        "message.send" | "message_send" => Some("message.send"),
        "payment.send" | "payment_send" => Some("payment.send"),
        "llm.infer" | "llm_infer" => Some("llm.infer"),
        "local.exec" | "local_exec" => Some("local.exec"),
        "db.query" | "db_query" => Some("db.query"),
        "http.request" | "http_request" => Some("http.request"),
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
        "message.send" => scope.starts_with("whitenoise:") || scope.starts_with("slack:"),
        "payment.send" => scope.starts_with("nwc:"),
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
