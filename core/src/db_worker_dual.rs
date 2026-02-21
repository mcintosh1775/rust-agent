use crate::db::{
    claim_next_queued_run, create_action_request, create_action_result,
    create_llm_token_usage_record, create_or_get_payment_request, create_payment_result,
    get_latest_payment_result, persist_artifact_metadata, renew_run_lease, requeue_expired_runs,
    sum_executed_payment_amount_msat_for_agent, sum_executed_payment_amount_msat_for_tenant,
    sum_llm_consumed_tokens_for_agent_since, sum_llm_consumed_tokens_for_model_since,
    sum_llm_consumed_tokens_for_tenant_since, update_action_request_status,
    update_payment_request_status, ActionRequestRecord, ActionResultRecord, ArtifactRecord,
    LlmTokenUsageRecord, NewActionRequest, NewActionResult, NewArtifact, NewLlmTokenUsageRecord,
    NewPaymentRequest, NewPaymentResult, PaymentRequestRecord, PaymentResultRecord, RunLeaseRecord,
};
use crate::db_pool::DbPool;
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

pub async fn create_action_request_dual(
    pool: &DbPool,
    new_request: &NewActionRequest,
) -> Result<ActionRequestRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => create_action_request(pg, new_request).await,
        DbPool::Sqlite(sqlite) => create_action_request_sqlite(sqlite, new_request).await,
    }
}

pub async fn update_action_request_status_dual(
    pool: &DbPool,
    action_request_id: Uuid,
    status: &str,
    decision_reason: Option<&str>,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            update_action_request_status(pg, action_request_id, status, decision_reason).await
        }
        DbPool::Sqlite(sqlite) => {
            update_action_request_status_sqlite(sqlite, action_request_id, status, decision_reason)
                .await
        }
    }
}

pub async fn create_action_result_dual(
    pool: &DbPool,
    new_result: &NewActionResult,
) -> Result<ActionResultRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => create_action_result(pg, new_result).await,
        DbPool::Sqlite(sqlite) => create_action_result_sqlite(sqlite, new_result).await,
    }
}

pub async fn create_or_get_payment_request_dual(
    pool: &DbPool,
    new_request: &NewPaymentRequest,
) -> Result<PaymentRequestRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => create_or_get_payment_request(pg, new_request).await,
        DbPool::Sqlite(sqlite) => create_or_get_payment_request_sqlite(sqlite, new_request).await,
    }
}

pub async fn create_payment_result_dual(
    pool: &DbPool,
    new_result: &NewPaymentResult,
) -> Result<PaymentResultRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => create_payment_result(pg, new_result).await,
        DbPool::Sqlite(sqlite) => create_payment_result_sqlite(sqlite, new_result).await,
    }
}

pub async fn get_latest_payment_result_dual(
    pool: &DbPool,
    payment_request_id: Uuid,
) -> Result<Option<PaymentResultRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => get_latest_payment_result(pg, payment_request_id).await,
        DbPool::Sqlite(sqlite) => {
            get_latest_payment_result_sqlite(sqlite, payment_request_id).await
        }
    }
}

pub async fn sum_executed_payment_amount_msat_for_tenant_dual(
    pool: &DbPool,
    tenant_id: &str,
) -> Result<i64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => sum_executed_payment_amount_msat_for_tenant(pg, tenant_id).await,
        DbPool::Sqlite(sqlite) => {
            sum_executed_payment_amount_msat_for_tenant_sqlite(sqlite, tenant_id).await
        }
    }
}

pub async fn sum_executed_payment_amount_msat_for_agent_dual(
    pool: &DbPool,
    tenant_id: &str,
    agent_id: Uuid,
) -> Result<i64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            sum_executed_payment_amount_msat_for_agent(pg, tenant_id, agent_id).await
        }
        DbPool::Sqlite(sqlite) => {
            sum_executed_payment_amount_msat_for_agent_sqlite(sqlite, tenant_id, agent_id).await
        }
    }
}

pub async fn update_payment_request_status_dual(
    pool: &DbPool,
    payment_request_id: Uuid,
    status: &str,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => update_payment_request_status(pg, payment_request_id, status).await,
        DbPool::Sqlite(sqlite) => {
            update_payment_request_status_sqlite(sqlite, payment_request_id, status).await
        }
    }
}

pub async fn create_llm_token_usage_record_dual(
    pool: &DbPool,
    new_record: &NewLlmTokenUsageRecord,
) -> Result<LlmTokenUsageRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => create_llm_token_usage_record(pg, new_record).await,
        DbPool::Sqlite(sqlite) => create_llm_token_usage_record_sqlite(sqlite, new_record).await,
    }
}

pub async fn sum_llm_consumed_tokens_for_tenant_since_dual(
    pool: &DbPool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            sum_llm_consumed_tokens_for_tenant_since(pg, tenant_id, since).await
        }
        DbPool::Sqlite(sqlite) => {
            sum_llm_consumed_tokens_for_tenant_since_sqlite(sqlite, tenant_id, since).await
        }
    }
}

pub async fn sum_llm_consumed_tokens_for_agent_since_dual(
    pool: &DbPool,
    tenant_id: &str,
    agent_id: Uuid,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            sum_llm_consumed_tokens_for_agent_since(pg, tenant_id, agent_id, since).await
        }
        DbPool::Sqlite(sqlite) => {
            sum_llm_consumed_tokens_for_agent_since_sqlite(sqlite, tenant_id, agent_id, since).await
        }
    }
}

pub async fn sum_llm_consumed_tokens_for_model_since_dual(
    pool: &DbPool,
    tenant_id: &str,
    model_key: &str,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            sum_llm_consumed_tokens_for_model_since(pg, tenant_id, model_key, since).await
        }
        DbPool::Sqlite(sqlite) => {
            sum_llm_consumed_tokens_for_model_since_sqlite(sqlite, tenant_id, model_key, since)
                .await
        }
    }
}

pub async fn persist_artifact_metadata_dual(
    pool: &DbPool,
    new_artifact: &NewArtifact,
) -> Result<ArtifactRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => persist_artifact_metadata(pg, new_artifact).await,
        DbPool::Sqlite(sqlite) => persist_artifact_metadata_sqlite(sqlite, new_artifact).await,
    }
}

pub async fn claim_next_queued_run_dual(
    pool: &DbPool,
    worker_id: &str,
    lease_for: Duration,
) -> Result<Option<RunLeaseRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => claim_next_queued_run(pg, worker_id, lease_for).await,
        DbPool::Sqlite(sqlite) => claim_next_queued_run_sqlite(sqlite, worker_id, lease_for).await,
    }
}

pub async fn renew_run_lease_dual(
    pool: &DbPool,
    run_id: Uuid,
    worker_id: &str,
    lease_for: Duration,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => renew_run_lease(pg, run_id, worker_id, lease_for).await,
        DbPool::Sqlite(sqlite) => {
            renew_run_lease_sqlite(sqlite, run_id, worker_id, lease_for).await
        }
    }
}

pub async fn requeue_expired_runs_dual(pool: &DbPool, limit: i64) -> Result<u64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => requeue_expired_runs(pg, limit).await,
        DbPool::Sqlite(sqlite) => requeue_expired_runs_sqlite(sqlite, limit).await,
    }
}

async fn create_action_request_sqlite(
    pool: &SqlitePool,
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
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        RETURNING id, step_id, action_type, status, decision_reason, created_at
        "#,
    )
    .bind(new_request.id.to_string())
    .bind(new_request.step_id.to_string())
    .bind(&new_request.action_type)
    .bind(new_request.args_json.to_string())
    .bind(&new_request.justification)
    .bind(&new_request.status)
    .bind(&new_request.decision_reason)
    .fetch_one(pool)
    .await?;

    Ok(ActionRequestRecord {
        id: parse_uuid_required(&row, "id")?,
        step_id: parse_uuid_required(&row, "step_id")?,
        action_type: row.get("action_type"),
        status: row.get("status"),
        decision_reason: row.get("decision_reason"),
        created_at: parse_datetime_required(&row, "created_at")?,
    })
}

async fn update_action_request_status_sqlite(
    pool: &SqlitePool,
    action_request_id: Uuid,
    status: &str,
    decision_reason: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE action_requests
        SET status = ?2,
            decision_reason = ?3
        WHERE id = ?1
        "#,
    )
    .bind(action_request_id.to_string())
    .bind(status)
    .bind(decision_reason)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

async fn create_action_result_sqlite(
    pool: &SqlitePool,
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
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(action_request_id) DO UPDATE
            SET status = excluded.status,
                result_json = excluded.result_json,
                error_json = excluded.error_json,
                executed_at = CURRENT_TIMESTAMP
        RETURNING id, action_request_id, status, executed_at
        "#,
    )
    .bind(new_result.id.to_string())
    .bind(new_result.action_request_id.to_string())
    .bind(&new_result.status)
    .bind(new_result.result_json.as_ref().map(Value::to_string))
    .bind(new_result.error_json.as_ref().map(Value::to_string))
    .fetch_one(pool)
    .await?;

    Ok(ActionResultRecord {
        id: parse_uuid_required(&row, "id")?,
        action_request_id: parse_uuid_required(&row, "action_request_id")?,
        status: row.get("status"),
        executed_at: parse_datetime_required(&row, "executed_at")?,
    })
}

async fn create_or_get_payment_request_sqlite(
    pool: &SqlitePool,
    new_request: &NewPaymentRequest,
) -> Result<PaymentRequestRecord, sqlx::Error> {
    sqlx::query(
        r#"
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
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ON CONFLICT (tenant_id, idempotency_key) DO NOTHING
        "#,
    )
    .bind(new_request.id.to_string())
    .bind(new_request.action_request_id.to_string())
    .bind(new_request.run_id.to_string())
    .bind(&new_request.tenant_id)
    .bind(new_request.agent_id.to_string())
    .bind(&new_request.provider)
    .bind(&new_request.operation)
    .bind(&new_request.destination)
    .bind(&new_request.idempotency_key)
    .bind(new_request.amount_msat)
    .bind(new_request.request_json.to_string())
    .bind(&new_request.status)
    .execute(pool)
    .await?;

    let row = sqlx::query(
        r#"
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
        WHERE tenant_id = ?1
          AND idempotency_key = ?2
        LIMIT 1
        "#,
    )
    .bind(&new_request.tenant_id)
    .bind(&new_request.idempotency_key)
    .fetch_one(pool)
    .await?;

    Ok(PaymentRequestRecord {
        id: parse_uuid_required(&row, "id")?,
        action_request_id: parse_uuid_required(&row, "action_request_id")?,
        run_id: parse_uuid_required(&row, "run_id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_uuid_required(&row, "agent_id")?,
        provider: row.get("provider"),
        operation: row.get("operation"),
        destination: row.get("destination"),
        idempotency_key: row.get("idempotency_key"),
        amount_msat: row.get("amount_msat"),
        request_json: parse_json_required(&row, "request_json")?,
        status: row.get("status"),
        created_at: parse_datetime_required(&row, "created_at")?,
        updated_at: parse_datetime_required(&row, "updated_at")?,
    })
}

async fn create_payment_result_sqlite(
    pool: &SqlitePool,
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
        VALUES (?1, ?2, ?3, ?4, ?5)
        RETURNING id,
                  payment_request_id,
                  status,
                  result_json,
                  error_json,
                  created_at
        "#,
    )
    .bind(new_result.id.to_string())
    .bind(new_result.payment_request_id.to_string())
    .bind(&new_result.status)
    .bind(new_result.result_json.as_ref().map(Value::to_string))
    .bind(new_result.error_json.as_ref().map(Value::to_string))
    .fetch_one(pool)
    .await?;

    Ok(PaymentResultRecord {
        id: parse_uuid_required(&row, "id")?,
        payment_request_id: parse_uuid_required(&row, "payment_request_id")?,
        status: row.get("status"),
        result_json: parse_json_optional(&row, "result_json")?,
        error_json: parse_json_optional(&row, "error_json")?,
        created_at: parse_datetime_required(&row, "created_at")?,
    })
}

async fn get_latest_payment_result_sqlite(
    pool: &SqlitePool,
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
        WHERE payment_request_id = ?1
        ORDER BY datetime(created_at) DESC, id DESC
        LIMIT 1
        "#,
    )
    .bind(payment_request_id.to_string())
    .fetch_optional(pool)
    .await?;

    row.map(|row| {
        Ok(PaymentResultRecord {
            id: parse_uuid_required(&row, "id")?,
            payment_request_id: parse_uuid_required(&row, "payment_request_id")?,
            status: row.get("status"),
            result_json: parse_json_optional(&row, "result_json")?,
            error_json: parse_json_optional(&row, "error_json")?,
            created_at: parse_datetime_required(&row, "created_at")?,
        })
    })
    .transpose()
}

async fn sum_executed_payment_amount_msat_for_tenant_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(amount_msat), 0)
        FROM payment_requests
        WHERE tenant_id = ?1
          AND operation = 'pay_invoice'
          AND status = 'executed'
        "#,
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await
}

async fn sum_executed_payment_amount_msat_for_agent_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    agent_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(amount_msat), 0)
        FROM payment_requests
        WHERE tenant_id = ?1
          AND agent_id = ?2
          AND operation = 'pay_invoice'
          AND status = 'executed'
        "#,
    )
    .bind(tenant_id)
    .bind(agent_id.to_string())
    .fetch_one(pool)
    .await
}

async fn update_payment_request_status_sqlite(
    pool: &SqlitePool,
    payment_request_id: Uuid,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE payment_requests
        SET status = ?2,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
    )
    .bind(payment_request_id.to_string())
    .bind(status)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

async fn create_llm_token_usage_record_sqlite(
    pool: &SqlitePool,
    new_record: &NewLlmTokenUsageRecord,
) -> Result<LlmTokenUsageRecord, sqlx::Error> {
    let window_started_at = new_record
        .window_started_at
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
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
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
    .bind(new_record.id.to_string())
    .bind(new_record.run_id.to_string())
    .bind(new_record.action_request_id.to_string())
    .bind(&new_record.tenant_id)
    .bind(new_record.agent_id.to_string())
    .bind(&new_record.route)
    .bind(&new_record.model_key)
    .bind(new_record.consumed_tokens)
    .bind(new_record.estimated_cost_usd)
    .bind(window_started_at)
    .bind(new_record.window_duration_seconds)
    .fetch_one(pool)
    .await?;

    Ok(LlmTokenUsageRecord {
        id: parse_uuid_required(&row, "id")?,
        run_id: parse_uuid_required(&row, "run_id")?,
        action_request_id: parse_uuid_required(&row, "action_request_id")?,
        tenant_id: row.get("tenant_id"),
        agent_id: parse_uuid_required(&row, "agent_id")?,
        route: row.get("route"),
        model_key: row.get("model_key"),
        consumed_tokens: row.get("consumed_tokens"),
        estimated_cost_usd: row.get("estimated_cost_usd"),
        window_started_at: parse_datetime_required(&row, "window_started_at")?,
        window_duration_seconds: row.get("window_duration_seconds"),
        created_at: parse_datetime_required(&row, "created_at")?,
    })
}

async fn sum_llm_consumed_tokens_for_tenant_since_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    let since_text = since.format(&Rfc3339).map_err(sqlite_protocol_error)?;
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0)
        FROM llm_token_usage
        WHERE tenant_id = ?1
          AND route = 'remote'
          AND datetime(created_at) >= datetime(?2)
        "#,
    )
    .bind(tenant_id)
    .bind(since_text)
    .fetch_one(pool)
    .await
}

async fn sum_llm_consumed_tokens_for_agent_since_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    agent_id: Uuid,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    let since_text = since.format(&Rfc3339).map_err(sqlite_protocol_error)?;
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0)
        FROM llm_token_usage
        WHERE tenant_id = ?1
          AND agent_id = ?2
          AND route = 'remote'
          AND datetime(created_at) >= datetime(?3)
        "#,
    )
    .bind(tenant_id)
    .bind(agent_id.to_string())
    .bind(since_text)
    .fetch_one(pool)
    .await
}

async fn sum_llm_consumed_tokens_for_model_since_sqlite(
    pool: &SqlitePool,
    tenant_id: &str,
    model_key: &str,
    since: OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    let since_text = since.format(&Rfc3339).map_err(sqlite_protocol_error)?;
    sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(consumed_tokens), 0)
        FROM llm_token_usage
        WHERE tenant_id = ?1
          AND model_key = ?2
          AND route = 'remote'
          AND datetime(created_at) >= datetime(?3)
        "#,
    )
    .bind(tenant_id)
    .bind(model_key)
    .bind(since_text)
    .fetch_one(pool)
    .await
}

async fn persist_artifact_metadata_sqlite(
    pool: &SqlitePool,
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
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        RETURNING id, run_id, path, content_type, size_bytes, storage_ref, created_at
        "#,
    )
    .bind(new_artifact.id.to_string())
    .bind(new_artifact.run_id.to_string())
    .bind(&new_artifact.path)
    .bind(&new_artifact.content_type)
    .bind(new_artifact.size_bytes)
    .bind(&new_artifact.checksum)
    .bind(&new_artifact.storage_ref)
    .fetch_one(pool)
    .await?;

    Ok(ArtifactRecord {
        id: parse_uuid_required(&row, "id")?,
        run_id: parse_uuid_required(&row, "run_id")?,
        path: row.get("path"),
        content_type: row.get("content_type"),
        size_bytes: row.get("size_bytes"),
        storage_ref: row.get("storage_ref"),
        created_at: parse_datetime_required(&row, "created_at")?,
    })
}

async fn claim_next_queued_run_sqlite(
    pool: &SqlitePool,
    worker_id: &str,
    lease_for: Duration,
) -> Result<Option<RunLeaseRecord>, sqlx::Error> {
    let now = OffsetDateTime::now_utc();
    let now_text = now.format(&Rfc3339).map_err(sqlite_protocol_error)?;
    let lease_expires_at = now + time::Duration::milliseconds(clamp_lease_ms(lease_for));
    let lease_expires_at_text = lease_expires_at
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;

    for _ in 0..3 {
        let candidate_id: Option<String> = sqlx::query_scalar(
            r#"
            SELECT id
            FROM runs
            WHERE status = 'queued'
              AND (lease_expires_at IS NULL OR datetime(lease_expires_at) < datetime('now'))
            ORDER BY
              CASE
                WHEN lower(COALESCE(json_extract(input_json, '$.queue_class'), json_extract(input_json, '$.llm_queue_class'), 'interactive')) = 'batch'
                  AND datetime(created_at) <= datetime('now', '-15 minutes') THEN 0
                WHEN lower(COALESCE(json_extract(input_json, '$.queue_class'), json_extract(input_json, '$.llm_queue_class'), 'interactive')) = 'interactive' THEN 0
                WHEN lower(COALESCE(json_extract(input_json, '$.queue_class'), json_extract(input_json, '$.llm_queue_class'), 'interactive')) = 'batch' THEN 1
                ELSE 0
              END,
              datetime(created_at) ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(pool)
        .await?;

        let Some(candidate_id) = candidate_id else {
            return Ok(None);
        };

        let claim = sqlx::query(
            r#"
            UPDATE runs
            SET status = 'running',
                started_at = COALESCE(started_at, ?2),
                attempts = attempts + 1,
                lease_owner = ?3,
                lease_expires_at = ?4
            WHERE id = ?1
              AND status = 'queued'
              AND (lease_expires_at IS NULL OR datetime(lease_expires_at) < datetime('now'))
            "#,
        )
        .bind(&candidate_id)
        .bind(&now_text)
        .bind(worker_id)
        .bind(&lease_expires_at_text)
        .execute(pool)
        .await?;

        if claim.rows_affected() != 1 {
            continue;
        }

        let row = sqlx::query(
            r#"
            SELECT id,
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
            FROM runs
            WHERE id = ?1
            "#,
        )
        .bind(&candidate_id)
        .fetch_one(pool)
        .await?;

        return Ok(Some(RunLeaseRecord {
            id: parse_uuid_required(&row, "id")?,
            tenant_id: row.get("tenant_id"),
            agent_id: parse_uuid_required(&row, "agent_id")?,
            triggered_by_user_id: parse_uuid_optional(&row, "triggered_by_user_id")?,
            recipe_id: row.get("recipe_id"),
            status: row.get("status"),
            input_json: parse_json_required(&row, "input_json")?,
            granted_capabilities: parse_json_required(&row, "granted_capabilities")?,
            attempts: row.get("attempts"),
            lease_owner: row.get("lease_owner"),
            lease_expires_at: parse_datetime_optional(&row, "lease_expires_at")?,
            created_at: parse_datetime_required(&row, "created_at")?,
            started_at: parse_datetime_optional(&row, "started_at")?,
        }));
    }

    Ok(None)
}

async fn renew_run_lease_sqlite(
    pool: &SqlitePool,
    run_id: Uuid,
    worker_id: &str,
    lease_for: Duration,
) -> Result<bool, sqlx::Error> {
    let lease_expires_at = (OffsetDateTime::now_utc()
        + time::Duration::milliseconds(clamp_lease_ms(lease_for)))
    .format(&Rfc3339)
    .map_err(sqlite_protocol_error)?;

    let result = sqlx::query(
        r#"
        UPDATE runs
        SET lease_expires_at = ?3
        WHERE id = ?1
          AND status = 'running'
          AND lease_owner = ?2
          AND lease_expires_at IS NOT NULL
          AND datetime(lease_expires_at) > datetime('now')
        "#,
    )
    .bind(run_id.to_string())
    .bind(worker_id)
    .bind(lease_expires_at)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

async fn requeue_expired_runs_sqlite(pool: &SqlitePool, limit: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE runs
        SET status = 'queued',
            lease_owner = NULL,
            lease_expires_at = NULL
        WHERE id IN (
            SELECT id
            FROM runs
            WHERE status = 'running'
              AND lease_expires_at IS NOT NULL
              AND datetime(lease_expires_at) < datetime('now')
            ORDER BY datetime(lease_expires_at) ASC
            LIMIT ?1
        )
        "#,
    )
    .bind(limit.max(0))
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
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

fn clamp_lease_ms(lease_for: Duration) -> i64 {
    lease_for
        .as_millis()
        .clamp(1, i64::MAX as u128)
        .try_into()
        .unwrap_or(i64::MAX)
}
