use core as agent_core;
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Row,
};
use std::{env, str::FromStr, time::Duration};
use uuid::Uuid;
use worker::{process_once, WorkerConfig, WorkerCycleOutcome};

struct TestDb {
    admin_pool: PgPool,
    app_pool: PgPool,
    schema: String,
}

#[test]
fn worker_process_once_completes_queued_run() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "queued"),
        )
        .await?;

        let config = WorkerConfig {
            worker_id: "worker-test-1".to_string(),
            lease_for: Duration::from_secs(30),
            requeue_limit: 10,
            poll_interval: Duration::from_millis(10),
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndCompleted { run_id });

        let run_row = sqlx::query(
            "SELECT status, attempts, lease_owner, lease_expires_at, finished_at FROM runs WHERE id = $1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        let status: String = run_row.get("status");
        let attempts: i32 = run_row.get("attempts");
        let lease_owner: Option<String> = run_row.get("lease_owner");
        let lease_expires_at: Option<time::OffsetDateTime> = run_row.get("lease_expires_at");
        let finished_at: Option<time::OffsetDateTime> = run_row.get("finished_at");

        assert_eq!(status, "succeeded");
        assert_eq!(attempts, 1);
        assert!(lease_owner.is_none());
        assert!(lease_expires_at.is_none());
        assert!(finished_at.is_some());

        let claimed_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM audit_events WHERE run_id = $1 AND event_type = 'run.claimed'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        let started_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM audit_events WHERE run_id = $1 AND event_type = 'run.processing_started'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        let completed_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM audit_events WHERE run_id = $1 AND event_type = 'run.completed'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;

        assert_eq!(claimed_count, 1);
        assert_eq!(started_count, 1);
        assert_eq!(completed_count, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn worker_process_once_requeues_stale_run_and_completes_it(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &new_run(run_id, agent_id, user_id, "running"),
        )
        .await?;

        sqlx::query(
            r#"
            UPDATE runs
            SET lease_owner = 'worker-stale',
                lease_expires_at = now() - interval '5 minutes'
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .execute(&test_db.app_pool)
        .await?;

        let config = WorkerConfig {
            worker_id: "worker-test-2".to_string(),
            lease_for: Duration::from_secs(30),
            requeue_limit: 10,
            poll_interval: Duration::from_millis(10),
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndCompleted { run_id });

        let status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(status, "succeeded");

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn worker_process_once_idle_when_no_work() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let config = WorkerConfig {
            worker_id: "worker-test-3".to_string(),
            lease_for: Duration::from_secs(30),
            requeue_limit: 10,
            poll_interval: Duration::from_millis(10),
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(
            outcome,
            WorkerCycleOutcome::Idle {
                requeued_expired_runs: 0
            }
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

async fn setup_test_db() -> Result<Option<TestDb>, Box<dyn std::error::Error>> {
    if !run_db_tests_enabled() {
        eprintln!("skipping worker integration test; set RUN_DB_TESTS=1 to enable");
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
        .max_connections(4)
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

async fn seed_agent_and_user(pool: &PgPool) -> Result<(Uuid, Uuid), sqlx::Error> {
    let agent_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO agents (id, tenant_id, name, status)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(agent_id)
    .bind("single")
    .bind("aegis_worker_test")
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
    .bind("single")
    .bind("worker:test:user")
    .bind("Worker Test User")
    .bind("active")
    .execute(pool)
    .await?;

    Ok((agent_id, user_id))
}

fn new_run(run_id: Uuid, agent_id: Uuid, user_id: Uuid, status: &str) -> agent_core::NewRun {
    agent_core::NewRun {
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
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(future)
}
