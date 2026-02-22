use core::{
    claim_next_queued_run_dual, claim_pending_compliance_siem_delivery_records_dual,
    claim_next_queued_run_with_limits_dual, compact_memory_records_dual, create_action_request_dual,
    create_action_result_dual,
    create_llm_token_usage_record_dual, create_or_get_payment_request_dual,
    create_payment_result_dual, create_run_dual, create_step_dual,
    dispatch_next_due_trigger_with_limits_dual, get_run_status_dual, get_tenant_ops_summary_dual,
    mark_compliance_siem_delivery_record_dead_lettered_dual,
    mark_compliance_siem_delivery_record_delivered_dual,
    mark_compliance_siem_delivery_record_failed_dual, persist_artifact_metadata_dual,
    renew_run_lease_dual, requeue_expired_runs_dual,
    sum_executed_payment_amount_msat_for_agent_dual,
    sum_executed_payment_amount_msat_for_tenant_dual, sum_llm_consumed_tokens_for_agent_since_dual,
    sum_llm_consumed_tokens_for_model_since_dual, sum_llm_consumed_tokens_for_tenant_since_dual,
    try_acquire_scheduler_lease_dual, update_action_request_status_dual,
    update_payment_request_status_dual, DbPool, NewActionRequest, NewActionResult, NewArtifact,
    NewLlmTokenUsageRecord, NewPaymentRequest, NewPaymentResult, NewRun, NewStep,
    SchedulerLeaseParams,
};
use serde_json::json;
use sqlx::SqlitePool;
use time::OffsetDateTime;
use uuid::Uuid;

#[test]
fn db_worker_dual_sqlite_claim_action_payment_usage_flow() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let pool = DbPool::connect("sqlite::memory:", 1).await?;
        pool.migrate().await?;

        let sqlite = sqlite_pool(&pool)?;
        let tenant_id = "solo";
        let agent_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();

        seed_agent_and_user(sqlite, tenant_id, agent_id, user_id).await?;

        create_run_dual(
            &pool,
            &NewRun {
                id: run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"hello"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "queued".to_string(),
                error_json: None,
            },
        )
        .await?;

        let claimed =
            claim_next_queued_run_dual(&pool, "worker-sqlite", std::time::Duration::from_secs(30))
                .await?
                .expect("queued run should be claimed");
        assert_eq!(claimed.id, run_id);
        assert_eq!(claimed.status, "running");

        let renewed = renew_run_lease_dual(
            &pool,
            run_id,
            "worker-sqlite",
            std::time::Duration::from_secs(30),
        )
        .await?;
        assert!(renewed);

        let step_id = Uuid::new_v4();
        create_step_dual(
            &pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "skill.invoke".to_string(),
                status: "running".to_string(),
                input_json: json!({"text":"hello"}),
                error_json: None,
            },
        )
        .await?;

        let action_request_id = Uuid::new_v4();
        create_action_request_dual(
            &pool,
            &NewActionRequest {
                id: action_request_id,
                step_id,
                action_type: "object.write".to_string(),
                args_json: json!({"path":"shownotes/out.md","content":"# Hi"}),
                justification: Some("persist output".to_string()),
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        assert!(
            update_action_request_status_dual(&pool, action_request_id, "executed", None,).await?
        );

        create_action_result_dual(
            &pool,
            &NewActionResult {
                id: Uuid::new_v4(),
                action_request_id,
                status: "executed".to_string(),
                result_json: Some(json!({"ok":true})),
                error_json: None,
            },
        )
        .await?;

        let artifact = persist_artifact_metadata_dual(
            &pool,
            &NewArtifact {
                id: Uuid::new_v4(),
                run_id,
                path: "shownotes/out.md".to_string(),
                content_type: "text/markdown".to_string(),
                size_bytes: 4,
                checksum: None,
                storage_ref: "artifacts/solo/out.md".to_string(),
            },
        )
        .await?;
        assert_eq!(artifact.path, "shownotes/out.md");

        let payment_request_id = Uuid::new_v4();
        let payment_action_request_id = Uuid::new_v4();
        create_action_request_dual(
            &pool,
            &NewActionRequest {
                id: payment_action_request_id,
                step_id,
                action_type: "payment.send".to_string(),
                args_json: json!({
                    "destination":"nwc:wallet-main",
                    "operation":"pay_invoice",
                    "idempotency_key":"idem-1",
                }),
                justification: Some("payment test".to_string()),
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        let first_payment_request = create_or_get_payment_request_dual(
            &pool,
            &NewPaymentRequest {
                id: payment_request_id,
                action_request_id: payment_action_request_id,
                run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: "idem-1".to_string(),
                amount_msat: Some(1250),
                request_json: json!({"invoice":"lnbc..."}),
                status: "requested".to_string(),
            },
        )
        .await?;

        let duplicate_payment_request = create_or_get_payment_request_dual(
            &pool,
            &NewPaymentRequest {
                id: Uuid::new_v4(),
                action_request_id: Uuid::new_v4(),
                run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: "idem-1".to_string(),
                amount_msat: Some(1250),
                request_json: json!({"invoice":"lnbc..."}),
                status: "requested".to_string(),
            },
        )
        .await?;
        assert_eq!(duplicate_payment_request.id, first_payment_request.id);
        assert_eq!(
            duplicate_payment_request.action_request_id,
            first_payment_request.action_request_id
        );

        create_payment_result_dual(
            &pool,
            &NewPaymentResult {
                id: Uuid::new_v4(),
                payment_request_id,
                status: "executed".to_string(),
                result_json: Some(json!({"settlement_status":"settled"})),
                error_json: None,
            },
        )
        .await?;
        assert!(update_payment_request_status_dual(&pool, payment_request_id, "executed").await?);

        let tenant_payment_spend =
            sum_executed_payment_amount_msat_for_tenant_dual(&pool, tenant_id).await?;
        let agent_payment_spend =
            sum_executed_payment_amount_msat_for_agent_dual(&pool, tenant_id, agent_id).await?;
        assert_eq!(tenant_payment_spend, 1250);
        assert_eq!(agent_payment_spend, 1250);

        let llm_action_request_id = Uuid::new_v4();
        create_action_request_dual(
            &pool,
            &NewActionRequest {
                id: llm_action_request_id,
                step_id,
                action_type: "llm.infer".to_string(),
                args_json: json!({"scope":"remote:test-model"}),
                justification: Some("llm token test".to_string()),
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        create_llm_token_usage_record_dual(
            &pool,
            &NewLlmTokenUsageRecord {
                id: Uuid::new_v4(),
                run_id,
                action_request_id: llm_action_request_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                route: "remote".to_string(),
                model_key: "remote:test-model".to_string(),
                consumed_tokens: 320,
                estimated_cost_usd: Some(0.0012),
                window_started_at: OffsetDateTime::now_utc() - time::Duration::minutes(5),
                window_duration_seconds: 300,
            },
        )
        .await?;

        let since = OffsetDateTime::now_utc() - time::Duration::hours(1);
        let tenant_tokens =
            sum_llm_consumed_tokens_for_tenant_since_dual(&pool, tenant_id, since).await?;
        let agent_tokens =
            sum_llm_consumed_tokens_for_agent_since_dual(&pool, tenant_id, agent_id, since).await?;
        let model_tokens = sum_llm_consumed_tokens_for_model_since_dual(
            &pool,
            tenant_id,
            "remote:test-model",
            since,
        )
        .await?;
        assert_eq!(tenant_tokens, 320);
        assert_eq!(agent_tokens, 320);
        assert_eq!(model_tokens, 320);

        // Expire the lease and ensure requeue works on sqlite.
        sqlx::query(
            r#"
            UPDATE runs
            SET lease_expires_at = datetime('now', '-1 minute')
            WHERE id = ?1
            "#,
        )
        .bind(run_id.to_string())
        .execute(sqlite)
        .await?;

        let requeued = requeue_expired_runs_dual(&pool, 10).await?;
        assert_eq!(requeued, 1);

        let status = get_run_status_dual(&pool, tenant_id, run_id)
            .await?
            .expect("run should exist");
        assert_eq!(status.status, "queued");

        let ops = get_tenant_ops_summary_dual(
            &pool,
            tenant_id,
            OffsetDateTime::now_utc() - time::Duration::hours(1),
        )
        .await?;
        assert_eq!(ops.queued_runs, 1);

        Ok(())
    })
}

#[test]
fn db_worker_dual_sqlite_claim_next_queued_run_with_limits_dual_respects_global_cap() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let pool = DbPool::connect("sqlite::memory:", 1).await?;
        pool.migrate().await?;

        let sqlite = sqlite_pool(&pool)?;
        let tenant_id = "solo";
        let agent_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let first_run_id = Uuid::new_v4();
        let second_run_id = Uuid::new_v4();

        seed_agent_and_user(sqlite, tenant_id, agent_id, user_id).await?;
        create_run_dual(
            &pool,
            &NewRun {
                id: first_run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"first"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "queued".to_string(),
                error_json: None,
            },
        )
        .await?;

        let claimed_running = claim_next_queued_run_dual(
            &pool,
            "worker-a",
            std::time::Duration::from_secs(30),
        )
        .await?
        .expect("expected one queued run to be claimed before cap check");
        assert_eq!(claimed_running.id, first_run_id);

        create_run_dual(
            &pool,
            &NewRun {
                id: second_run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"second"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "queued".to_string(),
                error_json: None,
            },
        )
        .await?;

        let blocked = claim_next_queued_run_with_limits_dual(
            &pool,
            "worker-a",
            std::time::Duration::from_secs(30),
            1,
            1000,
        )
        .await?
        .is_none();
        assert!(blocked);

        let claimed = claim_next_queued_run_with_limits_dual(
            &pool,
            "worker-a",
            std::time::Duration::from_secs(30),
            2,
            1000,
        )
        .await?;
        let claimed = claimed.expect("expected queued run when cap allows");
        assert_eq!(claimed.id, second_run_id);

        let second_status: String =
            sqlx::query_scalar("SELECT status FROM runs WHERE id = ?1")
                .bind(second_run_id.to_string())
                .fetch_one(sqlite)
                .await?;
        assert_eq!(second_status, "running");

        let claimed_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status = 'running'")
                .fetch_one(sqlite)
                .await?;
        assert_eq!(claimed_count, 2);

        Ok(())
    })
}

#[test]
fn db_worker_dual_sqlite_claim_next_queued_run_with_limits_dual_respects_tenant_fairness()
    -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let pool = DbPool::connect("sqlite::memory:", 1).await?;
        pool.migrate().await?;

        let sqlite = sqlite_pool(&pool)?;
        let tenant_a = "tenant_a";
        let tenant_b = "tenant_b";
        let tenant_a_agent = Uuid::new_v4();
        let tenant_b_agent = Uuid::new_v4();
        let tenant_a_user = Uuid::new_v4();
        let tenant_b_user = Uuid::new_v4();

        let tenant_a_run_one = Uuid::new_v4();
        let tenant_a_run_two = Uuid::new_v4();
        let tenant_b_run = Uuid::new_v4();

        seed_agent_and_user(sqlite, tenant_a, tenant_a_agent, tenant_a_user).await?;
        seed_agent_and_user(sqlite, tenant_b, tenant_b_agent, tenant_b_user).await?;

        create_run_dual(
            &pool,
            &NewRun {
                id: tenant_a_run_one,
                tenant_id: tenant_a.to_string(),
                agent_id: tenant_a_agent,
                triggered_by_user_id: Some(tenant_a_user),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"tenant-a-one"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "queued".to_string(),
                error_json: None,
            },
        )
        .await?;

        create_run_dual(
            &pool,
            &NewRun {
                id: tenant_a_run_two,
                tenant_id: tenant_a.to_string(),
                agent_id: tenant_a_agent,
                triggered_by_user_id: Some(tenant_a_user),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"tenant-a-two"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "queued".to_string(),
                error_json: None,
            },
        )
        .await?;

        create_run_dual(
            &pool,
            &NewRun {
                id: tenant_b_run,
                tenant_id: tenant_b.to_string(),
                agent_id: tenant_b_agent,
                triggered_by_user_id: Some(tenant_b_user),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"tenant-b"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "queued".to_string(),
                error_json: None,
            },
        )
        .await?;

        sqlx::query("UPDATE runs SET created_at = datetime('now', '-10 minutes') WHERE id = ?1")
            .bind(tenant_a_run_one.to_string())
            .execute(sqlite)
            .await?;
        sqlx::query("UPDATE runs SET created_at = datetime('now', '-9 minutes') WHERE id = ?1")
            .bind(tenant_a_run_two.to_string())
            .execute(sqlite)
            .await?;
        sqlx::query("UPDATE runs SET created_at = datetime('now', '-1 minutes') WHERE id = ?1")
            .bind(tenant_b_run.to_string())
            .execute(sqlite)
            .await?;

        let claimed = claim_next_queued_run_dual(
            &pool,
            "worker-a",
            std::time::Duration::from_secs(30),
        )
        .await?
        .expect("first tenant-a run should be claimed");
        assert_eq!(claimed.id, tenant_a_run_one);

        let next = claim_next_queued_run_with_limits_dual(
            &pool,
            "worker-a",
            std::time::Duration::from_secs(30),
            1000,
            1,
        )
        .await?
        .expect("expected tenant-b run to be claimed with tenant fairness");
        assert_eq!(next.id, tenant_b_run);
        assert_eq!(next.tenant_id, tenant_b);

        let blocked = claim_next_queued_run_with_limits_dual(
            &pool,
            "worker-a",
            std::time::Duration::from_secs(30),
            1000,
            1,
        )
        .await?;
        assert!(blocked.is_none());

        let claimed_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE status = 'running'")
            .fetch_one(sqlite)
            .await?;
        assert_eq!(claimed_count, 2);

        Ok(())
    })
}

#[test]
fn db_worker_dual_sqlite_dispatch_next_due_trigger_with_limits_dual_respects_tenant_cap() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let pool = DbPool::connect("sqlite::memory:", 1).await?;
        pool.migrate().await?;
        let sqlite = sqlite_pool(&pool)?;

        let tenant_id = "tenant_one";
        let agent_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        seed_agent_and_user(sqlite, tenant_id, agent_id, user_id).await?;

        sqlx::query(
            r#"
            INSERT INTO triggers (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
                interval_seconds, schedule_timezone, misfire_policy, max_attempts, max_inflight_runs,
                jitter_seconds, input_json, requested_capabilities, granted_capabilities, next_fire_at
            )
            VALUES (
                ?1, ?2, ?3, ?4, 'sqlite_interval', 'enabled', 'interval',
                60, 'UTC', 'fire_now', 3, 10, 0, '{}', '[]', '[]', datetime('now', '-1 minute')
            )
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(tenant_id)
        .bind(agent_id.to_string())
        .bind(user_id.to_string())
        .execute(sqlite)
        .await?;

        let in_flight_run = Uuid::new_v4();
        create_run_dual(
            &pool,
            &NewRun {
                id: in_flight_run,
                tenant_id: tenant_id.to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"active-run"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "running".to_string(),
                error_json: None,
            },
        )
        .await?;

        let blocked = dispatch_next_due_trigger_with_limits_dual(&pool, 1)
            .await?
            .is_none();
        assert!(blocked);

        sqlx::query("UPDATE runs SET status = 'succeeded' WHERE id = ?1")
            .bind(in_flight_run.to_string())
            .execute(sqlite)
            .await?;

        let dispatched = dispatch_next_due_trigger_with_limits_dual(&pool, 1)
            .await?
            .ok_or("expected trigger dispatch once tenant cap allows again")?;
        assert_eq!(dispatched.trigger_type, "interval");

        let queued_runs = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM runs WHERE tenant_id = ?1 AND status = 'queued'",
        )
        .bind(tenant_id)
        .fetch_one(sqlite)
        .await?;
        assert_eq!(queued_runs, 1);

        Ok(())
    })
}

#[test]
fn db_worker_dual_sqlite_scheduler_compaction_and_siem_flow(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let pool = DbPool::connect("sqlite::memory:", 1).await?;
        pool.migrate().await?;
        let sqlite = sqlite_pool(&pool)?;

        let tenant_id = "solo";
        let agent_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        seed_agent_and_user(sqlite, tenant_id, agent_id, user_id).await?;

        let lease_name = "sqlite-scheduler";
        let acquired = try_acquire_scheduler_lease_dual(
            &pool,
            &SchedulerLeaseParams {
                lease_name: lease_name.to_string(),
                lease_owner: "worker-a".to_string(),
                lease_for: std::time::Duration::from_secs(5),
            },
        )
        .await?;
        assert!(acquired);

        let acquired_by_other = try_acquire_scheduler_lease_dual(
            &pool,
            &SchedulerLeaseParams {
                lease_name: lease_name.to_string(),
                lease_owner: "worker-b".to_string(),
                lease_for: std::time::Duration::from_secs(5),
            },
        )
        .await?;
        assert!(!acquired_by_other);

        let interval_trigger_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO triggers (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
                interval_seconds, schedule_timezone, misfire_policy, max_attempts, max_inflight_runs,
                jitter_seconds, input_json, requested_capabilities, granted_capabilities, next_fire_at
            )
            VALUES (
                ?1, ?2, ?3, ?4, 'sqlite_interval', 'enabled', 'interval',
                60, 'UTC', 'fire_now', 3, 10, 0, '{}', '[]', '[]', datetime('now', '-1 minute')
            )
            "#,
        )
        .bind(interval_trigger_id.to_string())
        .bind(tenant_id)
        .bind(agent_id.to_string())
        .bind(user_id.to_string())
        .execute(sqlite)
        .await?;

        let cron_trigger_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO triggers (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
                cron_expression, schedule_timezone, misfire_policy, max_attempts, max_inflight_runs,
                jitter_seconds, input_json, requested_capabilities, granted_capabilities, next_fire_at
            )
            VALUES (
                ?1, ?2, ?3, ?4, 'sqlite_cron', 'enabled', 'cron',
                '0/1 * * * * * *', 'UTC', 'fire_now', 3, 10, 0, '{}', '[]', '[]', datetime('now', '-1 minute')
            )
            "#,
        )
        .bind(cron_trigger_id.to_string())
        .bind(tenant_id)
        .bind(agent_id.to_string())
        .bind(user_id.to_string())
        .execute(sqlite)
        .await?;

        let webhook_trigger_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO triggers (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, trigger_type,
                schedule_timezone, misfire_policy, max_attempts, max_inflight_runs, jitter_seconds,
                input_json, requested_capabilities, granted_capabilities, next_fire_at
            )
            VALUES (
                ?1, ?2, ?3, ?4, 'sqlite_webhook', 'enabled', 'webhook',
                'UTC', 'fire_now', 3, 10, 0, '{}', '[]', '[]', datetime('now', '-1 minute')
            )
            "#,
        )
        .bind(webhook_trigger_id.to_string())
        .bind(tenant_id)
        .bind(agent_id.to_string())
        .bind(user_id.to_string())
        .execute(sqlite)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO trigger_events (
                id, trigger_id, tenant_id, event_id, payload_json, status, next_attempt_at
            )
            VALUES (?1, ?2, ?3, 'evt-1', '{"sample":true}', 'pending', datetime('now', '-1 minute'))
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(webhook_trigger_id.to_string())
        .bind(tenant_id)
        .execute(sqlite)
        .await?;

        let first = dispatch_next_due_trigger_with_limits_dual(&pool, 100)
            .await?
            .ok_or("expected first trigger dispatch")?;
        let second = dispatch_next_due_trigger_with_limits_dual(&pool, 100)
            .await?
            .ok_or("expected second trigger dispatch")?;
        let third = dispatch_next_due_trigger_with_limits_dual(&pool, 100)
            .await?
            .ok_or("expected third trigger dispatch")?;

        let mut dispatched_types =
            vec![first.trigger_type, second.trigger_type, third.trigger_type];
        dispatched_types.sort();
        assert!(dispatched_types.contains(&"cron".to_string()));
        assert!(dispatched_types.contains(&"webhook".to_string()));

        sqlx::query("UPDATE triggers SET status = 'disabled' WHERE id = ?1")
            .bind(cron_trigger_id.to_string())
            .execute(sqlite)
            .await?;
        let fourth = dispatch_next_due_trigger_with_limits_dual(&pool, 100)
            .await?
            .ok_or("expected fourth trigger dispatch for interval")?;
        assert_eq!(fourth.trigger_type, "interval");

        let queued_runs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runs WHERE tenant_id = ?1 AND status = 'queued'",
        )
        .bind(tenant_id)
        .fetch_one(sqlite)
        .await?;
        assert_eq!(queued_runs, 4);

        let memory_a = Uuid::new_v4();
        let memory_b = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO memory_records (
                id, tenant_id, agent_id, memory_kind, scope, content_json, source, redaction_applied, created_at
            )
            VALUES
                (?1, ?3, ?4, 'session', 'memory:session/sqlite', '{"m":"a"}', 'worker', 0, datetime('now', '-10 minutes')),
                (?2, ?3, ?4, 'session', 'memory:session/sqlite', '{"m":"b"}', 'worker', 0, datetime('now', '-9 minutes'))
            "#,
        )
        .bind(memory_a.to_string())
        .bind(memory_b.to_string())
        .bind(tenant_id)
        .bind(agent_id.to_string())
        .execute(sqlite)
        .await?;

        let compaction =
            compact_memory_records_dual(&pool, OffsetDateTime::now_utc(), 2, 5).await?;
        assert_eq!(compaction.processed_groups, 1);
        assert_eq!(compaction.compacted_source_records, 2);

        let compacted_rows: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM memory_records
            WHERE tenant_id = ?1
              AND scope = 'memory:session/sqlite'
              AND compacted_at IS NOT NULL
            "#,
        )
        .bind(tenant_id)
        .fetch_one(sqlite)
        .await?;
        assert_eq!(compacted_rows, 2);

        let delivery_a = Uuid::new_v4();
        let delivery_b = Uuid::new_v4();
        let delivery_c = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO compliance_siem_delivery_outbox (
                id, tenant_id, run_id, adapter, delivery_target, content_type, payload_ndjson, status, max_attempts, next_attempt_at
            )
            VALUES
                (?1, ?4, NULL, 'secureagnt_ndjson', 'mock://success', 'application/x-ndjson', '{"a":1}', 'pending', 5, datetime('now', '-1 second')),
                (?2, ?4, NULL, 'secureagnt_ndjson', 'mock://fail', 'application/x-ndjson', '{"b":1}', 'pending', 5, datetime('now', '-1 second')),
                (?3, ?4, NULL, 'secureagnt_ndjson', 'mock://fail', 'application/x-ndjson', '{"c":1}', 'pending', 5, datetime('now', '-1 second'))
            "#,
        )
        .bind(delivery_a.to_string())
        .bind(delivery_b.to_string())
        .bind(delivery_c.to_string())
        .bind(tenant_id)
        .execute(sqlite)
        .await?;

        let claimed = claim_pending_compliance_siem_delivery_records_dual(
            &pool,
            "worker-a",
            std::time::Duration::from_secs(10),
            10,
        )
        .await?;
        assert_eq!(claimed.len(), 3);

        let _ = mark_compliance_siem_delivery_record_delivered_dual(&pool, delivery_a, Some(200))
            .await?;
        let _ = mark_compliance_siem_delivery_record_failed_dual(
            &pool,
            delivery_b,
            "temporary failure",
            Some(503),
            OffsetDateTime::now_utc() - time::Duration::seconds(1),
        )
        .await?;
        let _ = mark_compliance_siem_delivery_record_dead_lettered_dual(
            &pool,
            delivery_c,
            "permanent failure",
            Some(400),
        )
        .await?;

        let delivered_status: String =
            sqlx::query_scalar("SELECT status FROM compliance_siem_delivery_outbox WHERE id = ?1")
                .bind(delivery_a.to_string())
                .fetch_one(sqlite)
                .await?;
        let failed_status: String =
            sqlx::query_scalar("SELECT status FROM compliance_siem_delivery_outbox WHERE id = ?1")
                .bind(delivery_b.to_string())
                .fetch_one(sqlite)
                .await?;
        let dead_status: String =
            sqlx::query_scalar("SELECT status FROM compliance_siem_delivery_outbox WHERE id = ?1")
                .bind(delivery_c.to_string())
                .fetch_one(sqlite)
                .await?;
        assert_eq!(delivered_status, "delivered");
        assert_eq!(failed_status, "failed");
        assert_eq!(dead_status, "dead_lettered");

        Ok(())
    })
}

async fn seed_agent_and_user(
    pool: &SqlitePool,
    tenant_id: &str,
    agent_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO agents (id, tenant_id, name, status)
        VALUES (?1, ?2, ?3, 'active')
        "#,
    )
    .bind(agent_id.to_string())
    .bind(tenant_id)
    .bind("sqlite-agent")
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO users (id, tenant_id, external_subject, display_name, status)
        VALUES (?1, ?2, ?3, ?4, 'active')
        "#,
    )
    .bind(user_id.to_string())
    .bind(tenant_id)
    .bind("sqlite-user")
    .bind("SQLite User")
    .execute(pool)
    .await?;

    Ok(())
}

fn sqlite_pool(pool: &DbPool) -> Result<&SqlitePool, Box<dyn std::error::Error>> {
    match pool {
        DbPool::Sqlite(sqlite) => Ok(sqlite),
        DbPool::Postgres(_) => Err("expected sqlite pool".into()),
    }
}

fn run_async<F>(future: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(future)
}
