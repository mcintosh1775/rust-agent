use core::{
    append_audit_event_dual, create_run_dual, create_step_dual, get_run_status_dual,
    get_tenant_ops_summary_dual, list_run_audit_events_dual, mark_run_succeeded_dual,
    mark_step_succeeded_dual, DbPool, NewAuditEvent, NewRun, NewStep,
};
use serde_json::json;
use sqlx::SqlitePool;
use time::OffsetDateTime;
use uuid::Uuid;

#[test]
fn db_dual_sqlite_run_step_audit_ops_summary_flow() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let pool = DbPool::connect("sqlite::memory:", 1).await?;
        pool.migrate().await?;

        let sqlite = sqlite_pool(&pool)?;

        let tenant_id = "solo";
        let agent_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();

        seed_agent_and_user(sqlite, tenant_id, agent_id, user_id).await?;

        let run = create_run_dual(
            &pool,
            &NewRun {
                id: run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                input_json: json!({"text":"hello sqlite"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                status: "running".to_string(),
                error_json: None,
            },
        )
        .await?;
        assert_eq!(run.id, run_id);

        let step = create_step_dual(
            &pool,
            &NewStep {
                id: step_id,
                run_id,
                tenant_id: tenant_id.to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "skill.invoke".to_string(),
                status: "running".to_string(),
                input_json: json!({"text":"hello sqlite"}),
                error_json: None,
            },
        )
        .await?;
        assert_eq!(step.id, step_id);

        let step_marked =
            mark_step_succeeded_dual(&pool, step_id, json!({"markdown":"# Summary"})).await?;
        assert!(step_marked);

        append_audit_event_dual(
            &pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id,
                step_id: Some(step_id),
                tenant_id: tenant_id.to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker".to_string(),
                event_type: "run.succeeded".to_string(),
                payload_json: json!({"ok":true}),
            },
        )
        .await?;

        let run_marked = mark_run_succeeded_dual(&pool, run_id, "worker-1").await?;
        assert!(run_marked);

        let status = get_run_status_dual(&pool, tenant_id, run_id)
            .await?
            .expect("run status should exist");
        assert_eq!(status.status, "succeeded");

        let audit_events = list_run_audit_events_dual(&pool, tenant_id, run_id, 100).await?;
        assert_eq!(audit_events.len(), 1);
        assert_eq!(audit_events[0].event_type, "run.succeeded");

        let ops = get_tenant_ops_summary_dual(
            &pool,
            tenant_id,
            OffsetDateTime::now_utc() - time::Duration::hours(1),
        )
        .await?;
        assert_eq!(ops.queued_runs, 0);
        assert_eq!(ops.running_runs, 0);
        assert_eq!(ops.succeeded_runs_window, 1);
        assert_eq!(ops.failed_runs_window, 0);

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
