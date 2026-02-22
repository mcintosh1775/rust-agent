use crate::db::{
    claim_next_queued_run, claim_next_queued_run_with_limits, claim_pending_compliance_siem_delivery_records, compact_memory_records,
    create_action_request, create_action_result, create_llm_token_usage_record,
    create_or_get_payment_request, create_payment_result, dispatch_next_due_trigger_with_limits,
    count_inflight_runs, get_latest_payment_result, mark_compliance_siem_delivery_record_dead_lettered,
    mark_compliance_siem_delivery_record_delivered, mark_compliance_siem_delivery_record_failed,
    persist_artifact_metadata, renew_run_lease, requeue_expired_runs,
    sum_executed_payment_amount_msat_for_agent, sum_executed_payment_amount_msat_for_tenant,
    sum_llm_consumed_tokens_for_agent_since, sum_llm_consumed_tokens_for_model_since,
    sum_llm_consumed_tokens_for_tenant_since, try_acquire_scheduler_lease,
    update_action_request_status, update_payment_request_status, ActionRequestRecord,
    ActionResultRecord, ArtifactRecord, ComplianceSiemDeliveryRecord, LlmTokenUsageRecord,
    MemoryCompactionGroupOutcome, MemoryCompactionRunStats, NewActionRequest, NewActionResult,
    NewArtifact, NewLlmTokenUsageRecord, NewPaymentRequest, NewPaymentResult, PaymentRequestRecord,
    PaymentResultRecord, RunLeaseRecord, SchedulerLeaseParams, TriggerDispatchRecord,
};
use crate::db_pool::DbPool;
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use std::time::Duration;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

const TRIGGER_ERROR_CLASS_TRIGGER_POLICY: &str = "trigger_policy";
const TRIGGER_ERROR_CLASS_SCHEDULE: &str = "schedule";
const TRIGGER_ERROR_CLASS_EVENT_PAYLOAD: &str = "event_payload";

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

pub async fn claim_next_queued_run_with_limits_dual(
    pool: &DbPool,
    worker_id: &str,
    lease_for: Duration,
    global_max_inflight_runs: i64,
    tenant_max_inflight_runs: i64,
) -> Result<Option<RunLeaseRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            claim_next_queued_run_with_limits(
                pg,
                worker_id,
                lease_for,
                global_max_inflight_runs,
                tenant_max_inflight_runs,
            )
            .await
        }
        DbPool::Sqlite(sqlite) => {
            claim_next_queued_run_with_limits_sqlite(
                sqlite,
                worker_id,
                lease_for,
                global_max_inflight_runs,
                tenant_max_inflight_runs,
            )
            .await
        }
    }
}

pub async fn count_inflight_runs_dual(pool: &DbPool) -> Result<i64, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => count_inflight_runs(pg).await,
        DbPool::Sqlite(sqlite) => {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM runs
                WHERE status IN ('queued', 'running')
                "#,
            )
            .fetch_one(sqlite)
            .await
        }
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

pub async fn compact_memory_records_dual(
    pool: &DbPool,
    older_than_or_equal: OffsetDateTime,
    min_records: i64,
    max_groups: i64,
) -> Result<MemoryCompactionRunStats, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            compact_memory_records(pg, older_than_or_equal, min_records, max_groups).await
        }
        DbPool::Sqlite(sqlite) => {
            compact_memory_records_sqlite(sqlite, older_than_or_equal, min_records, max_groups)
                .await
        }
    }
}

pub async fn claim_pending_compliance_siem_delivery_records_dual(
    pool: &DbPool,
    lease_owner: &str,
    lease_for: Duration,
    limit: i64,
) -> Result<Vec<ComplianceSiemDeliveryRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            claim_pending_compliance_siem_delivery_records(pg, lease_owner, lease_for, limit).await
        }
        DbPool::Sqlite(sqlite) => {
            claim_pending_compliance_siem_delivery_records_sqlite(
                sqlite,
                lease_owner,
                lease_for,
                limit,
            )
            .await
        }
    }
}

pub async fn mark_compliance_siem_delivery_record_delivered_dual(
    pool: &DbPool,
    record_id: Uuid,
    http_status: Option<i32>,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            mark_compliance_siem_delivery_record_delivered(pg, record_id, http_status).await
        }
        DbPool::Sqlite(sqlite) => {
            mark_compliance_siem_delivery_record_delivered_sqlite(sqlite, record_id, http_status)
                .await
        }
    }
}

pub async fn mark_compliance_siem_delivery_record_failed_dual(
    pool: &DbPool,
    record_id: Uuid,
    error_message: &str,
    http_status: Option<i32>,
    retry_at: OffsetDateTime,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            mark_compliance_siem_delivery_record_failed(
                pg,
                record_id,
                error_message,
                http_status,
                retry_at,
            )
            .await
        }
        DbPool::Sqlite(sqlite) => {
            mark_compliance_siem_delivery_record_failed_sqlite(
                sqlite,
                record_id,
                error_message,
                http_status,
                retry_at,
            )
            .await
        }
    }
}

pub async fn mark_compliance_siem_delivery_record_dead_lettered_dual(
    pool: &DbPool,
    record_id: Uuid,
    error_message: &str,
    http_status: Option<i32>,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            mark_compliance_siem_delivery_record_dead_lettered(
                pg,
                record_id,
                error_message,
                http_status,
            )
            .await
        }
        DbPool::Sqlite(sqlite) => {
            mark_compliance_siem_delivery_record_dead_lettered_sqlite(
                sqlite,
                record_id,
                error_message,
                http_status,
            )
            .await
        }
    }
}

pub async fn try_acquire_scheduler_lease_dual(
    pool: &DbPool,
    params: &SchedulerLeaseParams,
) -> Result<bool, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => try_acquire_scheduler_lease(pg, params).await,
        DbPool::Sqlite(sqlite) => try_acquire_scheduler_lease_sqlite(sqlite, params).await,
    }
}

pub async fn dispatch_next_due_trigger_with_limits_dual(
    pool: &DbPool,
    tenant_max_inflight_runs: i64,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            dispatch_next_due_trigger_with_limits(pg, tenant_max_inflight_runs).await
        }
        DbPool::Sqlite(sqlite) => {
            dispatch_next_due_trigger_with_limits_sqlite(sqlite, tenant_max_inflight_runs).await
        }
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

async fn claim_next_queued_run_with_limits_sqlite(
    pool: &SqlitePool,
    worker_id: &str,
    lease_for: Duration,
    global_max_inflight_runs: i64,
    tenant_max_inflight_runs: i64,
) -> Result<Option<RunLeaseRecord>, sqlx::Error> {
    let now = OffsetDateTime::now_utc();
    let now_text = now.format(&Rfc3339).map_err(sqlite_protocol_error)?;
    let lease_expires_at = now + time::Duration::milliseconds(clamp_lease_ms(lease_for));
    let lease_expires_at_text = lease_expires_at
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
    let global_cap = global_max_inflight_runs.max(1);
    let tenant_cap = tenant_max_inflight_runs.max(1);

    for _ in 0..3 {
        let candidate = sqlx::query(
            r#"
            SELECT id, tenant_id
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
        .await?
        ;

        let Some(candidate_row) = candidate else {
            return Ok(None);
        };

        let candidate_id: String = candidate_row.get("id");
        let candidate_tenant_id: String = candidate_row.get("tenant_id");

        let global_inflight_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM runs
            WHERE status = 'running'
            "#,
        )
        .fetch_one(pool)
        .await?;

        if global_inflight_count >= global_cap {
            return Ok(None);
        }

        let tenant_inflight_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM runs
            WHERE tenant_id = ?1
              AND status = 'running'
            "#,
        )
        .bind(&candidate_tenant_id)
        .fetch_one(pool)
        .await?;

        if tenant_inflight_count >= tenant_cap {
            return Ok(None);
        }

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

async fn compact_memory_records_sqlite(
    pool: &SqlitePool,
    older_than_or_equal: OffsetDateTime,
    min_records: i64,
    max_groups: i64,
) -> Result<MemoryCompactionRunStats, sqlx::Error> {
    let min_records = min_records.max(2);
    let max_groups = max_groups.max(1);
    let cutoff_text = older_than_or_equal
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;

    let candidate_rows = sqlx::query(
        r#"
        SELECT mr.tenant_id,
               mr.agent_id,
               mr.memory_kind,
               mr.scope,
               COUNT(*) AS source_count,
               (
                   SELECT run_id
                   FROM memory_records latest
                   WHERE latest.tenant_id = mr.tenant_id
                     AND latest.agent_id = mr.agent_id
                     AND latest.memory_kind = mr.memory_kind
                     AND latest.scope = mr.scope
                     AND latest.compacted_at IS NULL
                     AND (latest.expires_at IS NULL OR datetime(latest.expires_at) > datetime('now'))
                     AND datetime(latest.created_at) <= datetime(?1)
                   ORDER BY datetime(latest.created_at) DESC, latest.id DESC
                   LIMIT 1
               ) AS representative_run_id,
               (
                   SELECT step_id
                   FROM memory_records latest
                   WHERE latest.tenant_id = mr.tenant_id
                     AND latest.agent_id = mr.agent_id
                     AND latest.memory_kind = mr.memory_kind
                     AND latest.scope = mr.scope
                     AND latest.compacted_at IS NULL
                     AND (latest.expires_at IS NULL OR datetime(latest.expires_at) > datetime('now'))
                     AND datetime(latest.created_at) <= datetime(?1)
                   ORDER BY datetime(latest.created_at) DESC, latest.id DESC
                   LIMIT 1
               ) AS representative_step_id
        FROM memory_records mr
        WHERE mr.compacted_at IS NULL
          AND (mr.expires_at IS NULL OR datetime(mr.expires_at) > datetime('now'))
          AND datetime(mr.created_at) <= datetime(?1)
        GROUP BY mr.tenant_id, mr.agent_id, mr.memory_kind, mr.scope
        HAVING COUNT(*) >= ?2
        ORDER BY MIN(datetime(mr.created_at)) ASC
        LIMIT ?3
        "#,
    )
    .bind(&cutoff_text)
    .bind(min_records)
    .bind(max_groups)
    .fetch_all(pool)
    .await?;

    let mut processed_groups = 0_i64;
    let mut compacted_source_records = 0_i64;
    let mut groups = Vec::new();

    for candidate in candidate_rows {
        let tenant_id: String = candidate.get("tenant_id");
        let agent_id = parse_uuid_required(&candidate, "agent_id")?;
        let memory_kind: String = candidate.get("memory_kind");
        let scope: String = candidate.get("scope");
        let representative_run_id = parse_uuid_optional(&candidate, "representative_run_id")?;
        let representative_step_id = parse_uuid_optional(&candidate, "representative_step_id")?;

        let source_rows = sqlx::query(
            r#"
            SELECT id
            FROM memory_records
            WHERE tenant_id = ?1
              AND agent_id = ?2
              AND memory_kind = ?3
              AND scope = ?4
              AND compacted_at IS NULL
              AND (expires_at IS NULL OR datetime(expires_at) > datetime('now'))
              AND datetime(created_at) <= datetime(?5)
            ORDER BY datetime(created_at) ASC, id ASC
            "#,
        )
        .bind(&tenant_id)
        .bind(agent_id.to_string())
        .bind(&memory_kind)
        .bind(&scope)
        .bind(&cutoff_text)
        .fetch_all(pool)
        .await?;

        let mut compacted_ids = Vec::new();
        for source_row in source_rows {
            let source_id = parse_uuid_required(&source_row, "id")?;
            let result = sqlx::query(
                r#"
                UPDATE memory_records
                SET compacted_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                  AND compacted_at IS NULL
                "#,
            )
            .bind(source_id.to_string())
            .execute(pool)
            .await?;
            if result.rows_affected() == 1 {
                compacted_ids.push(source_id);
            }
        }

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
            "memory_kind": memory_kind,
            "scope": scope,
            "source_count": compacted_count,
            "generated_at": OffsetDateTime::now_utc(),
        });
        sqlx::query(
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
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&tenant_id)
        .bind(agent_id.to_string())
        .bind(&memory_kind)
        .bind(&scope)
        .bind(compacted_count.clamp(1, i32::MAX as i64) as i32)
        .bind(source_entry_ids.to_string())
        .bind(summary_json.to_string())
        .execute(pool)
        .await?;

        processed_groups += 1;
        compacted_source_records += compacted_count;
        groups.push(MemoryCompactionGroupOutcome {
            tenant_id,
            agent_id,
            memory_kind,
            scope,
            source_count: compacted_count,
            source_entry_ids,
            representative_run_id,
            representative_step_id,
        });
    }

    Ok(MemoryCompactionRunStats {
        processed_groups,
        compacted_source_records,
        groups,
    })
}

async fn claim_pending_compliance_siem_delivery_records_sqlite(
    pool: &SqlitePool,
    lease_owner: &str,
    lease_for: Duration,
    limit: i64,
) -> Result<Vec<ComplianceSiemDeliveryRecord>, sqlx::Error> {
    let lease_expires_at = (OffsetDateTime::now_utc()
        + time::Duration::milliseconds(clamp_lease_ms(lease_for)))
    .format(&Rfc3339)
    .map_err(sqlite_protocol_error)?;
    let candidate_ids: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM compliance_siem_delivery_outbox
        WHERE status IN ('pending', 'failed')
          AND datetime(next_attempt_at) <= datetime('now')
          AND (lease_expires_at IS NULL OR datetime(lease_expires_at) <= datetime('now'))
        ORDER BY datetime(next_attempt_at) ASC, datetime(created_at) ASC, id ASC
        LIMIT ?1
        "#,
    )
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?;

    let mut claimed = Vec::new();
    for candidate_id in candidate_ids {
        let claim_result = sqlx::query(
            r#"
            UPDATE compliance_siem_delivery_outbox
            SET status = 'processing',
                leased_by = ?2,
                lease_expires_at = ?3,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
              AND status IN ('pending', 'failed')
              AND datetime(next_attempt_at) <= datetime('now')
              AND (lease_expires_at IS NULL OR datetime(lease_expires_at) <= datetime('now'))
            "#,
        )
        .bind(&candidate_id)
        .bind(lease_owner)
        .bind(&lease_expires_at)
        .execute(pool)
        .await?;
        if claim_result.rows_affected() != 1 {
            continue;
        }

        let row = sqlx::query(
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
            WHERE id = ?1
            "#,
        )
        .bind(&candidate_id)
        .fetch_one(pool)
        .await?;
        claimed.push(compliance_siem_delivery_from_sqlite_row(&row)?);
    }

    Ok(claimed)
}

async fn mark_compliance_siem_delivery_record_delivered_sqlite(
    pool: &SqlitePool,
    record_id: Uuid,
    http_status: Option<i32>,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET status = 'delivered',
            attempts = attempts + 1,
            last_error = NULL,
            last_http_status = ?2,
            leased_by = NULL,
            lease_expires_at = NULL,
            delivered_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
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
    .bind(record_id.to_string())
    .bind(http_status)
    .fetch_one(pool)
    .await?;

    compliance_siem_delivery_from_sqlite_row(&row)
}

async fn mark_compliance_siem_delivery_record_failed_sqlite(
    pool: &SqlitePool,
    record_id: Uuid,
    error_message: &str,
    http_status: Option<i32>,
    retry_at: OffsetDateTime,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    let retry_text = retry_at.format(&Rfc3339).map_err(sqlite_protocol_error)?;
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET attempts = attempts + 1,
            status = CASE
              WHEN attempts + 1 >= max_attempts THEN 'dead_lettered'
              ELSE 'failed'
            END,
            last_error = ?2,
            last_http_status = ?3,
            leased_by = NULL,
            lease_expires_at = NULL,
            next_attempt_at = CASE
              WHEN attempts + 1 >= max_attempts THEN CURRENT_TIMESTAMP
              ELSE ?4
            END,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
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
    .bind(record_id.to_string())
    .bind(error_message)
    .bind(http_status)
    .bind(retry_text)
    .fetch_one(pool)
    .await?;

    compliance_siem_delivery_from_sqlite_row(&row)
}

async fn mark_compliance_siem_delivery_record_dead_lettered_sqlite(
    pool: &SqlitePool,
    record_id: Uuid,
    error_message: &str,
    http_status: Option<i32>,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    let row = sqlx::query(
        r#"
        UPDATE compliance_siem_delivery_outbox
        SET attempts = attempts + 1,
            status = 'dead_lettered',
            last_error = ?2,
            last_http_status = ?3,
            leased_by = NULL,
            lease_expires_at = NULL,
            next_attempt_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
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
    .bind(record_id.to_string())
    .bind(error_message)
    .bind(http_status)
    .fetch_one(pool)
    .await?;

    compliance_siem_delivery_from_sqlite_row(&row)
}

async fn try_acquire_scheduler_lease_sqlite(
    pool: &SqlitePool,
    params: &SchedulerLeaseParams,
) -> Result<bool, sqlx::Error> {
    let lease_expires_at = (OffsetDateTime::now_utc()
        + time::Duration::milliseconds(clamp_lease_ms(params.lease_for)))
    .format(&Rfc3339)
    .map_err(sqlite_protocol_error)?;
    let acquired_owner: Option<String> = sqlx::query_scalar(
        r#"
        INSERT INTO scheduler_leases (lease_name, lease_owner, lease_expires_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT (lease_name) DO UPDATE
            SET lease_owner = excluded.lease_owner,
                lease_expires_at = excluded.lease_expires_at,
                updated_at = CURRENT_TIMESTAMP
        WHERE datetime(scheduler_leases.lease_expires_at) < datetime('now')
           OR scheduler_leases.lease_owner = excluded.lease_owner
        RETURNING lease_owner
        "#,
    )
    .bind(&params.lease_name)
    .bind(&params.lease_owner)
    .bind(lease_expires_at)
    .fetch_optional(pool)
    .await?;

    Ok(acquired_owner.as_deref() == Some(params.lease_owner.as_str()))
}

async fn dispatch_next_due_trigger_with_limits_sqlite(
    pool: &SqlitePool,
    tenant_max_inflight_runs: i64,
) -> Result<Option<TriggerDispatchRecord>, sqlx::Error> {
    if let Some(dispatch) =
        dispatch_next_due_webhook_event_sqlite(pool, tenant_max_inflight_runs).await?
    {
        return Ok(Some(dispatch));
    }
    if let Some(dispatch) =
        dispatch_next_due_cron_trigger_sqlite(pool, tenant_max_inflight_runs).await?
    {
        return Ok(Some(dispatch));
    }
    dispatch_next_due_interval_trigger_with_limits_sqlite(pool, tenant_max_inflight_runs).await
}

async fn dispatch_next_due_interval_trigger_with_limits_sqlite(
    pool: &SqlitePool,
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
          AND datetime(t.next_fire_at) <= datetime('now')
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
          ) < ?1
        ORDER BY CASE
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'batch'
                   AND datetime(t.next_fire_at) <= datetime('now', '-15 minutes') THEN 0
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'interactive' THEN 0
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'batch' THEN 1
                 ELSE 0
                 END,
                 datetime(t.next_fire_at) ASC, t.id ASC
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

    let trigger_id = parse_uuid_required(&candidate, "id")?;
    let tenant_id: String = candidate.get("tenant_id");
    let agent_id = parse_uuid_required(&candidate, "agent_id")?;
    let triggered_by_user_id = parse_uuid_optional(&candidate, "triggered_by_user_id")?;
    let recipe_id: String = candidate.get("recipe_id");
    let input_json = parse_json_required(&candidate, "input_json")?;
    let requested_capabilities = parse_json_required(&candidate, "requested_capabilities")?;
    let granted_capabilities = parse_json_required(&candidate, "granted_capabilities")?;
    let interval_seconds: i64 = candidate.get("interval_seconds");
    let misfire_policy: String = candidate.get("misfire_policy");
    let jitter_seconds: i32 = candidate.get("jitter_seconds");
    let scheduled_for_raw: String = candidate.get("scheduled_for");
    let scheduled_for = parse_datetime_str(scheduled_for_raw.as_str()).map_err(|err| {
        sqlx::Error::Protocol(
            format!(
                "invalid interval trigger scheduled_for datetime: {err} (value={scheduled_for_raw})"
            )
            .into(),
        )
    })?;
    let dedupe_key = scheduled_for.unix_timestamp_nanos().to_string();
    let now = OffsetDateTime::now_utc();
    let interval = time::Duration::seconds(interval_seconds);

    if misfire_policy == "skip" && (now - scheduled_for) >= interval {
        let next_fire_at = apply_jitter(now + interval, trigger_id, jitter_seconds, now);
        let next_fire_at_text = next_fire_at
            .format(&Rfc3339)
            .map_err(sqlite_protocol_error)?;
        let reserve_result = sqlx::query(
            r#"
            UPDATE triggers
            SET next_fire_at = ?2,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
              AND status = 'enabled'
              AND dead_lettered_at IS NULL
              AND next_fire_at = ?3
            "#,
        )
        .bind(trigger_id.to_string())
        .bind(next_fire_at_text)
        .bind(&scheduled_for_raw)
        .execute(&mut *tx)
        .await?;
        if reserve_result.rows_affected() == 1 {
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
                VALUES (?1, ?2, NULL, ?3, 'failed', ?4, ?5)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(trigger_id.to_string())
            .bind(scheduled_for_raw)
            .bind(dedupe_key)
            .bind(
                trigger_error_payload(
                    "MISFIRE_SKIPPED",
                    "interval trigger misfire skipped",
                    TRIGGER_ERROR_CLASS_TRIGGER_POLICY,
                )
                .to_string(),
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        return Ok(None);
    }

    let run_id = Uuid::new_v4();
    let run_input = inject_trace_id(input_json, &run_id.to_string());
    let next_fire_at = apply_jitter(scheduled_for + interval, trigger_id, jitter_seconds, now);
    let next_fire_at_text = next_fire_at
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
    let reserve_result = sqlx::query(
        r#"
        UPDATE triggers
        SET next_fire_at = ?2,
            last_fired_at = CURRENT_TIMESTAMP,
            consecutive_failures = 0,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
          AND status = 'enabled'
          AND dead_lettered_at IS NULL
          AND next_fire_at = ?3
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(next_fire_at_text)
    .bind(&scheduled_for_raw)
    .execute(&mut *tx)
    .await?;
    if reserve_result.rows_affected() != 1 {
        tx.commit().await?;
        return Ok(None);
    }

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
        VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, NULL)
        "#,
    )
    .bind(run_id.to_string())
    .bind(&tenant_id)
    .bind(agent_id.to_string())
    .bind(triggered_by_user_id.map(|value| value.to_string()))
    .bind(&recipe_id)
    .bind(run_input.to_string())
    .bind(requested_capabilities.to_string())
    .bind(granted_capabilities.to_string())
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
        VALUES (?1, ?2, ?3, ?4, 'created', ?5, NULL)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(trigger_id.to_string())
    .bind(run_id.to_string())
    .bind(scheduled_for_raw)
    .bind(dedupe_key)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(TriggerDispatchRecord {
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
    }))
}

async fn dispatch_next_due_cron_trigger_sqlite(
    pool: &SqlitePool,
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
          AND datetime(t.next_fire_at) <= datetime('now')
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
          ) < ?1
        ORDER BY CASE
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'batch'
                   AND datetime(t.next_fire_at) <= datetime('now', '-15 minutes') THEN 0
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'interactive' THEN 0
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'batch' THEN 1
                 ELSE 0
                 END,
                 datetime(t.next_fire_at) ASC, t.id ASC
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

    let trigger_id = parse_uuid_required(&candidate, "id")?;
    let tenant_id: String = candidate.get("tenant_id");
    let agent_id = parse_uuid_required(&candidate, "agent_id")?;
    let triggered_by_user_id = parse_uuid_optional(&candidate, "triggered_by_user_id")?;
    let recipe_id: String = candidate.get("recipe_id");
    let input_json = parse_json_required(&candidate, "input_json")?;
    let requested_capabilities = parse_json_required(&candidate, "requested_capabilities")?;
    let granted_capabilities = parse_json_required(&candidate, "granted_capabilities")?;
    let cron_expression: String = candidate.get("cron_expression");
    let schedule_timezone: String = candidate.get("schedule_timezone");
    let jitter_seconds: i32 = candidate.get("jitter_seconds");
    let scheduled_for_raw: String = candidate.get("scheduled_for");
    let scheduled_for = parse_datetime_str(scheduled_for_raw.as_str()).map_err(|err| {
        sqlx::Error::Protocol(
            format!(
                "invalid cron trigger scheduled_for datetime: {err} (value={scheduled_for_raw})"
            )
            .into(),
        )
    })?;
    let dedupe_key = scheduled_for.unix_timestamp_nanos().to_string();

    let next_fire_at = match next_cron_fire_at(&cron_expression, &schedule_timezone, scheduled_for)
    {
        Ok(value) => apply_jitter(value, trigger_id, jitter_seconds, scheduled_for),
        Err(error_message) => {
            let update_result = sqlx::query(
                r#"
                UPDATE triggers
                SET dead_lettered_at = CURRENT_TIMESTAMP,
                    dead_letter_reason = ?2,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                  AND status = 'enabled'
                  AND dead_lettered_at IS NULL
                  AND next_fire_at = ?3
                "#,
            )
            .bind(trigger_id.to_string())
            .bind(format!("SCHEDULE_COMPUTE_FAILED: {error_message}"))
            .bind(&scheduled_for_raw)
            .execute(&mut *tx)
            .await?;
            if update_result.rows_affected() == 1 {
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
                    VALUES (?1, ?2, NULL, ?3, 'failed', ?4, ?5)
                    "#,
                )
                .bind(Uuid::new_v4().to_string())
                .bind(trigger_id.to_string())
                .bind(scheduled_for_raw)
                .bind(dedupe_key)
                .bind(
                    trigger_error_payload(
                        "CRON_COMPUTE_FAILED",
                        &error_message,
                        TRIGGER_ERROR_CLASS_SCHEDULE,
                    )
                    .to_string(),
                )
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            return Ok(None);
        }
    };

    let next_fire_at_text = next_fire_at
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
    let run_id = Uuid::new_v4();
    let run_input = inject_trace_id(input_json, &run_id.to_string());
    let reserve_result = sqlx::query(
        r#"
        UPDATE triggers
        SET next_fire_at = ?2,
            last_fired_at = CURRENT_TIMESTAMP,
            consecutive_failures = 0,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
          AND status = 'enabled'
          AND dead_lettered_at IS NULL
          AND next_fire_at = ?3
        "#,
    )
    .bind(trigger_id.to_string())
    .bind(next_fire_at_text)
    .bind(&scheduled_for_raw)
    .execute(&mut *tx)
    .await?;
    if reserve_result.rows_affected() != 1 {
        tx.commit().await?;
        return Ok(None);
    }

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
        VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, NULL)
        "#,
    )
    .bind(run_id.to_string())
    .bind(&tenant_id)
    .bind(agent_id.to_string())
    .bind(triggered_by_user_id.map(|value| value.to_string()))
    .bind(&recipe_id)
    .bind(run_input.to_string())
    .bind(requested_capabilities.to_string())
    .bind(granted_capabilities.to_string())
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
        VALUES (?1, ?2, ?3, ?4, 'created', ?5, NULL)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(trigger_id.to_string())
    .bind(run_id.to_string())
    .bind(scheduled_for_raw)
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

async fn dispatch_next_due_webhook_event_sqlite(
    pool: &SqlitePool,
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
          AND datetime(e.next_attempt_at) <= datetime('now')
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
          ) < ?1
        ORDER BY CASE
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'batch'
                   AND datetime(e.created_at) <= datetime('now', '-15 minutes') THEN 0
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'interactive' THEN 0
                 WHEN lower(
                     COALESCE(
                         json_extract(t.input_json, '$.queue_class'),
                         json_extract(t.input_json, '$.llm_queue_class'),
                         'interactive'
                     )
                 ) = 'batch' THEN 1
                 ELSE 0
                 END,
                 datetime(e.created_at) ASC, e.id ASC
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

    let trigger_event_row_id = parse_uuid_required(&candidate, "trigger_event_row_id")?;
    let trigger_id = parse_uuid_required(&candidate, "trigger_id")?;
    let tenant_id: String = candidate.get("tenant_id");
    let agent_id = parse_uuid_required(&candidate, "agent_id")?;
    let triggered_by_user_id = parse_uuid_optional(&candidate, "triggered_by_user_id")?;
    let recipe_id: String = candidate.get("recipe_id");
    let event_id: String = candidate.get("event_id");
    let payload_json = parse_json_required(&candidate, "payload_json")?;
    let attempts: i32 = candidate.get("attempts");
    let max_attempts: i32 = candidate.get("max_attempts");
    let input_json = parse_json_required(&candidate, "input_json")?;
    let requested_capabilities = parse_json_required(&candidate, "requested_capabilities")?;
    let granted_capabilities = parse_json_required(&candidate, "granted_capabilities")?;
    let scheduled_for = OffsetDateTime::now_utc();
    let scheduled_for_text = scheduled_for
        .format(&Rfc3339)
        .map_err(sqlite_protocol_error)?;
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
        let update_result = sqlx::query(
            r#"
            UPDATE trigger_events
            SET attempts = attempts + 1,
                status = CASE WHEN ?2 THEN 'dead_lettered' ELSE 'pending' END,
                next_attempt_at = CASE
                    WHEN ?2 THEN CURRENT_TIMESTAMP
                    ELSE datetime('now', '+30 seconds')
                END,
                last_error_json = ?3,
                dead_lettered_at = CASE WHEN ?2 THEN CURRENT_TIMESTAMP ELSE NULL END
            WHERE id = ?1
              AND status = 'pending'
            "#,
        )
        .bind(trigger_event_row_id.to_string())
        .bind(dead_letter)
        .bind(event_error.to_string())
        .execute(&mut *tx)
        .await?;
        if update_result.rows_affected() == 1 {
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
                VALUES (?1, ?2, NULL, ?3, 'failed', ?4, ?5)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(trigger_id.to_string())
            .bind(scheduled_for_text)
            .bind(&event_id)
            .bind(event_error.to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        return Ok(None);
    }

    let event_update = sqlx::query(
        r#"
        UPDATE trigger_events
        SET attempts = attempts + 1,
            status = 'processed',
            processed_at = CURRENT_TIMESTAMP,
            next_attempt_at = CURRENT_TIMESTAMP
        WHERE id = ?1
          AND status = 'pending'
        "#,
    )
    .bind(trigger_event_row_id.to_string())
    .execute(&mut *tx)
    .await?;
    if event_update.rows_affected() != 1 {
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
        VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, NULL)
        "#,
    )
    .bind(run_id.to_string())
    .bind(&tenant_id)
    .bind(agent_id.to_string())
    .bind(triggered_by_user_id.map(|value| value.to_string()))
    .bind(&recipe_id)
    .bind(run_input.to_string())
    .bind(requested_capabilities.to_string())
    .bind(granted_capabilities.to_string())
    .execute(&mut *tx)
    .await?;

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
        VALUES (?1, ?2, ?3, ?4, 'created', ?5, NULL)
        "#,
    )
    .bind(Uuid::new_v4().to_string())
    .bind(trigger_id.to_string())
    .bind(run_id.to_string())
    .bind(scheduled_for_text)
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

fn compliance_siem_delivery_from_sqlite_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<ComplianceSiemDeliveryRecord, sqlx::Error> {
    Ok(ComplianceSiemDeliveryRecord {
        id: parse_uuid_required(row, "id")?,
        tenant_id: row.get("tenant_id"),
        run_id: parse_uuid_optional(row, "run_id")?,
        adapter: row.get("adapter"),
        delivery_target: row.get("delivery_target"),
        content_type: row.get("content_type"),
        payload_ndjson: row.get("payload_ndjson"),
        status: row.get("status"),
        attempts: row.get("attempts"),
        max_attempts: row.get("max_attempts"),
        next_attempt_at: parse_datetime_required(row, "next_attempt_at")?,
        leased_by: row.get("leased_by"),
        lease_expires_at: parse_datetime_optional(row, "lease_expires_at")?,
        last_error: row.get("last_error"),
        last_http_status: row.get("last_http_status"),
        created_at: parse_datetime_required(row, "created_at")?,
        updated_at: parse_datetime_required(row, "updated_at")?,
        delivered_at: parse_datetime_optional(row, "delivered_at")?,
    })
}

fn trigger_error_payload(code: &str, message: impl Into<String>, reason_class: &str) -> Value {
    json!({
        "code": code,
        "message": message.into(),
        "reason_class": reason_class,
    })
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
