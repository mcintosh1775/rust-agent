use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use core as agent_core;
use serde_json::{json, Value};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::{env, str::FromStr};
use tower::ServiceExt;
use uuid::Uuid;

struct TestDb {
    admin_pool: PgPool,
    app_pool: PgPool,
    schema: String,
}

#[test]
fn create_run_and_get_run_status() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"transcript_path": "podcasts/ep245/transcript.txt"},
                "requested_capabilities": [
                    {"capability": "object.read", "scope": "podcasts/*"}
                ]
            }),
        )?;

        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;

        let run_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing run id")?,
        )?;

        assert_eq!(
            create_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing status")?,
            "queued"
        );
        let granted = create_json
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;
        assert_eq!(granted.len(), 1);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing granted capability")?,
            "object.read"
        );
        assert_eq!(
            granted[0]
                .get("scope")
                .and_then(Value::as_str)
                .ok_or("missing granted scope")?,
            "podcasts/*"
        );

        let get_req = request_with_tenant(
            "GET",
            &format!("/v1/runs/{run_id}"),
            Some("single"),
            Value::Null,
        )?;
        let get_resp = app.clone().oneshot(get_req).await?;
        assert_eq!(get_resp.status(), StatusCode::OK);
        let run_json = response_json(get_resp).await?;

        assert_eq!(
            run_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing run id")?,
            run_id.to_string()
        );
        assert_eq!(
            run_json
                .get("tenant_id")
                .and_then(Value::as_str)
                .ok_or("missing tenant_id")?,
            "single"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_run_audit_returns_ordered_events() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let first_event_id = Uuid::new_v4();
        let second_event_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({"transcript_path":"podcasts/ep245/transcript.txt"}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;

        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
                id: first_event_id,
                run_id,
                step_id: None,
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "api".to_string(),
                event_type: "run.created".to_string(),
                payload_json: json!({"n":1}),
            },
        )
        .await?;

        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
                id: second_event_id,
                run_id,
                step_id: None,
                tenant_id: "single".to_string(),
                agent_id: Some(agent_id),
                user_id: Some(user_id),
                actor: "worker:1".to_string(),
                event_type: "run.claimed".to_string(),
                payload_json: json!({"n":2}),
            },
        )
        .await?;

        // Ensure deterministic ordering in assertion.
        sqlx::query(
            "UPDATE audit_events SET created_at = now() - interval '2 seconds' WHERE id = $1",
        )
        .bind(first_event_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "UPDATE audit_events SET created_at = now() - interval '1 seconds' WHERE id = $1",
        )
        .bind(second_event_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let audit_req = request_with_tenant(
            "GET",
            &format!("/v1/runs/{run_id}/audit?limit=10"),
            Some("single"),
            Value::Null,
        )?;
        let audit_resp = app.clone().oneshot(audit_req).await?;
        assert_eq!(audit_resp.status(), StatusCode::OK);

        let body = response_json(audit_resp).await?;
        let events = body.as_array().ok_or("audit body must be array")?;
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]
                .get("event_type")
                .and_then(Value::as_str)
                .ok_or("missing first event_type")?,
            "run.created"
        );
        assert_eq!(
            events[1]
                .get("event_type")
                .and_then(Value::as_str)
                .ok_or("missing second event_type")?,
            "run.claimed"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_filters_disallowed_capabilities() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"transcript_path": "podcasts/ep245/transcript.txt"},
                "requested_capabilities": [
                    {"capability": "object.write", "scope": "shownotes/*"},
                    {"capability": "message.send", "scope": "whitenoise:npub1target", "limits": {"max_payload_bytes": 50000}},
                    {"capability": "llm.infer", "scope": "local:*"},
                    {"capability": "local.exec", "scope": "local.exec:file.head"},
                    {"capability": "http.request", "scope": "api.github.com"},
                    {"capability": "object.write", "scope": "../etc/passwd"},
                    {"capability": "local.exec", "scope": "file.head"}
                ]
            }),
        )?;

        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;
        let granted = create_json
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;

        assert_eq!(granted.len(), 4);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 0")?,
            "object.write"
        );
        assert_eq!(
            granted[1]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 1")?,
            "message.send"
        );
        assert_eq!(
            granted[2]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 2")?,
            "llm.infer"
        );
        assert_eq!(
            granted[3]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 3")?,
            "local.exec"
        );
        let message_limit = granted[1]
            .get("limits")
            .and_then(|v| v.get("max_payload_bytes"))
            .and_then(Value::as_u64)
            .ok_or("missing message.send max_payload_bytes")?;
        assert_eq!(message_limit, 20_000);
        let llm_limit = granted[2]
            .get("limits")
            .and_then(|v| v.get("max_payload_bytes"))
            .and_then(Value::as_u64)
            .ok_or("missing llm.infer max_payload_bytes")?;
        assert_eq!(llm_limit, 32_000);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn tenant_header_is_required() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant(
            "POST",
            "/v1/runs",
            None,
            json!({
                "agent_id": agent_id,
                "recipe_id": "show_notes_v1",
                "input": {}
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_rejects_non_array_requested_capabilities() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "recipe_id": "show_notes_v1",
                "input": {},
                "requested_capabilities": {"capability":"object.read","scope":"podcasts/*"}
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

async fn setup_test_db() -> Result<Option<TestDb>, Box<dyn std::error::Error>> {
    if !run_db_tests_enabled() {
        eprintln!("skipping api integration test; set RUN_DB_TESTS=1 to enable");
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
    .bind("aegis_api_test")
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
    .bind("api:test:user")
    .bind("API Test User")
    .bind("active")
    .execute(pool)
    .await?;

    Ok((agent_id, user_id))
}

fn request_with_tenant(
    method: &str,
    uri: &str,
    tenant_id: Option<&str>,
    json_body: Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(tenant_id) = tenant_id {
        builder = builder.header("x-tenant-id", tenant_id);
    }

    let request = if method == "POST" {
        builder
            .header("content-type", "application/json")
            .body(Body::from(json_body.to_string()))?
    } else {
        builder.body(Body::empty())?
    };

    Ok(request)
}

async fn response_json(
    response: axum::response::Response,
) -> Result<Value, Box<dyn std::error::Error>> {
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
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
