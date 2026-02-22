use core::{
    append_audit_event, claim_next_queued_run, claim_pending_compliance_siem_delivery_records,
    claim_next_queued_run_with_limits,
    compact_memory_records, count_tenant_triggers, create_action_request, create_action_result,
    create_compliance_siem_delivery_record, create_cron_trigger, create_interval_trigger,
    create_llm_token_usage_record, create_memory_compaction_record, create_memory_record,
    create_or_get_payment_request, create_payment_result, create_run, create_step,
    create_webhook_trigger, dispatch_next_due_interval_trigger,
    dispatch_next_due_interval_trigger_with_limits, dispatch_next_due_trigger,
    enqueue_trigger_event, fire_trigger_manually, fire_trigger_manually_with_limits,
    get_latest_payment_result, get_llm_gateway_cache_entry, get_run_status,
    get_tenant_action_latency_summary, get_tenant_action_latency_traces,
    get_tenant_compliance_audit_policy, get_tenant_compliance_siem_delivery_slo,
    get_tenant_compliance_siem_delivery_summary, get_tenant_memory_compaction_stats,
    get_tenant_ops_summary, get_tenant_run_latency_histogram, get_tenant_run_latency_traces,
    list_run_audit_events, list_tenant_compliance_audit_events,
    list_tenant_compliance_siem_delivery_records,
    list_tenant_compliance_siem_delivery_target_summaries, list_tenant_handoff_memory_records,
    list_tenant_memory_records, mark_compliance_siem_delivery_record_dead_lettered,
    mark_compliance_siem_delivery_record_delivered, mark_compliance_siem_delivery_record_failed,
    mark_run_succeeded, mark_step_succeeded, prune_llm_gateway_cache_namespace,
    purge_expired_tenant_compliance_audit_events, purge_expired_tenant_memory_records,
    release_llm_gateway_admission_lease, renew_run_lease,
    requeue_dead_letter_compliance_siem_delivery_record, requeue_dead_letter_trigger_event,
    requeue_expired_runs, sum_llm_consumed_tokens_for_agent_since,
    sum_llm_consumed_tokens_for_model_since, sum_llm_consumed_tokens_for_tenant_since,
    try_acquire_llm_gateway_admission_lease, try_acquire_scheduler_lease,
    update_action_request_status, update_payment_request_status, upsert_llm_gateway_cache_entry,
    upsert_tenant_compliance_audit_policy, verify_tenant_compliance_audit_chain,
    LlmGatewayAdmissionLeaseAcquireParams, ManualTriggerFireOutcome, NewActionRequest,
    NewActionResult, NewAuditEvent, NewComplianceSiemDeliveryRecord, NewCronTrigger,
    NewIntervalTrigger, NewLlmGatewayCacheEntry, NewLlmTokenUsageRecord, NewMemoryCompactionRecord,
    NewMemoryRecord, NewPaymentRequest, NewPaymentResult, NewRun, NewStep, NewWebhookTrigger,
    SchedulerLeaseParams, TriggerEventEnqueueOutcome, TriggerEventEnqueueUnavailableReason,
    TriggerEventReplayOutcome,
};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Row,
};
use std::time::{Duration, Instant};
use std::{env, str::FromStr};
use time::OffsetDateTime;
use uuid::Uuid;

const REQUIRED_TABLES: [&str; 23] = [
    "agents",
    "users",
    "runs",
    "steps",
    "artifacts",
    "action_requests",
    "action_results",
    "audit_events",
    "compliance_audit_events",
    "compliance_audit_policies",
    "compliance_siem_delivery_outbox",
    "triggers",
    "trigger_runs",
    "trigger_events",
    "trigger_audit_events",
    "scheduler_leases",
    "llm_gateway_admission_leases",
    "llm_gateway_cache_entries",
    "payment_requests",
    "payment_results",
    "llm_token_usage",
    "memory_records",
    "memory_compactions",
];

struct TestDb {
    admin_pool: PgPool,
    app_pool: PgPool,
    schema: String,
}

#[test]
fn migrations_apply_successfully() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let table_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)::bigint
            FROM information_schema.tables
            WHERE table_schema = $1
              AND table_name = ANY($2)
            "#,
        )
        .bind(&test_db.schema)
        .bind(REQUIRED_TABLES.as_slice())
        .fetch_one(&test_db.app_pool)
        .await?;

        assert_eq!(table_count, REQUIRED_TABLES.len() as i64);
        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_and_step_persists_records() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();

        let run = create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({"transcript_path":"podcasts/ep245/transcript.txt"}),
                requested_capabilities: json!([
                    {"capability":"object.read","scope":"podcasts/*"},
                    {"capability":"object.write","scope":"shownotes/*"}
                ]),
                granted_capabilities: json!([
                    {"capability":"object.read","scope":"podcasts/*"},
                    {"capability":"object.write","scope":"shownotes/*"}
                ]),
                error_json: None,
            },
        )
        .await?;

        let step = create_step(
            &test_db.app_pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "summarize_transcript".to_string(),
                status: "queued".to_string(),
                input_json: json!({"text":"example transcript"}),
                error_json: None,
            },
        )
        .await?;

        assert_eq!(run.id, run_id);
        assert_eq!(step.id, step_id);
        assert_eq!(step.run_id, run_id);

        let persisted_step_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM steps WHERE id = $1 AND run_id = $2")
                .bind(step_id)
                .bind(run_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        assert_eq!(persisted_step_count, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn append_audit_event_persists_event() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "running".to_string(),
                input_json: json!({"transcript_path":"podcasts/ep245/transcript.txt"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;

        create_step(
            &test_db.app_pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "emit_audit".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        let event = append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "skill.invoked".to_string(),
                payload_json: json!({"skill":"summarize_transcript"}),
            },
        )
        .await?;

        let row =
            sqlx::query("SELECT actor, event_type, payload_json FROM audit_events WHERE id = $1")
                .bind(event.id)
                .fetch_one(&test_db.app_pool)
                .await?;

        let actor: String = row.get("actor");
        let event_type: String = row.get("event_type");
        let payload_json: serde_json::Value = row.get("payload_json");

        assert_eq!(actor, "worker");
        assert_eq!(event_type, "skill.invoked");
        assert_eq!(payload_json, json!({"skill":"summarize_transcript"}));

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_audit_plane_routes_high_risk_events() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        create_step(
            &test_db.app_pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "payment".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        let compliance_source = append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "action.executed".to_string(),
                payload_json: {
                    let action_request_id = Uuid::new_v4();
                    let payment_request_id = Uuid::new_v4();
                    json!({
                        "action_type": "payment.send",
                        "destination": "nwc:wallet-main",
                        "request_id": "req-123",
                        "session_id": "sess-abc",
                        "action_request_id": action_request_id,
                        "payment_request_id": payment_request_id,
                    })
                },
            },
        )
        .await?;

        append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "run.claimed".to_string(),
                payload_json: json!({}),
            },
        )
        .await?;

        let events = list_tenant_compliance_audit_events(
            &test_db.app_pool,
            "single",
            Some(run_id),
            None,
            50,
        )
        .await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "action.executed");
        assert_eq!(events[0].source_audit_event_id, compliance_source.id);
        assert_eq!(events[0].tamper_chain_seq, 1);
        assert_eq!(events[0].tamper_prev_hash, None);
        assert_eq!(events[0].tamper_hash.len(), 32);
        assert_eq!(events[0].request_id.as_deref(), Some("req-123"));
        assert_eq!(events[0].session_id.as_deref(), Some("sess-abc"));
        assert!(events[0].action_request_id.is_some());
        assert!(events[0].payment_request_id.is_some());

        let verification =
            verify_tenant_compliance_audit_chain(&test_db.app_pool, "single").await?;
        assert!(verification.verified);
        assert_eq!(verification.checked_events, 1);
        assert_eq!(verification.latest_chain_seq, Some(1));
        assert!(verification.first_invalid_event_id.is_none());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_audit_tamper_verification_detects_payload_mutation(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        create_step(
            &test_db.app_pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "payment".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "action.executed".to_string(),
                payload_json: json!({
                    "action_type": "payment.send",
                    "destination": "nwc:wallet-main",
                }),
            },
        )
        .await?;

        let second = append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "action.executed".to_string(),
                payload_json: json!({
                    "action_type": "payment.send",
                    "destination": "nwc:wallet-backup",
                }),
            },
        )
        .await?;

        let before = verify_tenant_compliance_audit_chain(&test_db.app_pool, "single").await?;
        assert!(before.verified);
        assert_eq!(before.checked_events, 2);

        sqlx::query(
            r#"
            UPDATE compliance_audit_events
            SET payload_json = '{"action_type":"payment.send","destination":"nwc:tampered"}'::jsonb
            WHERE source_audit_event_id = $1
            "#,
        )
        .bind(second.id)
        .execute(&test_db.app_pool)
        .await?;

        let after = verify_tenant_compliance_audit_chain(&test_db.app_pool, "single").await?;
        assert!(!after.verified);
        assert_eq!(after.first_invalid_event_id, Some(second.id));

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_audit_policy_defaults_and_upsert_round_trip() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let defaults = get_tenant_compliance_audit_policy(&test_db.app_pool, "single").await?;
        assert_eq!(defaults.tenant_id, "single");
        assert_eq!(defaults.compliance_hot_retention_days, 180);
        assert_eq!(defaults.compliance_archive_retention_days, 2555);
        assert!(!defaults.legal_hold);

        let updated = upsert_tenant_compliance_audit_policy(
            &test_db.app_pool,
            "single",
            30,
            365,
            true,
            Some("investigation-123"),
        )
        .await?;
        assert_eq!(updated.compliance_hot_retention_days, 30);
        assert_eq!(updated.compliance_archive_retention_days, 365);
        assert!(updated.legal_hold);
        assert_eq!(
            updated.legal_hold_reason.as_deref(),
            Some("investigation-123")
        );

        let fetched = get_tenant_compliance_audit_policy(&test_db.app_pool, "single").await?;
        assert_eq!(fetched.compliance_hot_retention_days, 30);
        assert_eq!(fetched.compliance_archive_retention_days, 365);
        assert!(fetched.legal_hold);
        assert_eq!(
            fetched.legal_hold_reason.as_deref(),
            Some("investigation-123")
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_audit_purge_respects_retention_and_legal_hold(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        create_step(
            &test_db.app_pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "payment".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        let first = append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "action.executed".to_string(),
                payload_json: json!({
                    "action_type": "payment.send",
                    "destination": "nwc:wallet-main",
                }),
            },
        )
        .await?;

        sqlx::query(
            "UPDATE compliance_audit_events SET created_at = now() - interval '200 days' WHERE source_audit_event_id = $1",
        )
        .bind(first.id)
        .execute(&test_db.app_pool)
        .await?;

        let purge = purge_expired_tenant_compliance_audit_events(
            &test_db.app_pool,
            "single",
            OffsetDateTime::now_utc(),
        )
        .await?;
        assert_eq!(purge.deleted_count, 1);
        assert!(!purge.legal_hold);

        let after_first_purge = list_tenant_compliance_audit_events(
            &test_db.app_pool,
            "single",
            Some(run_id),
            None,
            50,
        )
        .await?;
        assert!(after_first_purge.is_empty());

        upsert_tenant_compliance_audit_policy(
            &test_db.app_pool,
            "single",
            30,
            365,
            true,
            Some("investigation-legal-hold"),
        )
        .await?;

        let second = append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "action.executed".to_string(),
                payload_json: json!({
                    "action_type": "payment.send",
                    "destination": "nwc:wallet-hold",
                }),
            },
        )
        .await?;

        sqlx::query(
            "UPDATE compliance_audit_events SET created_at = now() - interval '200 days' WHERE source_audit_event_id = $1",
        )
        .bind(second.id)
        .execute(&test_db.app_pool)
        .await?;

        let purge_with_hold = purge_expired_tenant_compliance_audit_events(
            &test_db.app_pool,
            "single",
            OffsetDateTime::now_utc(),
        )
        .await?;
        assert_eq!(purge_with_hold.deleted_count, 0);
        assert!(purge_with_hold.legal_hold);

        let after_hold_purge = list_tenant_compliance_audit_events(
            &test_db.app_pool,
            "single",
            Some(run_id),
            None,
            50,
        )
        .await?;
        assert_eq!(after_hold_purge.len(), 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_siem_delivery_outbox_claim_and_deliver_round_trip(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let record = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "secureagnt_ndjson".to_string(),
                delivery_target: "mock://success".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"ok\"}\n".to_string(),
                max_attempts: 3,
            },
        )
        .await?;
        assert_eq!(record.status, "pending");
        assert_eq!(record.attempts, 0);

        let pending_rows = list_tenant_compliance_siem_delivery_records(
            &test_db.app_pool,
            "single",
            None,
            Some("pending"),
            20,
        )
        .await?;
        assert_eq!(pending_rows.len(), 1);
        assert_eq!(pending_rows[0].id, record.id);

        let claimed = claim_pending_compliance_siem_delivery_records(
            &test_db.app_pool,
            "worker-a",
            Duration::from_secs(30),
            10,
        )
        .await?;
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, record.id);
        assert_eq!(claimed[0].status, "processing");
        assert_eq!(claimed[0].leased_by.as_deref(), Some("worker-a"));
        assert!(claimed[0].lease_expires_at.is_some());

        let delivered =
            mark_compliance_siem_delivery_record_delivered(&test_db.app_pool, record.id, Some(200))
                .await?;
        assert_eq!(delivered.status, "delivered");
        assert_eq!(delivered.attempts, 1);
        assert_eq!(delivered.last_http_status, Some(200));
        assert!(delivered.last_error.is_none());
        assert!(delivered.delivered_at.is_some());

        let delivered_rows = list_tenant_compliance_siem_delivery_records(
            &test_db.app_pool,
            "single",
            None,
            Some("delivered"),
            20,
        )
        .await?;
        assert_eq!(delivered_rows.len(), 1);
        assert_eq!(delivered_rows[0].id, record.id);

        let claim_again = claim_pending_compliance_siem_delivery_records(
            &test_db.app_pool,
            "worker-b",
            Duration::from_secs(30),
            10,
        )
        .await?;
        assert!(claim_again.is_empty());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_siem_delivery_outbox_dead_letters_on_max_attempts(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let record = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "splunk_hec".to_string(),
                delivery_target: "mock://fail".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"fail\"}\n".to_string(),
                max_attempts: 1,
            },
        )
        .await?;

        let claimed = claim_pending_compliance_siem_delivery_records(
            &test_db.app_pool,
            "worker-a",
            Duration::from_secs(30),
            10,
        )
        .await?;
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, record.id);

        let failed = mark_compliance_siem_delivery_record_failed(
            &test_db.app_pool,
            record.id,
            "simulated delivery failure",
            Some(503),
            OffsetDateTime::now_utc() + time::Duration::seconds(30),
        )
        .await?;
        assert_eq!(failed.status, "dead_lettered");
        assert_eq!(failed.attempts, 1);
        assert_eq!(failed.last_http_status, Some(503));
        assert_eq!(
            failed.last_error.as_deref(),
            Some("simulated delivery failure")
        );

        let claim_again = claim_pending_compliance_siem_delivery_records(
            &test_db.app_pool,
            "worker-b",
            Duration::from_secs(30),
            10,
        )
        .await?;
        assert!(claim_again.is_empty());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_siem_delivery_outbox_can_be_force_dead_lettered(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let record = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "splunk_hec".to_string(),
                delivery_target: "https://siem.example/hec".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"fail\"}\n".to_string(),
                max_attempts: 5,
            },
        )
        .await?;

        let claimed = claim_pending_compliance_siem_delivery_records(
            &test_db.app_pool,
            "worker-a",
            Duration::from_secs(30),
            10,
        )
        .await?;
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, record.id);

        let dead_lettered = mark_compliance_siem_delivery_record_dead_lettered(
            &test_db.app_pool,
            record.id,
            "non-retryable delivery failure",
            Some(401),
        )
        .await?;
        assert_eq!(dead_lettered.status, "dead_lettered");
        assert_eq!(dead_lettered.attempts, 1);
        assert_eq!(dead_lettered.last_http_status, Some(401));
        assert_eq!(
            dead_lettered.last_error.as_deref(),
            Some("non-retryable delivery failure")
        );

        let claim_again = claim_pending_compliance_siem_delivery_records(
            &test_db.app_pool,
            "worker-b",
            Duration::from_secs(30),
            10,
        )
        .await?;
        assert!(claim_again.is_empty());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_siem_delivery_summary_counts_statuses() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let _pending = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "secureagnt_ndjson".to_string(),
                delivery_target: "mock://pending".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"pending\"}\n".to_string(),
                max_attempts: 3,
            },
        )
        .await?;
        let processing = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "secureagnt_ndjson".to_string(),
                delivery_target: "mock://processing".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"processing\"}\n".to_string(),
                max_attempts: 3,
            },
        )
        .await?;
        let delivered = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "secureagnt_ndjson".to_string(),
                delivery_target: "mock://delivered".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"delivered\"}\n".to_string(),
                max_attempts: 3,
            },
        )
        .await?;

        sqlx::query(
            "UPDATE compliance_siem_delivery_outbox SET status = 'processing' WHERE id = $1",
        )
        .bind(processing.id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query("UPDATE compliance_siem_delivery_outbox SET status = 'failed' WHERE id = $1")
            .bind(delivered.id)
            .execute(&test_db.app_pool)
            .await?;
        sqlx::query(
            "UPDATE compliance_siem_delivery_outbox SET status = 'delivered' WHERE id = $1",
        )
        .bind(delivered.id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "UPDATE compliance_siem_delivery_outbox SET status = 'dead_lettered' WHERE id = $1",
        )
        .bind(processing.id)
        .execute(&test_db.app_pool)
        .await?;

        let summary =
            get_tenant_compliance_siem_delivery_summary(&test_db.app_pool, "single", None).await?;
        assert_eq!(summary.pending_count, 1);
        assert_eq!(summary.processing_count, 0);
        assert_eq!(summary.failed_count, 0);
        assert_eq!(summary.delivered_count, 1);
        assert_eq!(summary.dead_lettered_count, 1);
        assert!(summary.oldest_pending_age_seconds.is_some());

        let other_summary =
            get_tenant_compliance_siem_delivery_summary(&test_db.app_pool, "other", None).await?;
        assert_eq!(other_summary.pending_count, 0);
        assert_eq!(other_summary.delivered_count, 0);
        assert!(other_summary.oldest_pending_age_seconds.is_none());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_siem_delivery_slo_reports_rates() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        for (status, target) in [
            ("pending", "mock://pending"),
            ("processing", "mock://processing"),
            ("delivered", "mock://delivered"),
            ("dead_lettered", "mock://dead-lettered"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO compliance_siem_delivery_outbox (
                    id, tenant_id, run_id, adapter, delivery_target, content_type, payload_ndjson,
                    status, attempts, max_attempts, next_attempt_at, created_at, updated_at
                )
                VALUES (
                    $1, 'single', NULL, 'secureagnt_ndjson', $2, 'application/x-ndjson', '{"event":"x"}',
                    $3, 0, 3, now(), now() - interval '10 seconds', now()
                )
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(target)
            .bind(status)
            .execute(&test_db.app_pool)
            .await?;
        }

        let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let slo = get_tenant_compliance_siem_delivery_slo(&test_db.app_pool, "single", None, since)
            .await?;
        assert_eq!(slo.total_count, 4);
        assert_eq!(slo.pending_count, 1);
        assert_eq!(slo.processing_count, 1);
        assert_eq!(slo.delivered_count, 1);
        assert_eq!(slo.dead_lettered_count, 1);
        assert_eq!(slo.failed_count, 0);
        assert_eq!(slo.delivery_success_rate_pct, Some(25.0));
        assert_eq!(slo.hard_failure_rate_pct, Some(25.0));
        assert_eq!(slo.dead_letter_rate_pct, Some(25.0));
        assert!(slo.oldest_pending_age_seconds.is_some());

        let empty =
            get_tenant_compliance_siem_delivery_slo(&test_db.app_pool, "other", None, since)
                .await?;
        assert_eq!(empty.total_count, 0);
        assert!(empty.delivery_success_rate_pct.is_none());
        assert!(empty.hard_failure_rate_pct.is_none());
        assert!(empty.dead_letter_rate_pct.is_none());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_siem_delivery_target_summaries_and_dead_letter_replay(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let dead_letter = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "splunk_hec".to_string(),
                delivery_target: "https://siem.example/hec".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"a\"}\n".to_string(),
                max_attempts: 1,
            },
        )
        .await?;
        let _ = mark_compliance_siem_delivery_record_failed(
            &test_db.app_pool,
            dead_letter.id,
            "auth denied",
            Some(401),
            OffsetDateTime::now_utc(),
        )
        .await?;

        let delivered = create_compliance_siem_delivery_record(
            &test_db.app_pool,
            &NewComplianceSiemDeliveryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                run_id: None,
                adapter: "splunk_hec".to_string(),
                delivery_target: "https://siem.example/hec".to_string(),
                content_type: "application/x-ndjson".to_string(),
                payload_ndjson: "{\"event\":\"b\"}\n".to_string(),
                max_attempts: 3,
            },
        )
        .await?;
        let _ = mark_compliance_siem_delivery_record_delivered(
            &test_db.app_pool,
            delivered.id,
            Some(200),
        )
        .await?;

        let target_rows = list_tenant_compliance_siem_delivery_target_summaries(
            &test_db.app_pool,
            "single",
            None,
            None,
            20,
        )
        .await?;
        assert_eq!(target_rows.len(), 1);
        assert_eq!(target_rows[0].delivery_target, "https://siem.example/hec");
        assert!(target_rows[0].total_count >= 2);
        assert!(target_rows[0].dead_lettered_count >= 1);
        assert!(target_rows[0].last_error.is_some());

        let replayed = requeue_dead_letter_compliance_siem_delivery_record(
            &test_db.app_pool,
            "single",
            dead_letter.id,
            OffsetDateTime::now_utc(),
        )
        .await?
        .ok_or("expected replay row")?;
        assert_eq!(replayed.status, "pending");
        assert_eq!(replayed.attempts, 0);
        assert!(replayed.last_error.is_none());
        assert!(replayed.last_http_status.is_none());

        let replay_other = requeue_dead_letter_compliance_siem_delivery_record(
            &test_db.app_pool,
            "other",
            dead_letter.id,
            OffsetDateTime::now_utc(),
        )
        .await?;
        assert!(replay_other.is_none());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_run_status_and_list_audit_events_are_tenant_scoped() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "queued"),
        )
        .await?;

        append_audit_event(
            &test_db.app_pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: None,
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "api".to_string(),
                event_type: "run.created".to_string(),
                payload_json: json!({"recipe_id":"show_notes_v1"}),
            },
        )
        .await?;

        let run = get_run_status(&test_db.app_pool, "single", run_id)
            .await?
            .expect("run should exist for tenant");
        assert_eq!(run.id, run_id);
        assert_eq!(run.status, "queued");

        let events = list_run_audit_events(&test_db.app_pool, "single", run_id, 100).await?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "run.created");

        let run_missing = get_run_status(&test_db.app_pool, "other", run_id).await?;
        assert!(run_missing.is_none());

        let other_events = list_run_audit_events(&test_db.app_pool, "other", run_id, 100).await?;
        assert!(other_events.is_empty());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn tenant_run_latency_histogram_and_ops_summary_reflect_duration_windows(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        for (run_id, duration_ms) in [
            (Uuid::new_v4(), 300_i64),
            (Uuid::new_v4(), 800_i64),
            (Uuid::new_v4(), 1_500_i64),
            (Uuid::new_v4(), 3_200_i64),
            (Uuid::new_v4(), 7_500_i64),
            (Uuid::new_v4(), 12_000_i64),
        ] {
            create_run(
                &test_db.app_pool,
                &new_run(run_id, agent_id, user_id, "succeeded"),
            )
            .await?;
            sqlx::query(
                r#"
                UPDATE runs
                SET started_at = now() - (($2::bigint + 1000) * interval '1 millisecond'),
                    finished_at = now() - (1000 * interval '1 millisecond'),
                    status = 'succeeded'
                WHERE id = $1
                "#,
            )
            .bind(run_id)
            .bind(duration_ms)
            .execute(&test_db.app_pool)
            .await?;
        }

        let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let histogram =
            get_tenant_run_latency_histogram(&test_db.app_pool, "single", since).await?;
        assert_eq!(histogram.len(), 6);
        let counts: Vec<i64> = histogram.iter().map(|bucket| bucket.run_count).collect();
        assert_eq!(counts, vec![1, 1, 1, 1, 1, 1]);

        let summary = get_tenant_ops_summary(&test_db.app_pool, "single", since).await?;
        assert_eq!(summary.succeeded_runs_window, 6);
        assert!(summary.avg_run_duration_ms.is_some());
        assert!(summary.p95_run_duration_ms.is_some());

        let traces = get_tenant_run_latency_traces(&test_db.app_pool, "single", since, 3).await?;
        assert_eq!(traces.len(), 3);
        assert!(traces[0].finished_at >= traces[1].finished_at);
        assert!(traces[1].finished_at >= traces[2].finished_at);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn tenant_action_latency_summary_is_tenant_scoped_and_reports_status_mix(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "running"),
        )
        .await?;

        let step = create_step(
            &test_db.app_pool,
            &NewStep {
                id: Uuid::new_v4(),
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "ops_action_metrics".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        let action_specs = vec![
            ("message.send", "executed", "executed", 120_i64),
            ("payment.send", "failed", "failed", 900_i64),
            ("payment.send", "denied", "denied", 80_i64),
        ];

        for (action_type, request_status, result_status, duration_ms) in action_specs {
            let action_request_id = Uuid::new_v4();
            create_action_request(
                &test_db.app_pool,
                &NewActionRequest {
                    id: action_request_id,
                    step_id: step.id,
                    action_type: action_type.to_string(),
                    args_json: json!({"destination":"test"}),
                    justification: Some("integration".to_string()),
                    status: request_status.to_string(),
                    decision_reason: None,
                },
            )
            .await?;
            create_action_result(
                &test_db.app_pool,
                &NewActionResult {
                    id: Uuid::new_v4(),
                    action_request_id,
                    status: result_status.to_string(),
                    result_json: Some(json!({})),
                    error_json: None,
                },
            )
            .await?;

            sqlx::query("UPDATE action_requests SET created_at = now() - interval '5 minutes' WHERE id = $1")
                .bind(action_request_id)
                .execute(&test_db.app_pool)
                .await?;
            sqlx::query(
                r#"
                UPDATE action_results
                SET executed_at = (
                    SELECT created_at + ($2::bigint * interval '1 millisecond')
                    FROM action_requests
                    WHERE id = $1
                )
                WHERE action_request_id = $1
                "#,
            )
            .bind(action_request_id)
            .bind(duration_ms)
            .execute(&test_db.app_pool)
            .await?;
        }

        let other_agent = Uuid::new_v4();
        let other_user = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'other', 'other_agent', 'active')",
        )
        .bind(other_agent)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO users (id, tenant_id, external_subject, display_name, status) VALUES ($1, 'other', 'tenant:other:user', 'Other User', 'active')",
        )
        .bind(other_user)
        .execute(&test_db.app_pool)
        .await?;
        let other_run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &NewRun {
                id: other_run_id,
                tenant_id: "other".to_string(),
                agent_id: other_agent,
                triggered_by_user_id: Some(other_user),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "running".to_string(),
                error_json: None,
            },
        )
        .await?;

        let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let rows = get_tenant_action_latency_summary(&test_db.app_pool, "single", since).await?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action_type, "payment.send");
        assert_eq!(rows[0].total_count, 2);
        assert_eq!(rows[0].failed_count, 1);
        assert_eq!(rows[0].denied_count, 1);
        assert!(rows[0].p95_duration_ms.is_some());
        assert_eq!(rows[1].action_type, "message.send");
        assert_eq!(rows[1].total_count, 1);
        assert_eq!(rows[1].failed_count, 0);
        assert_eq!(rows[1].denied_count, 0);

        let other_rows =
            get_tenant_action_latency_summary(&test_db.app_pool, "other", since).await?;
        assert!(other_rows.is_empty());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn tenant_action_latency_traces_are_filtered_and_tenant_scoped(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "running"),
        )
        .await?;

        let step = create_step(
            &test_db.app_pool,
            &NewStep {
                id: Uuid::new_v4(),
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "ops_action_traces".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        for (action_type, status, duration_ms, age_seconds) in [
            ("message.send", "executed", 140_i64, 600_i64),
            ("payment.send", "failed", 760_i64, 500_i64),
            ("payment.send", "denied", 90_i64, 400_i64),
        ] {
            let action_request_id = Uuid::new_v4();
            create_action_request(
                &test_db.app_pool,
                &NewActionRequest {
                    id: action_request_id,
                    step_id: step.id,
                    action_type: action_type.to_string(),
                    args_json: json!({"destination":"test"}),
                    justification: Some("integration".to_string()),
                    status: status.to_string(),
                    decision_reason: None,
                },
            )
            .await?;
            create_action_result(
                &test_db.app_pool,
                &NewActionResult {
                    id: Uuid::new_v4(),
                    action_request_id,
                    status: status.to_string(),
                    result_json: Some(json!({})),
                    error_json: None,
                },
            )
            .await?;
            sqlx::query(
                "UPDATE action_requests SET created_at = now() - ($2::bigint * interval '1 second') WHERE id = $1",
            )
            .bind(action_request_id)
            .bind(age_seconds)
            .execute(&test_db.app_pool)
            .await?;
            sqlx::query(
                r#"
                UPDATE action_results
                SET executed_at = (
                    SELECT created_at + ($2::bigint * interval '1 millisecond')
                    FROM action_requests
                    WHERE id = $1
                )
                WHERE action_request_id = $1
                "#,
            )
            .bind(action_request_id)
            .bind(duration_ms)
            .execute(&test_db.app_pool)
            .await?;
        }

        let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let traces = get_tenant_action_latency_traces(
            &test_db.app_pool,
            "single",
            since,
            Some("payment.send"),
            10,
        )
        .await?;
        assert_eq!(traces.len(), 2);
        assert_eq!(traces[0].action_type, "payment.send");
        assert_eq!(traces[1].action_type, "payment.send");
        assert!(traces[0].created_at >= traces[1].created_at);
        assert!(traces.iter().all(|trace| trace.duration_ms >= 0));

        let other_traces =
            get_tenant_action_latency_traces(&test_db.app_pool, "other", since, None, 10).await?;
        assert!(other_traces.is_empty());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn count_tenant_triggers_is_tenant_scoped() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let single_trigger = Uuid::new_v4();
        create_interval_trigger(
            &test_db.app_pool,
            &NewIntervalTrigger {
                id: single_trigger,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                interval_seconds: 60,
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                next_fire_at: OffsetDateTime::now_utc() + time::Duration::seconds(60),
                status: "enabled".to_string(),
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let other_agent_id = Uuid::new_v4();
        let other_user_id = Uuid::new_v4();
        sqlx::query("INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'other', 'tenant_other_agent', 'active')")
            .bind(other_agent_id)
            .execute(&test_db.app_pool)
            .await?;
        sqlx::query("INSERT INTO users (id, tenant_id, external_subject, display_name, status) VALUES ($1, 'other', 'tenant:other:user', 'Other User', 'active')")
            .bind(other_user_id)
            .execute(&test_db.app_pool)
            .await?;
        create_interval_trigger(
            &test_db.app_pool,
            &NewIntervalTrigger {
                id: Uuid::new_v4(),
                tenant_id: "other".to_string(),
                agent_id: other_agent_id,
                triggered_by_user_id: Some(other_user_id),
                recipe_id: "show_notes_v1".to_string(),
                interval_seconds: 120,
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                next_fire_at: OffsetDateTime::now_utc() + time::Duration::seconds(120),
                status: "enabled".to_string(),
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let single_count = count_tenant_triggers(&test_db.app_pool, "single").await?;
        let other_count = count_tenant_triggers(&test_db.app_pool, "other").await?;
        assert_eq!(single_count, 1);
        assert_eq!(other_count, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn step_and_action_records_persist_and_transition() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        let action_request_id = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "running"),
        )
        .await?;

        create_step(
            &test_db.app_pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "summarize_transcript".to_string(),
                status: "running".to_string(),
                input_json: json!({"text":"example transcript"}),
                error_json: None,
            },
        )
        .await?;

        create_action_request(
            &test_db.app_pool,
            &NewActionRequest {
                id: action_request_id,
                step_id,
                action_type: "object.write".to_string(),
                args_json: json!({"path":"shownotes/ep245.md","content":"# Summary"}),
                justification: Some("Persist generated notes".to_string()),
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        let updated =
            update_action_request_status(&test_db.app_pool, action_request_id, "executed", None)
                .await?;
        assert!(updated);

        create_action_result(
            &test_db.app_pool,
            &NewActionResult {
                id: Uuid::new_v4(),
                action_request_id,
                status: "executed".to_string(),
                result_json: Some(json!({"path":"shownotes/ep245.md"})),
                error_json: None,
            },
        )
        .await?;

        let step_completed =
            mark_step_succeeded(&test_db.app_pool, step_id, json!({"markdown":"# Summary"}))
                .await?;
        assert!(step_completed);

        let step_status: String = sqlx::query_scalar("SELECT status FROM steps WHERE id = $1")
            .bind(step_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(step_status, "succeeded");

        let action_status: String =
            sqlx::query_scalar("SELECT status FROM action_requests WHERE id = $1")
                .bind(action_request_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        assert_eq!(action_status, "executed");

        let result_status: String =
            sqlx::query_scalar("SELECT status FROM action_results WHERE action_request_id = $1")
                .bind(action_request_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        assert_eq!(result_status, "executed");

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn claim_next_queued_run_claims_oldest_and_sets_lease() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let first_run_id = Uuid::new_v4();
        let second_run_id = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &new_run(first_run_id, agent_id, user_id, "queued"),
        )
        .await?;
        create_run(
            &test_db.app_pool,
            &new_run(second_run_id, agent_id, user_id, "queued"),
        )
        .await?;

        let claimed = claim_next_queued_run(&test_db.app_pool, "worker-a", Duration::from_secs(30))
            .await?
            .expect("expected queued run");

        assert_eq!(claimed.id, first_run_id);
        assert_eq!(claimed.status, "running");
        assert_eq!(claimed.attempts, 1);
        assert_eq!(claimed.lease_owner.as_deref(), Some("worker-a"));
        assert!(claimed.lease_expires_at.is_some());

        let first_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(first_run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        let second_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(second_run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(first_status, "running");
        assert_eq!(second_status, "queued");

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn claim_next_queued_run_with_limits_respects_global_cap() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let first_run_id = Uuid::new_v4();
        let second_run_id = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &new_run(first_run_id, agent_id, user_id, "queued"),
        )
        .await?;
        create_run(
            &test_db.app_pool,
            &new_run(second_run_id, agent_id, user_id, "queued"),
        )
        .await?;

        let claimed = claim_next_queued_run_with_limits(
            &test_db.app_pool,
            "worker-a",
            Duration::from_secs(30),
            1,
            1000,
        )
        .await?
        .expect("expected one queued run");
        assert_eq!(claimed.id, first_run_id);

        let blocked = claim_next_queued_run_with_limits(
            &test_db.app_pool,
            "worker-a",
            Duration::from_secs(30),
            1,
            1000,
        )
        .await?;
        assert!(blocked.is_none());

        let second_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(second_run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(second_status, "queued");

        let claimed_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status = 'running'")
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(claimed_count, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn claim_next_queued_run_with_limits_respects_tenant_fairness() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let tenant_a = "tenant_a";
        let tenant_b = "tenant_b";
        let (tenant_a_agent_id, tenant_a_user_id) =
            seed_agent_and_user_for_tenant(&test_db.app_pool, tenant_a).await?;
        let (tenant_b_agent_id, tenant_b_user_id) =
            seed_agent_and_user_for_tenant(&test_db.app_pool, tenant_b).await?;

        let tenant_a_oldest = Uuid::new_v4();
        let tenant_a_newer = Uuid::new_v4();
        let tenant_b_new = Uuid::new_v4();

        create_run(
            &test_db.app_pool,
            &NewRun {
                id: tenant_a_oldest,
                tenant_id: tenant_a.to_string(),
                agent_id: tenant_a_agent_id,
                triggered_by_user_id: Some(tenant_a_user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({"transcript_path":"podcasts/tenant_a/oldest.txt"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        create_run(
            &test_db.app_pool,
            &NewRun {
                id: tenant_a_newer,
                tenant_id: tenant_a.to_string(),
                agent_id: tenant_a_agent_id,
                triggered_by_user_id: Some(tenant_a_user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({"transcript_path":"podcasts/tenant_a/newer.txt"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        create_run(
            &test_db.app_pool,
            &NewRun {
                id: tenant_b_new,
                tenant_id: tenant_b.to_string(),
                agent_id: tenant_b_agent_id,
                triggered_by_user_id: Some(tenant_b_user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({"transcript_path":"podcasts/tenant_b/older.txt"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;

        let query = "UPDATE runs SET created_at = now() - ($2::bigint * interval '1 second') WHERE id = $1";
        sqlx::query(query)
            .bind(tenant_a_oldest)
            .bind(600_i64)
            .execute(&test_db.app_pool)
            .await?;
        sqlx::query(query)
            .bind(tenant_a_newer)
            .bind(540_i64)
            .execute(&test_db.app_pool)
            .await?;
        sqlx::query(query)
            .bind(tenant_b_new)
            .bind(60_i64)
            .execute(&test_db.app_pool)
            .await?;

        let claimed = claim_next_queued_run(&test_db.app_pool, "worker-a", Duration::from_secs(30))
            .await?
            .expect("expected tenant-a oldest run to be claimed");
        assert_eq!(claimed.id, tenant_a_oldest);
        assert_eq!(claimed.tenant_id, tenant_a);

        let next = claim_next_queued_run_with_limits(
            &test_db.app_pool,
            "worker-a",
            Duration::from_secs(30),
            1000,
            1,
        )
        .await?
        .expect("expected tenant-b run to be claimed on tenant cap fallback");
        assert_eq!(next.id, tenant_b_new);
        assert_eq!(next.tenant_id, tenant_b);

        let blocked = claim_next_queued_run_with_limits(
            &test_db.app_pool,
            "worker-a",
            Duration::from_secs(30),
            1000,
            1,
        )
        .await?;
        assert!(blocked.is_none());

        let claimed_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status = 'running'")
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(claimed_count, 2);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn claim_next_queued_run_prioritizes_interactive_over_batch(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let batch_run_id = Uuid::new_v4();
        let interactive_run_id = Uuid::new_v4();

        let mut batch = new_run(batch_run_id, agent_id, user_id, "queued");
        batch.input_json = json!({
            "queue_class":"batch",
            "transcript_path":"podcasts/ep245/transcript.txt"
        });
        create_run(&test_db.app_pool, &batch).await?;

        let mut interactive = new_run(interactive_run_id, agent_id, user_id, "queued");
        interactive.input_json = json!({
            "queue_class":"interactive",
            "transcript_path":"podcasts/ep245/transcript.txt"
        });
        create_run(&test_db.app_pool, &interactive).await?;

        let claimed = claim_next_queued_run(&test_db.app_pool, "worker-a", Duration::from_secs(30))
            .await?
            .expect("expected queued run");
        assert_eq!(claimed.id, interactive_run_id);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn renew_and_complete_run_lease_flow() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "queued"),
        )
        .await?;

        let claimed = claim_next_queued_run(&test_db.app_pool, "worker-a", Duration::from_secs(5))
            .await?
            .expect("run should be claimed");
        let old_lease = claimed.lease_expires_at.expect("lease expiry");

        let renewed = renew_run_lease(
            &test_db.app_pool,
            run_id,
            "worker-a",
            Duration::from_secs(20),
        )
        .await?;
        assert!(renewed);

        let new_lease: Option<time::OffsetDateTime> =
            sqlx::query_scalar("SELECT lease_expires_at FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        let new_lease = new_lease.expect("renewed lease expiry");
        assert!(new_lease > old_lease);

        let completed = mark_run_succeeded(&test_db.app_pool, run_id, "worker-a").await?;
        assert!(completed);

        let row = sqlx::query(
            "SELECT status, lease_owner, lease_expires_at, finished_at FROM runs WHERE id = $1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        let status: String = row.get("status");
        let lease_owner: Option<String> = row.get("lease_owner");
        let lease_expires_at: Option<time::OffsetDateTime> = row.get("lease_expires_at");
        let finished_at: Option<time::OffsetDateTime> = row.get("finished_at");

        assert_eq!(status, "succeeded");
        assert!(lease_owner.is_none());
        assert!(lease_expires_at.is_none());
        assert!(finished_at.is_some());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn requeue_expired_runs_moves_run_back_to_queue() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "running"),
        )
        .await?;

        sqlx::query(
            r#"
            UPDATE runs
            SET lease_owner = 'worker-stale',
                lease_expires_at = now() - interval '10 seconds'
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .execute(&test_db.app_pool)
        .await?;

        let requeued = requeue_expired_runs(&test_db.app_pool, 10).await?;
        assert_eq!(requeued, 1);

        let row =
            sqlx::query("SELECT status, lease_owner, lease_expires_at FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        let status: String = row.get("status");
        let lease_owner: Option<String> = row.get("lease_owner");
        let lease_expires_at: Option<time::OffsetDateTime> = row.get("lease_expires_at");

        assert_eq!(status, "queued");
        assert!(lease_owner.is_none());
        assert!(lease_expires_at.is_none());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn dispatch_next_due_interval_trigger_creates_run_and_updates_schedule(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        let before_next_fire = time::OffsetDateTime::now_utc() - time::Duration::seconds(1);
        create_interval_trigger(
            &test_db.app_pool,
            &NewIntervalTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                interval_seconds: 60,
                input_json: json!({"transcript_path":"podcasts/ep245/transcript.txt"}),
                requested_capabilities: json!([
                    {"capability":"object.read","scope":"podcasts/*"}
                ]),
                granted_capabilities: json!([
                    {"capability":"object.read","scope":"podcasts/*"}
                ]),
                next_fire_at: before_next_fire,
                status: "enabled".to_string(),
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let dispatched = dispatch_next_due_interval_trigger(&test_db.app_pool)
            .await?
            .expect("due trigger should dispatch");
        assert_eq!(dispatched.trigger_id, trigger_id);
        assert_eq!(dispatched.tenant_id, "single");
        assert_eq!(dispatched.recipe_id, "show_notes_v1");
        assert!(dispatched.next_fire_at > dispatched.scheduled_for);

        let run_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(dispatched.run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(run_status, "queued");

        let trigger_runs: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM trigger_runs WHERE trigger_id = $1")
                .bind(trigger_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        assert_eq!(trigger_runs, 1);

        let none = dispatch_next_due_interval_trigger(&test_db.app_pool).await?;
        assert!(none.is_none());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn dispatch_next_due_interval_trigger_skips_misfire_when_configured(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        create_interval_trigger(
            &test_db.app_pool,
            &NewIntervalTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                interval_seconds: 60,
                input_json: json!({"transcript_path":"podcasts/ep245/transcript.txt"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                next_fire_at: time::OffsetDateTime::now_utc() - time::Duration::minutes(5),
                status: "enabled".to_string(),
                misfire_policy: "skip".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let dispatched = dispatch_next_due_interval_trigger(&test_db.app_pool).await?;
        assert!(dispatched.is_none());

        let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM runs")
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(run_count, 0);

        let failed_ledger_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM trigger_runs WHERE trigger_id = $1 AND status = 'failed'",
        )
        .bind(trigger_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(failed_ledger_rows, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn enqueue_and_dispatch_webhook_trigger_event_creates_run() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        create_webhook_trigger(
            &test_db.app_pool,
            &NewWebhookTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"request_write": false}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "enabled".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let enqueue = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-1",
            json!({"kind":"test"}),
        )
        .await?;
        assert_eq!(enqueue, TriggerEventEnqueueOutcome::Enqueued);

        let duplicate = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-1",
            json!({"kind":"test"}),
        )
        .await?;
        assert_eq!(duplicate, TriggerEventEnqueueOutcome::Duplicate);

        let dispatched = dispatch_next_due_trigger(&test_db.app_pool)
            .await?
            .expect("webhook event should dispatch");
        assert_eq!(dispatched.trigger_id, trigger_id);
        assert_eq!(dispatched.trigger_type, "webhook");
        assert_eq!(dispatched.trigger_event_id.as_deref(), Some("evt-1"));

        let processed_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM trigger_events WHERE trigger_id = $1 AND status = 'processed'",
        )
        .bind(trigger_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(processed_events, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn enqueue_trigger_event_dedupes_by_payload_regardless_of_event_id() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        create_webhook_trigger(
            &test_db.app_pool,
            &NewWebhookTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"request_write": false}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "enabled".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let first = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-1",
            json!({"kind":"webhook","value":{"b":2,"a":1}}),
        )
        .await?;
        assert_eq!(first, TriggerEventEnqueueOutcome::Enqueued);

        let duplicate = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-2",
            json!({"kind":"webhook","value":{"a":1,"b":2}}),
        )
        .await?;
        assert_eq!(duplicate, TriggerEventEnqueueOutcome::Duplicate);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn enqueue_trigger_event_returns_unavailable_reasons_for_non_dispatchable_triggers(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let missing = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            Uuid::new_v4(),
            "evt-missing",
            json!({"kind":"test"}),
        )
        .await?;
        assert_eq!(
            missing,
            TriggerEventEnqueueOutcome::TriggerUnavailable {
                reason: TriggerEventEnqueueUnavailableReason::TriggerNotFound
            }
        );

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;

        let disabled_trigger_id = Uuid::new_v4();
        create_webhook_trigger(
            &test_db.app_pool,
            &NewWebhookTrigger {
                id: disabled_trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"request_write": false}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "disabled".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;
        let disabled = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            disabled_trigger_id,
            "evt-disabled",
            json!({"kind":"test"}),
        )
        .await?;
        assert_eq!(
            disabled,
            TriggerEventEnqueueOutcome::TriggerUnavailable {
                reason: TriggerEventEnqueueUnavailableReason::TriggerDisabled
            }
        );

        let interval_trigger_id = Uuid::new_v4();
        create_interval_trigger(
            &test_db.app_pool,
            &NewIntervalTrigger {
                id: interval_trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"request_write": false}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                next_fire_at: OffsetDateTime::now_utc(),
                status: "enabled".to_string(),
                interval_seconds: 60,
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;
        let wrong_type = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            interval_trigger_id,
            "evt-interval",
            json!({"kind":"test"}),
        )
        .await?;
        assert_eq!(
            wrong_type,
            TriggerEventEnqueueOutcome::TriggerUnavailable {
                reason: TriggerEventEnqueueUnavailableReason::TriggerTypeMismatch
            }
        );

        let schedule_broken_trigger_id = Uuid::new_v4();
        create_webhook_trigger(
            &test_db.app_pool,
            &NewWebhookTrigger {
                id: schedule_broken_trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"request_write": false}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "enabled".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;
        sqlx::query(
            "UPDATE triggers SET dead_lettered_at = now(), dead_letter_reason = 'schedule broken' WHERE id = $1",
        )
        .bind(schedule_broken_trigger_id)
        .execute(&test_db.app_pool)
        .await?;
        let schedule_broken = enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            schedule_broken_trigger_id,
            "evt-broken",
            json!({"kind":"test"}),
        )
        .await?;
        assert_eq!(
            schedule_broken,
            TriggerEventEnqueueOutcome::TriggerUnavailable {
                reason: TriggerEventEnqueueUnavailableReason::TriggerScheduleBroken
            }
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn requeue_dead_letter_trigger_event_resets_event_for_replay(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        create_webhook_trigger(
            &test_db.app_pool,
            &NewWebhookTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"request_write": false}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "enabled".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-replay-1",
            json!({"kind":"test"}),
        )
        .await?;

        sqlx::query(
            r#"
            UPDATE trigger_events
            SET status = 'dead_lettered',
                attempts = 3,
                next_attempt_at = now() + interval '5 minutes',
                last_error_json = '{"code":"TEST"}'::jsonb,
                dead_lettered_at = now()
            WHERE trigger_id = $1
              AND event_id = $2
            "#,
        )
        .bind(trigger_id)
        .bind("evt-replay-1")
        .execute(&test_db.app_pool)
        .await?;

        let replay = requeue_dead_letter_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-replay-1",
        )
        .await?;
        assert_eq!(replay, TriggerEventReplayOutcome::Requeued);

        let row = sqlx::query(
            r#"
            SELECT status, attempts, last_error_json, dead_lettered_at
            FROM trigger_events
            WHERE trigger_id = $1
              AND event_id = $2
            "#,
        )
        .bind(trigger_id)
        .bind("evt-replay-1")
        .fetch_one(&test_db.app_pool)
        .await?;
        let status: String = row.get("status");
        let attempts: i32 = row.get("attempts");
        let last_error_json: Option<serde_json::Value> = row.get("last_error_json");
        let dead_lettered_at: Option<time::OffsetDateTime> = row.get("dead_lettered_at");
        assert_eq!(status, "pending");
        assert_eq!(attempts, 0);
        assert!(last_error_json.is_none());
        assert!(dead_lettered_at.is_none());

        let replay_again = requeue_dead_letter_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-replay-1",
        )
        .await?;
        assert_eq!(
            replay_again,
            TriggerEventReplayOutcome::NotDeadLettered {
                status: "pending".to_string()
            }
        );

        let missing = requeue_dead_letter_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-missing",
        )
        .await?;
        assert_eq!(missing, TriggerEventReplayOutcome::NotFound);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn fire_trigger_manually_creates_run_and_dedupes_by_idempotency_key(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        create_interval_trigger(
            &test_db.app_pool,
            &NewIntervalTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                interval_seconds: 60,
                input_json: json!({"origin":"manual-test"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                next_fire_at: time::OffsetDateTime::now_utc() + time::Duration::minutes(5),
                status: "enabled".to_string(),
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let first =
            fire_trigger_manually(&test_db.app_pool, "single", trigger_id, "manual-001", None)
                .await?;
        let created_run_id = match first {
            ManualTriggerFireOutcome::Created(dispatched) => dispatched.run_id,
            other => panic!("unexpected outcome: {other:?}"),
        };

        let second =
            fire_trigger_manually(&test_db.app_pool, "single", trigger_id, "manual-001", None)
                .await?;
        match second {
            ManualTriggerFireOutcome::Duplicate { run_id } => {
                assert_eq!(run_id, Some(created_run_id));
            }
            other => panic!("unexpected outcome: {other:?}"),
        }

        let run_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM runs WHERE tenant_id = 'single' AND recipe_id = 'show_notes_v1'",
        )
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(run_count, 1);

        let dedupe_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM trigger_runs WHERE trigger_id = $1 AND dedupe_key = $2",
        )
        .bind(trigger_id)
        .bind("manual:manual-001")
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(dedupe_rows, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn dispatch_next_due_cron_trigger_creates_run_and_updates_schedule(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        create_cron_trigger(
            &test_db.app_pool,
            &NewCronTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                cron_expression: "0/1 * * * * * *".to_string(),
                schedule_timezone: "UTC".to_string(),
                input_json: json!({"source":"cron"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "enabled".to_string(),
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
            },
        )
        .await?;

        sqlx::query("UPDATE triggers SET next_fire_at = now() - interval '1 second' WHERE id = $1")
            .bind(trigger_id)
            .execute(&test_db.app_pool)
            .await?;

        let dispatched = dispatch_next_due_trigger(&test_db.app_pool)
            .await?
            .expect("due cron trigger should dispatch");
        assert_eq!(dispatched.trigger_type, "cron");
        assert!(dispatched.next_fire_at > dispatched.scheduled_for);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn dispatch_next_due_interval_trigger_respects_inflight_guardrails(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        create_interval_trigger(
            &test_db.app_pool,
            &NewIntervalTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                interval_seconds: 60,
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                next_fire_at: time::OffsetDateTime::now_utc() - time::Duration::seconds(1),
                status: "enabled".to_string(),
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let existing_run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &NewRun {
                id: existing_run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        sqlx::query(
            "INSERT INTO trigger_runs (id, trigger_id, run_id, scheduled_for, status, dedupe_key, error_json) VALUES ($1, $2, $3, now(), 'created', $4, NULL)",
        )
        .bind(Uuid::new_v4())
        .bind(trigger_id)
        .bind(existing_run_id)
        .bind("existing")
        .execute(&test_db.app_pool)
        .await?;

        let blocked = dispatch_next_due_interval_trigger(&test_db.app_pool).await?;
        assert!(blocked.is_none());

        let tenant_blocked =
            dispatch_next_due_interval_trigger_with_limits(&test_db.app_pool, 1).await?;
        assert!(tenant_blocked.is_none());

        let manual = fire_trigger_manually_with_limits(
            &test_db.app_pool,
            "single",
            trigger_id,
            "manual-guard",
            None,
            10,
        )
        .await?;
        assert!(matches!(manual, ManualTriggerFireOutcome::InflightLimited));

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn scheduler_lease_allows_single_active_owner() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let acquired_a = try_acquire_scheduler_lease(
            &test_db.app_pool,
            &SchedulerLeaseParams {
                lease_name: "trigger_dispatch".to_string(),
                lease_owner: "worker-a".to_string(),
                lease_for: Duration::from_secs(30),
            },
        )
        .await?;
        assert!(acquired_a);

        let acquired_b = try_acquire_scheduler_lease(
            &test_db.app_pool,
            &SchedulerLeaseParams {
                lease_name: "trigger_dispatch".to_string(),
                lease_owner: "worker-b".to_string(),
                lease_for: Duration::from_secs(30),
            },
        )
        .await?;
        assert!(!acquired_b);

        sqlx::query(
            "UPDATE scheduler_leases SET lease_expires_at = now() - interval '1 second' WHERE lease_name = $1",
        )
        .bind("trigger_dispatch")
        .execute(&test_db.app_pool)
        .await?;

        let acquired_b_after_expiry = try_acquire_scheduler_lease(
            &test_db.app_pool,
            &SchedulerLeaseParams {
                lease_name: "trigger_dispatch".to_string(),
                lease_owner: "worker-b".to_string(),
                lease_for: Duration::from_secs(30),
            },
        )
        .await?;
        assert!(acquired_b_after_expiry);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn llm_gateway_admission_leases_enforce_lane_capacity() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let namespace = "tenant:single:agent:test";
        let first = try_acquire_llm_gateway_admission_lease(
            &test_db.app_pool,
            &LlmGatewayAdmissionLeaseAcquireParams {
                namespace: namespace.to_string(),
                lane: "interactive".to_string(),
                max_inflight: 1,
                lease_id: Uuid::new_v4(),
                lease_owner: "worker-a".to_string(),
                lease_for: Duration::from_secs(30),
            },
        )
        .await?
        .expect("first lease");
        assert_eq!(first.slot_index, 1);

        let second = try_acquire_llm_gateway_admission_lease(
            &test_db.app_pool,
            &LlmGatewayAdmissionLeaseAcquireParams {
                namespace: namespace.to_string(),
                lane: "interactive".to_string(),
                max_inflight: 1,
                lease_id: Uuid::new_v4(),
                lease_owner: "worker-b".to_string(),
                lease_for: Duration::from_secs(30),
            },
        )
        .await?;
        assert!(second.is_none());

        let released = release_llm_gateway_admission_lease(&test_db.app_pool, &first).await?;
        assert!(released);

        let after_release = try_acquire_llm_gateway_admission_lease(
            &test_db.app_pool,
            &LlmGatewayAdmissionLeaseAcquireParams {
                namespace: namespace.to_string(),
                lane: "interactive".to_string(),
                max_inflight: 1,
                lease_id: Uuid::new_v4(),
                lease_owner: "worker-b".to_string(),
                lease_for: Duration::from_secs(30),
            },
        )
        .await?;
        assert!(after_release.is_some());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn payment_request_idempotency_returns_existing_request() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;

        let step = create_step(
            &test_db.app_pool,
            &NewStep {
                id: Uuid::new_v4(),
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "payment".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;
        let action_a = create_action_request(
            &test_db.app_pool,
            &NewActionRequest {
                id: Uuid::new_v4(),
                step_id: step.id,
                action_type: "payment.send".to_string(),
                args_json: json!({}),
                justification: None,
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;
        let action_b = create_action_request(
            &test_db.app_pool,
            &NewActionRequest {
                id: Uuid::new_v4(),
                step_id: step.id,
                action_type: "payment.send".to_string(),
                args_json: json!({}),
                justification: None,
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        let first = create_or_get_payment_request(
            &test_db.app_pool,
            &NewPaymentRequest {
                id: Uuid::new_v4(),
                action_request_id: action_a.id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: "idem-1".to_string(),
                amount_msat: Some(2100),
                request_json: json!({"invoice":"lnbc..." }),
                status: "requested".to_string(),
            },
        )
        .await?;

        let second = create_or_get_payment_request(
            &test_db.app_pool,
            &NewPaymentRequest {
                id: Uuid::new_v4(),
                action_request_id: action_b.id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: "idem-1".to_string(),
                amount_msat: Some(2100),
                request_json: json!({"invoice":"lnbc..." }),
                status: "requested".to_string(),
            },
        )
        .await?;

        assert_eq!(first.id, second.id);
        assert_eq!(second.action_request_id, action_a.id);

        let result = create_payment_result(
            &test_db.app_pool,
            &NewPaymentResult {
                id: Uuid::new_v4(),
                payment_request_id: first.id,
                status: "executed".to_string(),
                result_json: Some(json!({"settlement_status":"settled"})),
                error_json: None,
            },
        )
        .await?;
        assert_eq!(result.status, "executed");

        let latest = get_latest_payment_result(&test_db.app_pool, first.id).await?;
        assert!(latest.is_some());
        assert_eq!(
            latest.as_ref().map(|row| row.status.as_str()),
            Some("executed")
        );

        let updated =
            update_payment_request_status(&test_db.app_pool, first.id, "executed").await?;
        assert!(updated);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn payment_request_idempotency_key_is_scoped_by_tenant() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let tenant_a = "tenant_alpha";
        let tenant_b = "tenant_beta";
        let (agent_a, user_a) = seed_agent_and_user_for_tenant(&test_db.app_pool, tenant_a).await?;
        let (agent_b, user_b) = seed_agent_and_user_for_tenant(&test_db.app_pool, tenant_b).await?;
        let run_a = Uuid::new_v4();
        let run_b = Uuid::new_v4();
        let idempotency_key = "idem-shared-cross-tenant";

        for (run_id, tenant_id, agent_id, user_id) in [
            (run_a, tenant_a, agent_a, user_a),
            (run_b, tenant_b, agent_b, user_b),
        ] {
            create_run(
                &test_db.app_pool,
                &NewRun {
                    id: run_id,
                    tenant_id: tenant_id.to_string(),
                    agent_id,
                    triggered_by_user_id: Some(user_id),
                    recipe_id: "payments_v1".to_string(),
                    status: "queued".to_string(),
                    input_json: json!({}),
                    requested_capabilities: json!([]),
                    granted_capabilities: json!([]),
                    error_json: None,
                },
            )
            .await?;
        }

        let step_a = create_step(
            &test_db.app_pool,
            &NewStep {
                id: Uuid::new_v4(),
                run_id: run_a,
                tenant_id: tenant_a.to_string(),
                agent_id: agent_a,
                user_id: Some(user_a),
                name: "payment_a".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;
        let step_b = create_step(
            &test_db.app_pool,
            &NewStep {
                id: Uuid::new_v4(),
                run_id: run_b,
                tenant_id: tenant_b.to_string(),
                agent_id: agent_b,
                user_id: Some(user_b),
                name: "payment_b".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        let action_a = create_action_request(
            &test_db.app_pool,
            &NewActionRequest {
                id: Uuid::new_v4(),
                step_id: step_a.id,
                action_type: "payment.send".to_string(),
                args_json: json!({}),
                justification: None,
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;
        let action_b = create_action_request(
            &test_db.app_pool,
            &NewActionRequest {
                id: Uuid::new_v4(),
                step_id: step_b.id,
                action_type: "payment.send".to_string(),
                args_json: json!({}),
                justification: None,
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        let request_a = create_or_get_payment_request(
            &test_db.app_pool,
            &NewPaymentRequest {
                id: Uuid::new_v4(),
                action_request_id: action_a.id,
                run_id: run_a,
                tenant_id: tenant_a.to_string(),
                agent_id: agent_a,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: idempotency_key.to_string(),
                amount_msat: Some(1000),
                request_json: json!({"invoice":"lnbc1a"}),
                status: "requested".to_string(),
            },
        )
        .await?;

        let request_b = create_or_get_payment_request(
            &test_db.app_pool,
            &NewPaymentRequest {
                id: Uuid::new_v4(),
                action_request_id: action_b.id,
                run_id: run_b,
                tenant_id: tenant_b.to_string(),
                agent_id: agent_b,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: idempotency_key.to_string(),
                amount_msat: Some(1000),
                request_json: json!({"invoice":"lnbc1b"}),
                status: "requested".to_string(),
            },
        )
        .await?;

        assert_ne!(request_a.id, request_b.id);
        assert_eq!(request_a.action_request_id, action_a.id);
        assert_eq!(request_b.action_request_id, action_b.id);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn llm_token_usage_records_and_budget_sums_are_queryable() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "llm_remote_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        let step = create_step(
            &test_db.app_pool,
            &NewStep {
                id: Uuid::new_v4(),
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "llm".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;
        let action = create_action_request(
            &test_db.app_pool,
            &NewActionRequest {
                id: Uuid::new_v4(),
                step_id: step.id,
                action_type: "llm.infer".to_string(),
                args_json: json!({}),
                justification: None,
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        create_llm_token_usage_record(
            &test_db.app_pool,
            &NewLlmTokenUsageRecord {
                id: Uuid::new_v4(),
                run_id,
                action_request_id: action.id,
                tenant_id: "single".to_string(),
                agent_id,
                route: "remote".to_string(),
                model_key: "remote:mock-remote-model".to_string(),
                consumed_tokens: 420,
                estimated_cost_usd: Some(0.0021),
                window_started_at: time::OffsetDateTime::now_utc() - time::Duration::hours(1),
                window_duration_seconds: 86_400,
            },
        )
        .await?;

        let since = time::OffsetDateTime::now_utc() - time::Duration::hours(2);
        let tenant_sum =
            sum_llm_consumed_tokens_for_tenant_since(&test_db.app_pool, "single", since).await?;
        assert_eq!(tenant_sum, 420);
        let agent_sum =
            sum_llm_consumed_tokens_for_agent_since(&test_db.app_pool, "single", agent_id, since)
                .await?;
        assert_eq!(agent_sum, 420);
        let model_sum = sum_llm_consumed_tokens_for_model_since(
            &test_db.app_pool,
            "single",
            "remote:mock-remote-model",
            since,
        )
        .await?;
        assert_eq!(model_sum, 420);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn llm_gateway_cache_entries_roundtrip_and_prune() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let namespace = "tenant:single:agent:cache";
        let first_key = "cachekey-first";
        let second_key = "cachekey-second";

        upsert_llm_gateway_cache_entry(
            &test_db.app_pool,
            &NewLlmGatewayCacheEntry {
                cache_key_sha256: first_key.to_string(),
                namespace: namespace.to_string(),
                route: "local".to_string(),
                model: "local-model".to_string(),
                response_json: json!({
                    "response_text":"first",
                    "prompt_tokens":1,
                    "completion_tokens":2,
                    "total_tokens":3
                }),
                ttl: Duration::from_secs(60),
            },
        )
        .await?;

        let first = get_llm_gateway_cache_entry(&test_db.app_pool, namespace, first_key)
            .await?
            .expect("first cache entry");
        assert_eq!(first.route, "local");
        assert_eq!(first.response_json["response_text"], "first");

        tokio::time::sleep(Duration::from_millis(10)).await;
        upsert_llm_gateway_cache_entry(
            &test_db.app_pool,
            &NewLlmGatewayCacheEntry {
                cache_key_sha256: second_key.to_string(),
                namespace: namespace.to_string(),
                route: "remote".to_string(),
                model: "remote-model".to_string(),
                response_json: json!({
                    "response_text":"second",
                    "prompt_tokens":4,
                    "completion_tokens":5,
                    "total_tokens":9
                }),
                ttl: Duration::from_secs(60),
            },
        )
        .await?;

        let pruned = prune_llm_gateway_cache_namespace(&test_db.app_pool, namespace, 1).await?;
        assert!(pruned >= 1);
        let missing_first =
            get_llm_gateway_cache_entry(&test_db.app_pool, namespace, first_key).await?;
        assert!(missing_first.is_none());
        let second = get_llm_gateway_cache_entry(&test_db.app_pool, namespace, second_key)
            .await?
            .expect("second cache entry");
        assert_eq!(second.model, "remote-model");

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_records_persist_and_query_tenant_scoped() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        create_run(
            &test_db.app_pool,
            &NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "memory_v1".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        create_step(
            &test_db.app_pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "memory".to_string(),
                status: "running".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;

        let first = create_memory_record(
            &test_db.app_pool,
            &NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id,
                run_id: Some(run_id),
                step_id: Some(step_id),
                memory_kind: "semantic".to_string(),
                scope: "memory:project/roadmap".to_string(),
                content_json: json!({"fact":"Use White Noise first"}),
                summary_text: Some("communication preference".to_string()),
                source: "worker".to_string(),
                redaction_applied: true,
                expires_at: None,
            },
        )
        .await?;
        assert_eq!(first.memory_kind, "semantic");

        let other_agent_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'other', 'other-agent', 'active')",
        )
        .bind(other_agent_id)
        .execute(&test_db.app_pool)
        .await?;
        create_memory_record(
            &test_db.app_pool,
            &NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "other".to_string(),
                agent_id: other_agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "semantic".to_string(),
                scope: "memory:project/private".to_string(),
                content_json: json!({"fact":"other tenant"}),
                summary_text: None,
                source: "worker".to_string(),
                redaction_applied: false,
                expires_at: None,
            },
        )
        .await?;

        let single_rows = list_tenant_memory_records(
            &test_db.app_pool,
            "single",
            Some(agent_id),
            Some("semantic"),
            Some("memory:project"),
            50,
        )
        .await?;
        assert_eq!(single_rows.len(), 1);
        assert_eq!(single_rows[0].id, first.id);
        assert_eq!(single_rows[0].tenant_id, "single");
        assert_eq!(single_rows[0].scope, "memory:project/roadmap");

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn handoff_memory_listing_filters_by_to_and_from_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (to_agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let from_agent_a = Uuid::new_v4();
        let from_agent_b = Uuid::new_v4();

        create_memory_record(
            &test_db.app_pool,
            &NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id: to_agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "handoff".to_string(),
                scope: format!("memory:handoff/{to_agent_id}/{}", Uuid::new_v4()),
                content_json: json!({
                    "packet_id": Uuid::new_v4(),
                    "from_agent_id": from_agent_a,
                    "to_agent_id": to_agent_id,
                    "title": "packet-a",
                    "payload_json": {"task":"alpha"},
                }),
                summary_text: Some("packet-a".to_string()),
                source: "api".to_string(),
                redaction_applied: false,
                expires_at: None,
            },
        )
        .await?;

        create_memory_record(
            &test_db.app_pool,
            &NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id: to_agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "handoff".to_string(),
                scope: format!("memory:handoff/{to_agent_id}/{}", Uuid::new_v4()),
                content_json: json!({
                    "packet_id": Uuid::new_v4(),
                    "from_agent_id": from_agent_b,
                    "to_agent_id": to_agent_id,
                    "title": "packet-b",
                    "payload_json": {"task":"beta"},
                }),
                summary_text: Some("packet-b".to_string()),
                source: "api".to_string(),
                redaction_applied: false,
                expires_at: None,
            },
        )
        .await?;

        let by_to = list_tenant_handoff_memory_records(
            &test_db.app_pool,
            "single",
            Some(to_agent_id),
            None,
            20,
        )
        .await?;
        assert_eq!(by_to.len(), 2);

        let by_to_and_from = list_tenant_handoff_memory_records(
            &test_db.app_pool,
            "single",
            Some(to_agent_id),
            Some(from_agent_a),
            20,
        )
        .await?;
        assert_eq!(by_to_and_from.len(), 1);
        let expected_from_agent = from_agent_a.to_string();
        assert_eq!(
            by_to_and_from[0]
                .content_json
                .get("from_agent_id")
                .and_then(serde_json::Value::as_str),
            Some(expected_from_agent.as_str())
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_purge_and_compaction_records_work() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let expired = create_memory_record(
            &test_db.app_pool,
            &NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "session".to_string(),
                scope: "memory:session/chat-1".to_string(),
                content_json: json!({"note":"expired"}),
                summary_text: None,
                source: "api".to_string(),
                redaction_applied: false,
                expires_at: Some(OffsetDateTime::now_utc() - time::Duration::hours(1)),
            },
        )
        .await?;
        let retained = create_memory_record(
            &test_db.app_pool,
            &NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "session".to_string(),
                scope: "memory:session/chat-1".to_string(),
                content_json: json!({"note":"retained"}),
                summary_text: None,
                source: "api".to_string(),
                redaction_applied: false,
                expires_at: Some(OffsetDateTime::now_utc() + time::Duration::hours(1)),
            },
        )
        .await?;

        let visible_before_purge = list_tenant_memory_records(
            &test_db.app_pool,
            "single",
            Some(agent_id),
            Some("session"),
            Some("memory:session"),
            10,
        )
        .await?;
        assert_eq!(visible_before_purge.len(), 1);
        assert_eq!(visible_before_purge[0].id, retained.id);

        let purge = purge_expired_tenant_memory_records(
            &test_db.app_pool,
            "single",
            OffsetDateTime::now_utc(),
        )
        .await?;
        assert_eq!(purge.deleted_count, 1);
        assert_eq!(purge.tenant_id, "single");

        let remaining = list_tenant_memory_records(
            &test_db.app_pool,
            "single",
            Some(agent_id),
            Some("session"),
            Some("memory:session"),
            10,
        )
        .await?;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, retained.id);
        assert_ne!(remaining[0].id, expired.id);

        let compaction = create_memory_compaction_record(
            &test_db.app_pool,
            &NewMemoryCompactionRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                memory_kind: "session".to_string(),
                scope: "memory:session/chat-1".to_string(),
                source_count: 1,
                source_entry_ids: json!([retained.id]),
                summary_json: json!({"summary":"retained memory compacted"}),
            },
        )
        .await?;
        assert_eq!(compaction.source_count, 1);
        assert_eq!(compaction.memory_kind, "session");

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_retrieval_under_concurrent_load_is_tenant_isolated_and_bounded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_primary, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let agent_secondary = Uuid::new_v4();
        sqlx::query("INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'single', 'secureagnt_secondary', 'active')")
            .bind(agent_secondary)
            .execute(&test_db.app_pool)
            .await?;

        let agent_other_tenant = Uuid::new_v4();
        sqlx::query("INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'other', 'secureagnt_other', 'active')")
            .bind(agent_other_tenant)
            .execute(&test_db.app_pool)
            .await?;

        for idx in 0..120 {
            let agent_id = if idx % 2 == 0 {
                agent_primary
            } else {
                agent_secondary
            };
            create_memory_record(
                &test_db.app_pool,
                &NewMemoryRecord {
                    id: Uuid::new_v4(),
                    tenant_id: "single".to_string(),
                    agent_id,
                    run_id: None,
                    step_id: None,
                    memory_kind: "semantic".to_string(),
                    scope: "memory:project/load".to_string(),
                    content_json: json!({"note": format!("single-{idx}")}),
                    summary_text: None,
                    source: "worker".to_string(),
                    redaction_applied: false,
                    expires_at: if idx % 10 == 0 {
                        Some(OffsetDateTime::now_utc() - time::Duration::minutes(10))
                    } else {
                        None
                    },
                },
            )
            .await?;
        }

        for idx in 0..80 {
            create_memory_record(
                &test_db.app_pool,
                &NewMemoryRecord {
                    id: Uuid::new_v4(),
                    tenant_id: "other".to_string(),
                    agent_id: agent_other_tenant,
                    run_id: None,
                    step_id: None,
                    memory_kind: "semantic".to_string(),
                    scope: "memory:project/load".to_string(),
                    content_json: json!({"note": format!("other-{idx}")}),
                    summary_text: None,
                    source: "worker".to_string(),
                    redaction_applied: false,
                    expires_at: None,
                },
            )
            .await?;
        }

        let started = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..12 {
            let pool = test_db.app_pool.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..20 {
                    let rows = list_tenant_memory_records(
                        &pool,
                        "single",
                        None,
                        Some("semantic"),
                        Some("memory:project/load"),
                        200,
                    )
                    .await
                    .expect("list_tenant_memory_records should succeed");
                    assert!(
                        !rows.is_empty(),
                        "single tenant memory list should not be empty"
                    );
                    assert!(
                        rows.iter().all(|row| row.tenant_id == "single"),
                        "memory list should be tenant scoped"
                    );
                    assert!(
                        rows.iter().all(|row| row
                            .expires_at
                            .is_none_or(|expires_at| expires_at > OffsetDateTime::now_utc())),
                        "expired records should not be returned by list query"
                    );
                }
            }));
        }

        for handle in handles {
            handle
                .await
                .map_err(|err| format!("concurrent retrieval task join failure: {err}"))?;
        }

        let elapsed = started.elapsed();
        let max_ms = read_benchmark_limit_ms("MEMORY_RETRIEVAL_BENCH_MAX_MS", 15_000);
        assert!(
            elapsed.as_millis() <= max_ms as u128,
            "memory retrieval benchmark exceeded limit: {}ms > {}ms",
            elapsed.as_millis(),
            max_ms
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_compaction_under_load_compacts_groups_and_exposes_stats(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        for idx in 0..40 {
            create_memory_record(
                &test_db.app_pool,
                &NewMemoryRecord {
                    id: Uuid::new_v4(),
                    tenant_id: "single".to_string(),
                    agent_id,
                    run_id: None,
                    step_id: None,
                    memory_kind: "session".to_string(),
                    scope: "memory:session/load-1".to_string(),
                    content_json: json!({"idx": idx}),
                    summary_text: None,
                    source: "worker".to_string(),
                    redaction_applied: false,
                    expires_at: None,
                },
            )
            .await?;
        }

        let compacted =
            compact_memory_records(&test_db.app_pool, OffsetDateTime::now_utc(), 10, 10).await?;
        assert_eq!(compacted.processed_groups, 1);
        assert_eq!(compacted.compacted_source_records, 40);
        assert_eq!(compacted.groups.len(), 1);

        let active_rows = list_tenant_memory_records(
            &test_db.app_pool,
            "single",
            Some(agent_id),
            Some("session"),
            Some("memory:session/load-1"),
            100,
        )
        .await?;
        assert_eq!(active_rows.len(), 0);

        let stats = get_tenant_memory_compaction_stats(
            &test_db.app_pool,
            "single",
            Some(OffsetDateTime::now_utc() - time::Duration::hours(1)),
        )
        .await?;
        assert_eq!(stats.compacted_groups_window, 1);
        assert_eq!(stats.compacted_source_records_window, 40);
        assert_eq!(stats.pending_uncompacted_records, 0);
        assert!(stats.last_compacted_at.is_some());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_compaction_respects_group_limit() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        for scope_idx in 0..3 {
            for item_idx in 0..12 {
                create_memory_record(
                    &test_db.app_pool,
                    &NewMemoryRecord {
                        id: Uuid::new_v4(),
                        tenant_id: "single".to_string(),
                        agent_id,
                        run_id: None,
                        step_id: None,
                        memory_kind: "semantic".to_string(),
                        scope: format!("memory:project/scope-{scope_idx}"),
                        content_json: json!({"item_idx": item_idx}),
                        summary_text: None,
                        source: "worker".to_string(),
                        redaction_applied: false,
                        expires_at: None,
                    },
                )
                .await?;
            }
        }

        let compacted =
            compact_memory_records(&test_db.app_pool, OffsetDateTime::now_utc(), 10, 2).await?;
        assert_eq!(compacted.processed_groups, 2);
        assert_eq!(compacted.groups.len(), 2);
        assert_eq!(compacted.compacted_source_records, 24);

        let stats = get_tenant_memory_compaction_stats(&test_db.app_pool, "single", None).await?;
        assert_eq!(stats.compacted_groups_window, 2);
        assert_eq!(stats.pending_uncompacted_records, 12);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

async fn seed_agent_and_user(pool: &PgPool) -> Result<(Uuid, Uuid), sqlx::Error> {
    seed_agent_and_user_for_tenant(pool, "single").await
}

async fn seed_agent_and_user_for_tenant(
    pool: &PgPool,
    tenant_id: &str,
) -> Result<(Uuid, Uuid), sqlx::Error> {
    let agent_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO agents (id, tenant_id, name, status)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(agent_id)
    .bind(tenant_id)
    .bind("secureagnt_local")
    .bind("active")
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO users (id, tenant_id, external_subject, display_name, status)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind("local:user:1")
    .bind("Local User")
    .bind("active")
    .execute(pool)
    .await?;

    Ok((agent_id, user_id))
}

async fn setup_test_db() -> Result<Option<TestDb>, Box<dyn std::error::Error>> {
    if !run_db_tests_enabled() {
        eprintln!("skipping db integration test; set RUN_DB_TESTS=1 to enable");
        return Ok(None);
    }

    let database_url = test_database_url();
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;

    let schema = format!("test_{}", Uuid::new_v4().simple());
    let create_schema_sql = format!("CREATE SCHEMA {schema}");
    sqlx::query(&create_schema_sql).execute(&admin_pool).await?;

    let connect_options =
        PgConnectOptions::from_str(&database_url)?.options([("search_path", schema.as_str())]);
    let app_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(connect_options)
        .await?;

    sqlx::migrate!("../migrations").run(&app_pool).await?;

    Ok(Some(TestDb {
        admin_pool,
        app_pool,
        schema,
    }))
}

async fn teardown_test_db(test_db: TestDb) -> Result<(), sqlx::Error> {
    test_db.app_pool.close().await;
    let drop_schema_sql = format!("DROP SCHEMA IF EXISTS {} CASCADE", test_db.schema);
    sqlx::query(&drop_schema_sql)
        .execute(&test_db.admin_pool)
        .await?;
    test_db.admin_pool.close().await;
    Ok(())
}

fn run_db_tests_enabled() -> bool {
    match env::var("RUN_DB_TESTS") {
        Ok(value) => value == "1" || value.eq_ignore_ascii_case("true"),
        Err(_) => false,
    }
}

fn test_database_url() -> String {
    env::var("TEST_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/agentdb".to_string())
}

fn read_benchmark_limit_ms(key: &str, default: u64) -> u64 {
    match env::var(key) {
        Ok(value) => value.parse::<u64>().unwrap_or(default),
        Err(_) => default,
    }
}

fn new_run(run_id: Uuid, agent_id: Uuid, user_id: Uuid, status: &str) -> NewRun {
    NewRun {
        id: run_id,
        tenant_id: "single".to_string(),
        agent_id,
        triggered_by_user_id: Some(user_id),
        recipe_id: "show_notes_v1".to_string(),
        status: status.to_string(),
        input_json: json!({"transcript_path":"podcasts/ep245/transcript.txt"}),
        requested_capabilities: json!([]),
        granted_capabilities: json!([]),
        error_json: None,
    }
}

fn run_async<F>(future: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(future)
}
