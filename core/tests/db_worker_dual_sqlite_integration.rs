use core::{
    claim_next_queued_run_dual, create_action_request_dual, create_action_result_dual,
    create_llm_token_usage_record_dual, create_or_get_payment_request_dual,
    create_payment_result_dual, create_run_dual, create_step_dual, get_run_status_dual,
    get_tenant_ops_summary_dual, persist_artifact_metadata_dual, renew_run_lease_dual,
    requeue_expired_runs_dual, sum_executed_payment_amount_msat_for_agent_dual,
    sum_executed_payment_amount_msat_for_tenant_dual, sum_llm_consumed_tokens_for_agent_since_dual,
    sum_llm_consumed_tokens_for_model_since_dual, sum_llm_consumed_tokens_for_tenant_since_dual,
    update_action_request_status_dual, update_payment_request_status_dual, DbPool,
    NewActionRequest, NewActionResult, NewArtifact, NewLlmTokenUsageRecord, NewPaymentRequest,
    NewPaymentResult, NewRun, NewStep,
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
