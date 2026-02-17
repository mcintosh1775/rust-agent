use serde_json::{json, Value};
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
    pub webhook_secret_ref: Option<String>,
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
    pub misfire_policy: String,
    pub max_attempts: i32,
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
            misfire_policy,
            max_attempts,
            webhook_secret_ref,
            input_json,
            requested_capabilities,
            granted_capabilities,
            next_fire_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'interval', $7, $8, $9, $10, $11, $12, $13)
        RETURNING id,
                  tenant_id,
                  agent_id,
                  triggered_by_user_id,
                  recipe_id,
                  status,
                  trigger_type,
                  interval_seconds,
                  misfire_policy,
                  max_attempts,
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
            misfire_policy,
            max_attempts,
            webhook_secret_ref,
            input_json,
            requested_capabilities,
            granted_capabilities,
            next_fire_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'webhook', NULL, 'fire_now', $7, $8, $9, $10, $11, now())
        RETURNING id,
                  tenant_id,
                  agent_id,
                  triggered_by_user_id,
                  recipe_id,
                  status,
                  trigger_type,
                  interval_seconds,
                  misfire_policy,
                  max_attempts,
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
    .bind(&new_trigger.webhook_secret_ref)
    .bind(&new_trigger.input_json)
    .bind(&new_trigger.requested_capabilities)
    .bind(&new_trigger.granted_capabilities)
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
               misfire_policy,
               max_attempts,
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

pub async fn enqueue_trigger_event(
    pool: &PgPool,
    tenant_id: &str,
    trigger_id: Uuid,
    event_id: &str,
    payload_json: Value,
) -> Result<TriggerEventEnqueueOutcome, sqlx::Error> {
    let trigger_exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM triggers
            WHERE id = $1
              AND tenant_id = $2
              AND status = 'enabled'
              AND trigger_type = 'webhook'
              AND dead_lettered_at IS NULL
        )
        "#,
    )
    .bind(trigger_id)
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    if !trigger_exists {
        return Ok(TriggerEventEnqueueOutcome::Duplicate);
    }

    let result = sqlx::query(
        r#"
        INSERT INTO trigger_events (
            id,
            trigger_id,
            tenant_id,
            event_id,
            payload_json,
            status
        )
        VALUES ($1, $2, $3, $4, $5, 'pending')
        ON CONFLICT (trigger_id, event_id) DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(trigger_id)
    .bind(tenant_id)
    .bind(event_id)
    .bind(payload_json)
    .execute(pool)
    .await?;

    if result.rows_affected() == 1 {
        Ok(TriggerEventEnqueueOutcome::Enqueued)
    } else {
        Ok(TriggerEventEnqueueOutcome::Duplicate)
    }
}

pub async fn dispatch_next_due_trigger(
    pool: &PgPool,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    if let Some(dispatch) = dispatch_next_due_webhook_event(pool).await? {
        return Ok(Some(dispatch));
    }
    dispatch_next_due_interval_trigger(pool).await
}

pub async fn dispatch_next_due_interval_trigger(
    pool: &PgPool,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
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
               interval_seconds,
               misfire_policy,
               next_fire_at AS scheduled_for
        FROM triggers
        WHERE status = 'enabled'
          AND trigger_type = 'interval'
          AND dead_lettered_at IS NULL
          AND next_fire_at <= now()
        ORDER BY next_fire_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
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
    let scheduled_for: OffsetDateTime = candidate.get("scheduled_for");
    let now = OffsetDateTime::now_utc();
    let dedupe_key = scheduled_for.unix_timestamp_nanos().to_string();
    let interval = time::Duration::seconds(interval_seconds);

    if misfire_policy == "skip" && (now - scheduled_for) >= interval {
        let next_fire_at = now + interval;
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
        .bind(json!({"code":"MISFIRE_SKIPPED","message":"interval trigger misfire skipped"}))
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

    let next_fire_at = scheduled_for + interval;
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

async fn dispatch_next_due_webhook_event(
    pool: &PgPool,
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
        ORDER BY e.created_at ASC
        FOR UPDATE SKIP LOCKED
        LIMIT 1
        "#,
    )
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
        .bind(json!({"code":"EVENT_PAYLOAD_TOO_LARGE","message":"webhook trigger event payload exceeded 64KB"}))
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
        .bind(json!({"code":"EVENT_PAYLOAD_TOO_LARGE"}))
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
    let run_input = merge_json_objects(input_json, trigger_envelope);
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
        misfire_policy: row.get("misfire_policy"),
        max_attempts: row.get("max_attempts"),
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

fn clamp_lease_ms(lease_for: Duration) -> i64 {
    lease_for.as_millis().clamp(1, i64::MAX as u128) as i64
}
