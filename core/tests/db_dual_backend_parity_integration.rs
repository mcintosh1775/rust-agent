use core::{
    append_audit_event_dual, create_run_dual, create_step_dual, get_run_status_dual,
    get_tenant_ops_summary_dual, list_run_audit_events_dual, mark_run_succeeded_dual,
    mark_step_succeeded_dual, DbPool, NewAuditEvent, NewRun, NewStep,
};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::{env, str::FromStr};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DualFlowSnapshot {
    run_status: String,
    audit_event_types: Vec<String>,
    queued_runs: i64,
    running_runs: i64,
    succeeded_runs_window: i64,
    failed_runs_window: i64,
}

struct TestDb {
    admin_pool: PgPool,
    app_pool: PgPool,
    schema: String,
}

#[test]
fn db_dual_sqlite_and_postgres_run_flow_parity() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let sqlite_pool = DbPool::connect("sqlite::memory:", 1).await?;
        sqlite_pool.migrate().await?;

        let sqlite_snapshot = run_dual_flow_and_snapshot(&sqlite_pool, "single").await?;

        let pg_pool = DbPool::Postgres(test_db.app_pool.clone());
        let postgres_snapshot = run_dual_flow_and_snapshot(&pg_pool, "single").await?;

        assert_eq!(sqlite_snapshot, postgres_snapshot);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

async fn run_dual_flow_and_snapshot(
    pool: &DbPool,
    tenant_id: &str,
) -> Result<DualFlowSnapshot, Box<dyn std::error::Error>> {
    let agent_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let step_id = Uuid::new_v4();

    seed_agent_and_user_dual(pool, tenant_id, agent_id, user_id).await?;

    create_run_dual(
        pool,
        &NewRun {
            id: run_id,
            tenant_id: tenant_id.to_string(),
            agent_id,
            triggered_by_user_id: Some(user_id),
            recipe_id: "show_notes_v1".to_string(),
            input_json: json!({"text":"parity-check"}),
            requested_capabilities: json!([]),
            granted_capabilities: json!([]),
            status: "running".to_string(),
            error_json: None,
        },
    )
    .await?;

    create_step_dual(
        pool,
        &NewStep {
            id: step_id,
            run_id,
            tenant_id: tenant_id.to_string(),
            agent_id,
            user_id: Some(user_id),
            name: "skill.invoke".to_string(),
            status: "running".to_string(),
            input_json: json!({"text":"parity-check"}),
            error_json: None,
        },
    )
    .await?;

    let step_updated = mark_step_succeeded_dual(pool, step_id, json!({"ok": true})).await?;
    assert!(step_updated);

    append_audit_event_dual(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id,
            step_id: Some(step_id),
            tenant_id: tenant_id.to_string(),
            agent_id: Some(agent_id),
            user_id: Some(user_id),
            actor: "worker".to_string(),
            event_type: "run.succeeded".to_string(),
            payload_json: json!({"backend":"dual"}),
        },
    )
    .await?;

    let run_updated = mark_run_succeeded_dual(pool, run_id, "worker-parity").await?;
    assert!(run_updated);

    let run_status = get_run_status_dual(pool, tenant_id, run_id)
        .await?
        .ok_or("missing run status after mark_run_succeeded_dual")?;
    let audit_events = list_run_audit_events_dual(pool, tenant_id, run_id, 100).await?;
    let mut audit_event_types = audit_events
        .iter()
        .map(|event| event.event_type.clone())
        .collect::<Vec<_>>();
    audit_event_types.sort();

    let ops = get_tenant_ops_summary_dual(
        pool,
        tenant_id,
        OffsetDateTime::now_utc() - time::Duration::hours(1),
    )
    .await?;

    Ok(DualFlowSnapshot {
        run_status: run_status.status,
        audit_event_types,
        queued_runs: ops.queued_runs,
        running_runs: ops.running_runs,
        succeeded_runs_window: ops.succeeded_runs_window,
        failed_runs_window: ops.failed_runs_window,
    })
}

async fn seed_agent_and_user_dual(
    pool: &DbPool,
    tenant_id: &str,
    agent_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    match pool {
        DbPool::Postgres(pg) => {
            sqlx::query(
                r#"
                INSERT INTO agents (id, tenant_id, name, status)
                VALUES ($1, $2, $3, 'active')
                "#,
            )
            .bind(agent_id)
            .bind(tenant_id)
            .bind("parity-agent")
            .execute(pg)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO users (id, tenant_id, external_subject, display_name, status)
                VALUES ($1, $2, $3, $4, 'active')
                "#,
            )
            .bind(user_id)
            .bind(tenant_id)
            .bind("parity-user")
            .bind("Parity User")
            .execute(pg)
            .await?;
        }
        DbPool::Sqlite(sqlite) => {
            sqlx::query(
                r#"
                INSERT INTO agents (id, tenant_id, name, status)
                VALUES (?1, ?2, ?3, 'active')
                "#,
            )
            .bind(agent_id.to_string())
            .bind(tenant_id)
            .bind("parity-agent")
            .execute(sqlite)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO users (id, tenant_id, external_subject, display_name, status)
                VALUES (?1, ?2, ?3, ?4, 'active')
                "#,
            )
            .bind(user_id.to_string())
            .bind(tenant_id)
            .bind("parity-user")
            .bind("Parity User")
            .execute(sqlite)
            .await?;
        }
    }

    Ok(())
}

async fn setup_test_db() -> Result<Option<TestDb>, Box<dyn std::error::Error>> {
    if !run_db_tests_enabled() {
        return Ok(None);
    }

    let database_url = test_database_url();
    let admin_options = PgConnectOptions::from_str(&database_url)?;
    let admin_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(admin_options)
        .await?;

    let schema = format!("test_dual_parity_{}", Uuid::new_v4().simple());
    let create_schema_sql = format!("CREATE SCHEMA {}", schema);
    sqlx::query(&create_schema_sql).execute(&admin_pool).await?;

    let app_options =
        PgConnectOptions::from_str(&database_url)?.options([("search_path", &schema)]);
    let app_pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(app_options)
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

fn run_async<F>(future: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(future)
}
