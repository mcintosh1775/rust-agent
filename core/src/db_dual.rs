use crate::db::{
    append_audit_event, count_tenant_inflight_runs, create_run, create_step, get_run_status,
    get_tenant_ops_summary, list_run_audit_events, mark_run_failed, mark_run_succeeded,
    mark_step_failed, mark_step_succeeded, AuditEventDetailRecord, AuditEventRecord, NewAuditEvent,
    create_run_with_semantic_dedupe_key, get_active_run_id_by_semantic_dedupe_key, NewRun, NewStep,
    RunRecord, RunStatusRecord, StepRecord, TenantOpsSummaryRecord,
};
use crate::db_pool::DbPool;
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

pub async fn create_run_dual(pool: &DbPool, new_run: &NewRun) -> Result<RunRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => create_run(pg, new_run).await,
        DbPool::Sqlite(sqlite) => create_run_sqlite(sqlite, new_run).await,
    }
}

pub async fn create_run_with_semantic_dedupe_key_dual(
    pool: &DbPool,
    new_run: &NewRun,
    semantic_dedupe_key: &str,
) -> Result<Option<RunRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            create_run_with_semantic_dedupe_key(pg, new_run, semantic_dedupe_key).await
        }
        DbPool::Sqlite(sqlite) => {
            create_run_with_semantic_dedupe_key_sqlite(sqlite, new_run, semantic_dedupe_key).await
        }
    }
}

pub async fn get_active_run_id_by_semantic_dedupe_key_dual(
    pool: &DbPool,
    tenant_id: &str,
    semantic_dedupe_key: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            get_active_run_id_by_semantic_dedupe_key(pg, tenant_id, semantic_dedupe_key).await
        }
        DbPool::Sqlite(sqlite) => {
            get_active_run_id_by_semantic_dedupe_key_sqlite(sqlite, tenant_id, semantic_dedupe_key)
                .await
        }
    }
}

pub async fn count_tenant_inflight_runs_dual(
    pool: &DbPool,
    tenant_id: &str,
) -> Result<i64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => count_tenant_inflight_runs(pg, tenant_id).await,
        DbPool::Sqlite(sqlite) => count_tenant_inflight_runs_sqlite(sqlite, tenant_id).await,
    }
}

pub async fn get_run_status_dual(
    pool: &DbPool,
    tenant_id: &str,
    run_id: Uuid,
) -> Result<Option<RunStatusRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => get_run_status(pg, tenant_id, run_id).await,
        DbPool::Sqlite(sqlite) => get_run_status_sqlite(sqlite, tenant_id, run_id).await,
    }
}

pub async fn create_step_dual(
    pool: &DbPool,
    new_step: &NewStep,
) -> Result<StepRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => create_step(pg, new_step).await,
        DbPool::Sqlite(sqlite) => create_step_sqlite(sqlite, new_step).await,
    }
}

pub async fn mark_step_succeeded_dual(
    pool: &DbPool,
    step_id: Uuid,
    output_json: Value,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => mark_step_succeeded(pg, step_id, output_json).await,
        DbPool::Sqlite(sqlite) => mark_step_succeeded_sqlite(sqlite, step_id, output_json).await,
    }
}

pub async fn mark_step_failed_dual(
    pool: &DbPool,
    step_id: Uuid,
    error_json: Value,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => mark_step_failed(pg, step_id, error_json).await,
        DbPool::Sqlite(sqlite) => mark_step_failed_sqlite(sqlite, step_id, error_json).await,
    }
}

pub async fn mark_run_succeeded_dual(
    pool: &DbPool,
    run_id: Uuid,
    worker_id: &str,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => mark_run_succeeded(pg, run_id, worker_id).await,
        DbPool::Sqlite(sqlite) => mark_run_succeeded_sqlite(sqlite, run_id, worker_id).await,
    }
}

pub async fn mark_run_failed_dual(
    pool: &DbPool,
    run_id: Uuid,
    worker_id: &str,
    error_json: Value,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => mark_run_failed(pg, run_id, worker_id, error_json).await,
        DbPool::Sqlite(sqlite) => {
            mark_run_failed_sqlite(sqlite, run_id, worker_id, error_json).await
        }
    }
}

pub async fn append_audit_event_dual(
    pool: &DbPool,
    new_event: &NewAuditEvent,
) -> Result<AuditEventRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => append_audit_event(pg, new_event).await,
        DbPool::Sqlite(sqlite) => append_audit_event_sqlite(sqlite, new_event).await,
    }
}

pub async fn list_run_audit_events_dual(
    pool: &DbPool,
    tenant_id: &str,
    run_id: Uuid,
    limit: i64,
) -> Result<Vec<AuditEventDetailRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => list_run_audit_events(pg, tenant_id, run_id, limit).await,
        DbPool::Sqlite(sqlite) => {
            list_run_audit_events_sqlite(sqlite, tenant_id, run_id, limit).await
        }
    }
}

pub async fn get_tenant_ops_summary_dual(
    pool: &DbPool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<TenantOpsSummaryRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => get_tenant_ops_summary(pg, tenant_id, since).await,
        DbPool::Sqlite(sqlite) => get_tenant_ops_summary_sqlite(sqlite, tenant_id, since).await,
    }
}

async fn create_run_sqlite(pool: &SqlitePool, new_run: &NewRun) -> Result<RunRecord, sqlx::Error> {
    let now = OffsetDateTime::now_utc();
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
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        RETURNING id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, created_at
        "#,
    )
    .bind(new_run.id.to_string())
    .bind(&new_run.tenant_id)
    .bind(new_run.agent_id.to_string())
    .bind(new_run.triggered_by_user_id.map(|id| id.to_string()))
    .bind(&new_run.recipe_id)
    .bind(&new_run.status)
    .bind(new_run.input_json.to_string())
    .bind(new_run.requested_capabilities.to_string())
    .bind(new_run.granted_capabilities.to_string())
    .bind(new_run.error_json.as_ref().map(Value::to_string))
    .bind(now.format(&Rfc3339).map_err(sqlite_protocol_error)?)
    .fetch_one(pool)
    .await?;

    Ok(RunRecord {
        id: parse_uuid_required(&row, "id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_uuid_required(&row, "agent_id")?,
        triggered_by_user_id: parse_uuid_optional(&row, "triggered_by_user_id")?,
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        created_at: parse_datetime_required(&row, "created_at")?,
    })
}

async fn create_run_with_semantic_dedupe_key_sqlite(
    pool: &SqlitePool,
    new_run: &NewRun,
    semantic_dedupe_key: &str,
) -> Result<Option<RunRecord>, sqlx::Error> {
    let now = OffsetDateTime::now_utc();
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
            semantic_dedupe_key,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT (tenant_id, semantic_dedupe_key)
            WHERE status IN ('queued', 'running')
            DO NOTHING
        RETURNING id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, created_at
        "#,
    )
    .bind(new_run.id.to_string())
    .bind(&new_run.tenant_id)
    .bind(new_run.agent_id.to_string())
    .bind(new_run.triggered_by_user_id.map(|id| id.to_string()))
    .bind(&new_run.recipe_id)
    .bind(&new_run.status)
    .bind(new_run.input_json.to_string())
    .bind(new_run.requested_capabilities.to_string())
    .bind(new_run.granted_capabilities.to_string())
    .bind(new_run.error_json.as_ref().map(Value::to_string))
    .bind(semantic_dedupe_key)
    .bind(now.format(&Rfc3339).map_err(sqlite_protocol_error)?)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(RunRecord {
        id: parse_uuid_required(&row, "id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_uuid_required(&row, "agent_id")?,
        triggered_by_user_id: parse_uuid_optional(&row, "triggered_by_user_id")?,
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        created_at: parse_datetime_required(&row, "created_at")?,
    }))
}

async fn get_active_run_id_by_semantic_dedupe_key_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    semantic_dedupe_key: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let run_id: Option<String> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM runs
        WHERE tenant_id = ?1
          AND semantic_dedupe_key = ?2
          AND status IN ('queued', 'running')
        ORDER BY created_at DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(tenant_id)
    .bind(semantic_dedupe_key)
    .fetch_optional(pool)
    .await?;

    let Some(run_id) = run_id else {
        return Ok(None);
    };

    let parsed = Uuid::parse_str(&run_id).map_err(|err| sqlx::Error::Protocol(err.into()))?;
    Ok(Some(parsed))
}

async fn get_run_status_sqlite(
    pool: &SqlitePool,
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
        WHERE tenant_id = ?1
          AND id = ?2
        "#,
    )
    .bind(tenant_id)
    .bind(run_id.to_string())
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    Ok(Some(RunStatusRecord {
        id: parse_uuid_required(&row, "id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_uuid_required(&row, "agent_id")?,
        triggered_by_user_id: parse_uuid_optional(&row, "triggered_by_user_id")?,
        recipe_id: row.get("recipe_id"),
        status: row.get("status"),
        requested_capabilities: parse_json_required(&row, "requested_capabilities")?,
        granted_capabilities: parse_json_required(&row, "granted_capabilities")?,
        created_at: parse_datetime_required(&row, "created_at")?,
        started_at: parse_datetime_optional(&row, "started_at")?,
        finished_at: parse_datetime_optional(&row, "finished_at")?,
        error_json: parse_json_optional(&row, "error_json")?,
        attempts: row.get("attempts"),
        lease_owner: row.get("lease_owner"),
        lease_expires_at: parse_datetime_optional(&row, "lease_expires_at")?,
    }))
}

async fn create_step_sqlite(
    pool: &SqlitePool,
    new_step: &NewStep,
) -> Result<StepRecord, sqlx::Error> {
    let now = OffsetDateTime::now_utc();
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
            error_json,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        RETURNING id, run_id, tenant_id, agent_id, user_id, name, status, created_at
        "#,
    )
    .bind(new_step.id.to_string())
    .bind(new_step.run_id.to_string())
    .bind(&new_step.tenant_id)
    .bind(new_step.agent_id.to_string())
    .bind(new_step.user_id.map(|id| id.to_string()))
    .bind(&new_step.name)
    .bind(&new_step.status)
    .bind(new_step.input_json.to_string())
    .bind(new_step.error_json.as_ref().map(Value::to_string))
    .bind(now.format(&Rfc3339).map_err(sqlite_protocol_error)?)
    .fetch_one(pool)
    .await?;

    Ok(StepRecord {
        id: parse_uuid_required(&row, "id")?,
        run_id: parse_uuid_required(&row, "run_id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_uuid_required(&row, "agent_id")?,
        user_id: parse_uuid_optional(&row, "user_id")?,
        name: row.get("name"),
        status: row.get("status"),
        created_at: parse_datetime_required(&row, "created_at")?,
    })
}

async fn mark_step_succeeded_sqlite(
    pool: &SqlitePool,
    step_id: Uuid,
    output_json: Value,
) -> Result<bool, sqlx::Error> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
    let result = sqlx::query(
        r#"
        UPDATE steps
        SET status = 'succeeded',
            output_json = ?2,
            finished_at = ?3
        WHERE id = ?1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(step_id.to_string())
    .bind(output_json.to_string())
    .bind(now)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

async fn mark_step_failed_sqlite(
    pool: &SqlitePool,
    step_id: Uuid,
    error_json: Value,
) -> Result<bool, sqlx::Error> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
    let result = sqlx::query(
        r#"
        UPDATE steps
        SET status = 'failed',
            error_json = ?2,
            finished_at = ?3
        WHERE id = ?1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(step_id.to_string())
    .bind(error_json.to_string())
    .bind(now)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

async fn mark_run_succeeded_sqlite(
    pool: &SqlitePool,
    run_id: Uuid,
    worker_id: &str,
) -> Result<bool, sqlx::Error> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
    let result = sqlx::query(
        r#"
        UPDATE runs
        SET status = 'succeeded',
            finished_at = ?2,
            lease_owner = NULL,
            lease_expires_at = NULL
        WHERE id = ?1
          AND status = 'running'
          AND (lease_owner = ?3 OR lease_owner IS NULL)
        "#,
    )
    .bind(run_id.to_string())
    .bind(now)
    .bind(worker_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

async fn mark_run_failed_sqlite(
    pool: &SqlitePool,
    run_id: Uuid,
    worker_id: &str,
    error_json: Value,
) -> Result<bool, sqlx::Error> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
    let result = sqlx::query(
        r#"
        UPDATE runs
        SET status = 'failed',
            finished_at = ?2,
            error_json = ?3,
            lease_owner = NULL,
            lease_expires_at = NULL
        WHERE id = ?1
          AND status = 'running'
          AND (lease_owner = ?4 OR lease_owner IS NULL)
        "#,
    )
    .bind(run_id.to_string())
    .bind(now)
    .bind(error_json.to_string())
    .bind(worker_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

async fn append_audit_event_sqlite(
    pool: &SqlitePool,
    new_event: &NewAuditEvent,
) -> Result<AuditEventRecord, sqlx::Error> {
    let now = OffsetDateTime::now_utc();
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
            payload_json,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        RETURNING id, run_id, step_id, actor, event_type, created_at
        "#,
    )
    .bind(new_event.id.to_string())
    .bind(new_event.run_id.to_string())
    .bind(new_event.step_id.map(|id| id.to_string()))
    .bind(&new_event.tenant_id)
    .bind(new_event.agent_id.map(|id| id.to_string()))
    .bind(new_event.user_id.map(|id| id.to_string()))
    .bind(&new_event.actor)
    .bind(&new_event.event_type)
    .bind(new_event.payload_json.to_string())
    .bind(now.format(&Rfc3339).map_err(sqlite_protocol_error)?)
    .fetch_one(pool)
    .await?;

    Ok(AuditEventRecord {
        id: parse_uuid_required(&row, "id")?,
        run_id: parse_uuid_required(&row, "run_id")?,
        step_id: parse_uuid_optional(&row, "step_id")?,
        actor: row.get("actor"),
        event_type: row.get("event_type"),
        created_at: parse_datetime_required(&row, "created_at")?,
    })
}

async fn list_run_audit_events_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    run_id: Uuid,
    limit: i64,
) -> Result<Vec<AuditEventDetailRecord>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT id, run_id, step_id, actor, event_type, payload_json, created_at
        FROM audit_events
        WHERE tenant_id = ?1
          AND run_id = ?2
        ORDER BY datetime(created_at) ASC, id ASC
        LIMIT ?3
        "#,
    )
    .bind(tenant_id)
    .bind(run_id.to_string())
    .bind(limit.clamp(1, 1000))
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(AuditEventDetailRecord {
                id: parse_uuid_required(&row, "id")?,
                run_id: parse_uuid_required(&row, "run_id")?,
                step_id: parse_uuid_optional(&row, "step_id")?,
                actor: row.get("actor"),
                event_type: row.get("event_type"),
                payload_json: parse_json_required(&row, "payload_json")?,
                created_at: parse_datetime_required(&row, "created_at")?,
            })
        })
        .collect()
}

async fn get_tenant_ops_summary_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<TenantOpsSummaryRecord, sqlx::Error> {
    let since_text = since.format(&Rfc3339).map_err(sqlite_protocol_error)?;

    let queued_runs: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM runs
        WHERE tenant_id = ?1
          AND status = 'queued'
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    let running_runs: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM runs
        WHERE tenant_id = ?1
          AND status = 'running'
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    let succeeded_runs_window: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM runs
        WHERE tenant_id = ?1
          AND status = 'succeeded'
          AND datetime(finished_at) >= datetime(?2)
        "#,
    )
    .bind(tenant_id)
    .bind(&since_text)
    .fetch_one(pool)
    .await?;
    let failed_runs_window: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM runs
        WHERE tenant_id = ?1
          AND status = 'failed'
          AND datetime(finished_at) >= datetime(?2)
        "#,
    )
    .bind(tenant_id)
    .bind(&since_text)
    .fetch_one(pool)
    .await?;
    let dead_letter_trigger_events_window: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM trigger_events
        WHERE tenant_id = ?1
          AND status = 'dead_lettered'
          AND datetime(created_at) >= datetime(?2)
        "#,
    )
    .bind(tenant_id)
    .bind(&since_text)
    .fetch_one(pool)
    .await?;

    let duration_rows = sqlx::query(
        r#"
        SELECT (julianday(finished_at) - julianday(started_at)) * 86400000.0 AS duration_ms
        FROM runs
        WHERE tenant_id = ?1
          AND finished_at IS NOT NULL
          AND started_at IS NOT NULL
          AND datetime(finished_at) >= datetime(?2)
        "#,
    )
    .bind(tenant_id)
    .bind(&since_text)
    .fetch_all(pool)
    .await?;

    let mut durations: Vec<f64> = duration_rows
        .into_iter()
        .filter_map(|row| row.get::<Option<f64>, _>("duration_ms"))
        .map(|duration| duration.max(0.0))
        .collect();

    let avg_run_duration_ms = if durations.is_empty() {
        None
    } else {
        Some(durations.iter().sum::<f64>() / durations.len() as f64)
    };
    let p95_run_duration_ms = if durations.is_empty() {
        None
    } else {
        durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let rank = ((durations.len() as f64) * 0.95).ceil() as usize;
        let index = rank.saturating_sub(1).min(durations.len() - 1);
        Some(durations[index])
    };

    Ok(TenantOpsSummaryRecord {
        queued_runs,
        running_runs,
        succeeded_runs_window,
        failed_runs_window,
        dead_letter_trigger_events_window,
        avg_run_duration_ms,
        p95_run_duration_ms,
    })
}

async fn count_tenant_inflight_runs_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM runs
        WHERE tenant_id = ?1
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await
}

fn parse_uuid_required(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Uuid, sqlx::Error> {
    let raw: String = row.get(column);
    Uuid::parse_str(raw.as_str()).map_err(|err| {
        sqlx::Error::Protocol(
            format!("invalid uuid in column `{column}`: {err} (value={raw})").into(),
        )
    })
}

fn parse_uuid_optional(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let raw: Option<String> = row.get(column);
    raw.map(|value| {
        Uuid::parse_str(value.as_str()).map_err(|err| {
            sqlx::Error::Protocol(
                format!("invalid uuid in column `{column}`: {err} (value={value})").into(),
            )
        })
    })
    .transpose()
}

fn parse_json_required(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Value, sqlx::Error> {
    let raw: String = row.get(column);
    serde_json::from_str(raw.as_str()).map_err(|err| {
        sqlx::Error::Protocol(
            format!("invalid json in column `{column}`: {err} (value={raw})").into(),
        )
    })
}

fn parse_json_optional(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<Value>, sqlx::Error> {
    let raw: Option<String> = row.get(column);
    raw.map(|value| {
        serde_json::from_str(value.as_str()).map_err(|err| {
            sqlx::Error::Protocol(
                format!("invalid json in column `{column}`: {err} (value={value})").into(),
            )
        })
    })
    .transpose()
}

fn parse_datetime_required(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<OffsetDateTime, sqlx::Error> {
    let raw: String = row.get(column);
    parse_datetime_str(raw.as_str()).map_err(|err| {
        sqlx::Error::Protocol(
            format!("invalid datetime in column `{column}`: {err} (value={raw})").into(),
        )
    })
}

fn parse_datetime_optional(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<OffsetDateTime>, sqlx::Error> {
    let raw: Option<String> = row.get(column);
    raw.map(|value| {
        parse_datetime_str(value.as_str()).map_err(|err| {
            sqlx::Error::Protocol(
                format!("invalid datetime in column `{column}`: {err} (value={value})").into(),
            )
        })
    })
    .transpose()
}

fn parse_datetime_str(raw: &str) -> Result<OffsetDateTime, time::error::Parse> {
    if let Ok(parsed) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Ok(parsed);
    }

    const SQLITE_FORMAT: &[time::format_description::FormatItem<'_>] =
        time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let primitive = PrimitiveDateTime::parse(raw, SQLITE_FORMAT)?;
    Ok(primitive.assume_utc())
}

fn sqlite_protocol_error(error: time::error::Format) -> sqlx::Error {
    sqlx::Error::Protocol(format!("sqlite datetime format error: {error}").into())
}
