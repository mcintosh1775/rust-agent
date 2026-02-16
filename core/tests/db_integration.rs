use core::{
    append_audit_event, claim_next_queued_run, create_run, create_step, mark_run_succeeded,
    renew_run_lease, requeue_expired_runs, NewAuditEvent, NewRun, NewStep,
};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Row,
};
use std::time::Duration;
use std::{env, str::FromStr};
use uuid::Uuid;

const REQUIRED_TABLES: [&str; 8] = [
    "agents",
    "users",
    "runs",
    "steps",
    "artifacts",
    "action_requests",
    "action_results",
    "audit_events",
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
    .bind("aegis_local")
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
