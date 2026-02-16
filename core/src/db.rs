use serde_json::Value;
use sqlx::{PgPool, Row};
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
