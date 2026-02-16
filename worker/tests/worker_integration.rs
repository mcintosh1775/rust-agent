use core as agent_core;
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Row,
};
use std::{env, fs, path::PathBuf, str::FromStr, time::Duration};
use uuid::Uuid;
use worker::signer::{NostrSignerConfig, NostrSignerMode};
use worker::{process_once, WorkerConfig, WorkerCycleOutcome};

struct TestDb {
    admin_pool: PgPool,
    app_pool: PgPool,
    schema: String,
}

#[test]
fn worker_process_once_executes_skill_and_persists_actions(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_success");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_write": true
                }),
                requested_capabilities: json!([
                    {"capability":"object.write","scope":"shownotes/*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"object.write",
                        "scope":"shownotes/*",
                        "limits":{"max_payload_bytes":500000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let config = worker_test_config("worker-test-1", artifact_root.clone());
        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let run_row = sqlx::query("SELECT status, error_json, finished_at FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        let run_status: String = run_row.get("status");
        let run_error: Option<serde_json::Value> = run_row.get("error_json");
        assert_eq!(run_status, "succeeded");
        assert!(run_error.is_none());

        let step_row =
            sqlx::query("SELECT status, output_json, error_json FROM steps WHERE run_id = $1")
                .bind(run_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        let step_status: String = step_row.get("status");
        let step_output: Option<serde_json::Value> = step_row.get("output_json");
        let step_error: Option<serde_json::Value> = step_row.get("error_json");
        assert_eq!(step_status, "succeeded");
        assert!(step_error.is_none());
        assert!(step_output
            .as_ref()
            .and_then(|v| v.get("markdown"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .starts_with("# Summary"));

        let action_status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar JOIN steps s ON s.id = ar.step_id WHERE s.run_id = $1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(action_status, "executed");

        let action_result_status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_results ar JOIN action_requests aq ON aq.id = ar.action_request_id JOIN steps s ON s.id = aq.step_id WHERE s.run_id = $1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(action_result_status, "executed");

        let artifact_path = artifact_root.join("shownotes/ep245.md");
        assert!(artifact_path.exists());

        let executed_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM audit_events WHERE run_id = $1 AND event_type = 'action.executed'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(executed_count, 1);

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_denies_out_of_scope_action_and_fails_run(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_deny");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_write": true
                }),
                requested_capabilities: json!([
                    {"capability":"object.write","scope":"podcasts/*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"object.write",
                        "scope":"podcasts/*",
                        "limits":{"max_payload_bytes":500000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let config = worker_test_config("worker-test-2", artifact_root.clone());
        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let run_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(run_status, "failed");

        let step_status: String = sqlx::query_scalar("SELECT status FROM steps WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(step_status, "failed");

        let denied_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM action_requests ar JOIN steps s ON s.id = ar.step_id WHERE s.run_id = $1 AND ar.status = 'denied'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(denied_count, 1);

        let denied_result_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM action_results ar JOIN action_requests aq ON aq.id = ar.action_request_id JOIN steps s ON s.id = aq.step_id WHERE s.run_id = $1 AND ar.status = 'denied'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(denied_result_count, 1);

        let denied_audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM audit_events WHERE run_id = $1 AND event_type = 'action.denied'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(denied_audit_count, 1);

        let artifact_path = artifact_root.join("shownotes/ep245.md");
        assert!(!artifact_path.exists());

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_executes_whitenoise_message_send_with_local_signer(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_message_send_success");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "notify_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_message": true,
                    "destination": "whitenoise:npub1receiver"
                }),
                requested_capabilities: json!([
                    {"capability":"message.send","scope":"whitenoise:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"message.send",
                        "scope":"whitenoise:*",
                        "limits":{"max_payload_bytes":500000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-msg-1", artifact_root.clone());
        config.nostr_signer = local_signer_config();
        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let message_status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar JOIN steps s ON s.id = ar.step_id WHERE s.run_id = $1 AND ar.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(message_status, "executed");

        let outbox_path: String = sqlx::query_scalar(
            "SELECT path FROM artifacts WHERE run_id = $1 AND path LIKE 'messages/whitenoise/%' LIMIT 1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        let outbox_file = artifact_root.join(outbox_path);
        assert!(outbox_file.exists());

        let outbox_body = fs::read_to_string(outbox_file)?;
        assert!(outbox_body.contains("\"provider\": \"whitenoise\""));
        assert!(outbox_body.contains("\"nostr_public_key\": \"npub1"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_fails_whitenoise_message_send_without_signer(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_message_send_missing_signer");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "notify_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_message": true,
                    "destination": "whitenoise:npub1receiver"
                }),
                requested_capabilities: json!([
                    {"capability":"message.send","scope":"whitenoise:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"message.send",
                        "scope":"whitenoise:*",
                        "limits":{"max_payload_bytes":500000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let config = worker_test_config("worker-test-msg-2", artifact_root.clone());
        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let run_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&test_db.app_pool)
            .await?;
        assert_eq!(run_status, "failed");

        let message_status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar JOIN steps s ON s.id = ar.step_id WHERE s.run_id = $1 AND ar.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(message_status, "failed");

        let failed_result_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM action_results ar JOIN action_requests aq ON aq.id = ar.action_request_id JOIN steps s ON s.id = aq.step_id WHERE s.run_id = $1 AND ar.status = 'failed'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(failed_result_count, 1);

        let failed_audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM audit_events WHERE run_id = $1 AND event_type = 'action.failed'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(failed_audit_count, 1);

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_idle_when_no_work() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let config = worker_test_config("worker-test-3", temp_artifact_root("worker_idle"));
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

fn local_signer_config() -> NostrSignerConfig {
    NostrSignerConfig {
        mode: NostrSignerMode::LocalKey,
        local_secret_key: Some(
            "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        ),
        local_secret_key_file: None,
        nip46_bunker_uri: None,
        nip46_public_key: None,
    }
}

fn worker_test_config(worker_id: &str, artifact_root: PathBuf) -> WorkerConfig {
    let skill_script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../skills/python/summarize_transcript/main.py")
        .to_string_lossy()
        .to_string();
    WorkerConfig {
        worker_id: worker_id.to_string(),
        lease_for: Duration::from_secs(30),
        requeue_limit: 10,
        poll_interval: Duration::from_millis(10),
        skill_command: "python3".to_string(),
        skill_args: vec![skill_script],
        skill_timeout: Duration::from_secs(3),
        skill_max_output_bytes: 64 * 1024,
        artifact_root,
        nostr_signer: NostrSignerConfig::default(),
    }
}

fn temp_artifact_root(suffix: &str) -> PathBuf {
    env::temp_dir().join(format!("aegis_{}_{}", suffix, Uuid::new_v4()))
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
