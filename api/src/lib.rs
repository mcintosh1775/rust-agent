use agent_core::{
    append_audit_event, create_run, get_run_status, list_run_audit_events, NewAuditEvent, NewRun,
};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use core as agent_core;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

const TENANT_HEADER: &str = "x-tenant-id";

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

pub fn app_router(pool: PgPool) -> Router {
    Router::new()
        .route("/v1/runs", post(create_run_handler))
        .route("/v1/runs/:id", get(get_run_handler))
        .route("/v1/runs/:id/audit", get(get_run_audit_handler))
        .with_state(AppState { pool })
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
    let run_id = Uuid::new_v4();

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
            requested_capabilities: req.requested_capabilities,
            // API defaults to no granted capabilities until policy wiring is implemented.
            granted_capabilities: json!([]),
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
            payload_json: json!({"recipe_id": created.recipe_id}),
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

fn default_json_array() -> Value {
    json!([])
}
