use axum::{
    body::{to_bytes, Body},
    http::{HeaderValue, Request, StatusCode},
};
use core as agent_core;
use serde_json::{json, Value};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Row,
};
use std::{env, fs, path::PathBuf, str::FromStr};
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
fn console_index_route_serves_html_shell() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router(test_db.app_pool.clone());
        let req = Request::builder()
            .method("GET")
            .uri("/console")
            .body(Body::empty())?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.starts_with("text/html"),
            "unexpected content-type: {content_type}"
        );

        let body = to_bytes(resp.into_body(), 1024 * 1024).await?;
        let body_text = String::from_utf8(body.to_vec())?;
        assert!(body_text.contains("SecureAgnt Operations Console"));
        assert!(body_text.contains("<option value=\"viewer\">viewer</option>"));
        assert!(body_text.contains("x-user-role"));
        assert!(body_text.contains("ROLE_FORBIDDEN"));
        assert!(body_text.contains("Run Latency Traces"));
        assert!(body_text.contains("Load Run Context"));
        assert!(body_text.contains("secureagnt_console_controls_v1"));
        assert!(body_text.contains("Export Snapshot JSON"));
        assert!(body_text.contains("Export Health JSON"));
        assert!(body_text.contains("threshold-chips"));
        assert!(body_text.contains("INPUT_REQUIRED"));
        assert!(body_text.contains("FETCH_FAILED"));
        assert!(body_text.contains("FORBIDDEN"));
        assert!(body_text.contains("x-auth-proxy-token"));
        assert!(body_text.contains("Auth Proxy Token"));
        assert!(body_text.contains("x-user-id"));
        assert!(body_text.contains("Acknowledge Alert"));
        assert!(body_text.contains("/v1/audit/compliance/siem/deliveries/alerts/ack"));

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn agent_context_inspect_and_heartbeat_compile_endpoints_work(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let context_root = make_temp_context_root("inspect_compile");
        let source_dir = context_root.join("single").join(agent_id.to_string());
        fs::create_dir_all(&source_dir)?;
        fs::write(source_dir.join("SOUL.md"), "You are calm and precise.\n")?;
        fs::write(
            source_dir.join("HEARTBEAT.md"),
            "- every 900 recipe=show_notes_v1 max_inflight=2 jitter=5\n",
        )?;

        let app = api::app_router_with_agent_context_config(
            test_db.app_pool.clone(),
            agent_core::AgentContextLoaderConfig {
                root_dir: context_root.clone(),
                required_files: vec!["SOUL.md".to_string(), "HEARTBEAT.md".to_string()],
                max_file_bytes: 64 * 1024,
                max_total_bytes: 256 * 1024,
                max_dynamic_files_per_dir: 4,
            },
            false,
        );

        let inspect_req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/agents/{agent_id}/context"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let inspect_resp = app.clone().oneshot(inspect_req).await?;
        assert_eq!(inspect_resp.status(), StatusCode::OK);
        let inspect_json = response_json(inspect_resp).await?;
        assert_eq!(
            inspect_json
                .get("loaded_file_count")
                .and_then(Value::as_u64)
                .ok_or("missing loaded_file_count")?,
            2
        );
        assert_eq!(
            inspect_json
                .get("summary_digest_sha256")
                .and_then(Value::as_str)
                .ok_or("missing summary_digest_sha256")?
                .len(),
            64
        );

        let compile_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/agents/{agent_id}/heartbeat/compile"),
            Some("single"),
            Some("operator"),
            json!({}),
        )?;
        let compile_resp = app.clone().oneshot(compile_req).await?;
        assert_eq!(compile_resp.status(), StatusCode::OK);
        let compile_json = response_json(compile_resp).await?;
        assert_eq!(
            compile_json
                .get("candidate_count")
                .and_then(Value::as_u64)
                .ok_or("missing candidate_count")?,
            1
        );
        assert_eq!(
            compile_json
                .get("issue_count")
                .and_then(Value::as_u64)
                .ok_or("missing issue_count")?,
            0
        );

        let viewer_compile_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/agents/{agent_id}/heartbeat/compile"),
            Some("single"),
            Some("viewer"),
            json!({}),
        )?;
        let viewer_compile_resp = app.clone().oneshot(viewer_compile_req).await?;
        assert_eq!(viewer_compile_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(context_root);
        Ok(())
    })
}

#[test]
fn agent_context_mutation_enforces_mutability_boundaries() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let context_root = make_temp_context_root("mutability");
        let source_dir = context_root.join("single").join(agent_id.to_string());
        fs::create_dir_all(source_dir.join("sessions"))?;
        fs::write(source_dir.join("SOUL.md"), "immutable soul\n")?;
        fs::write(source_dir.join("USER.md"), "owner notes\n")?;

        let app = api::app_router_with_agent_context_config(
            test_db.app_pool.clone(),
            agent_core::AgentContextLoaderConfig {
                root_dir: context_root.clone(),
                required_files: vec!["SOUL.md".to_string(), "USER.md".to_string()],
                max_file_bytes: 64 * 1024,
                max_total_bytes: 256 * 1024,
                max_dynamic_files_per_dir: 4,
            },
            true,
        );

        let immutable_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/agents/{agent_id}/context"),
            Some("single"),
            Some("owner"),
            json!({
                "relative_path": "SOUL.md",
                "content": "new",
                "mode": "replace"
            }),
        )?;
        let immutable_resp = app.clone().oneshot(immutable_req).await?;
        assert_eq!(immutable_resp.status(), StatusCode::FORBIDDEN);

        let human_primary_operator_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/agents/{agent_id}/context"),
            Some("single"),
            Some("operator"),
            json!({
                "relative_path": "USER.md",
                "content": "operator tried edit",
                "mode": "replace"
            }),
        )?;
        let human_primary_operator_resp = app.clone().oneshot(human_primary_operator_req).await?;
        assert_eq!(human_primary_operator_resp.status(), StatusCode::FORBIDDEN);

        let sessions_replace_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/agents/{agent_id}/context"),
            Some("single"),
            Some("owner"),
            json!({
                "relative_path": "sessions/day-1.jsonl",
                "content": "{\"event\":\"x\"}",
                "mode": "replace"
            }),
        )?;
        let sessions_replace_resp = app.clone().oneshot(sessions_replace_req).await?;
        assert_eq!(sessions_replace_resp.status(), StatusCode::BAD_REQUEST);

        let sessions_append_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/agents/{agent_id}/context"),
            Some("single"),
            Some("owner"),
            json!({
                "relative_path": "sessions/day-1.jsonl",
                "content": "{\"event\":\"x\"}",
                "mode": "append"
            }),
        )?;
        let sessions_append_resp = app.clone().oneshot(sessions_append_req).await?;
        assert_eq!(sessions_append_resp.status(), StatusCode::OK);
        let sessions_contents = fs::read_to_string(source_dir.join("sessions/day-1.jsonl"))?;
        assert!(sessions_contents.ends_with('\n'));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(context_root);
        Ok(())
    })
}

#[test]
fn create_run_enforces_tenant_inflight_capacity_limit() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let existing_run_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'show_notes_v1', 'queued', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(existing_run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router_with_tenant_limit(test_db.app_pool.clone(), Some(1));
        let create_req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"transcript_path": "podcasts/ep245/transcript.txt"},
                "requested_capabilities": []
            }),
        )?;

        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = response_json(create_resp).await?;
        assert_eq!(
            body.get("error")
                .and_then(|v| v.get("code"))
                .and_then(Value::as_str)
                .ok_or("missing error.code")?,
            "TENANT_INFLIGHT_LIMITED"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn run_and_audit_endpoints_are_tenant_isolated() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'show_notes_v1', 'queued', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (id, run_id, step_id, tenant_id, agent_id, user_id, actor, event_type, payload_json) VALUES ($1, $2, null, 'single', $3, $4, 'api', 'run.created', '{}'::jsonb)",
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());

        let get_run_req = request_with_tenant(
            "GET",
            &format!("/v1/runs/{run_id}"),
            Some("other"),
            Value::Null,
        )?;
        let get_run_resp = app.clone().oneshot(get_run_req).await?;
        assert_eq!(get_run_resp.status(), StatusCode::NOT_FOUND);

        let get_audit_req = request_with_tenant(
            "GET",
            &format!("/v1/runs/{run_id}/audit?limit=10"),
            Some("other"),
            Value::Null,
        )?;
        let get_audit_resp = app.clone().oneshot(get_audit_req).await?;
        assert_eq!(get_audit_resp.status(), StatusCode::NOT_FOUND);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_trigger_with_role_preset_persists_record() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant_and_role_and_user(
            "POST",
            "/v1/triggers",
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"transcript_path": "podcasts/ep245/transcript.txt"},
                "requested_capabilities": [],
                "interval_seconds": 60
            }),
        )?;

        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;

        assert_eq!(
            create_json
                .get("trigger_type")
                .and_then(Value::as_str)
                .ok_or("missing trigger_type")?,
            "interval"
        );
        assert_eq!(
            create_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing status")?,
            "enabled"
        );
        assert_eq!(
            create_json
                .get("interval_seconds")
                .and_then(Value::as_i64)
                .ok_or("missing interval_seconds")?,
            60
        );
        let granted = create_json
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;
        assert_eq!(granted.len(), 4);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing granted capability 0")?,
            "object.read"
        );
        assert_eq!(
            granted[3]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing granted capability 3")?,
            "llm.infer"
        );

        let trigger_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing trigger id")?,
        )?;
        let persisted: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM triggers WHERE id = $1")
                .bind(trigger_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        assert_eq!(persisted, 1);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_trigger_rejects_viewer_role() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant_and_role(
            "POST",
            "/v1/triggers",
            Some("single"),
            Some("viewer"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"transcript_path": "podcasts/ep245/transcript.txt"},
                "requested_capabilities": [],
                "interval_seconds": 60
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_trigger_rejects_operator_without_user_id_header() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant_and_role(
            "POST",
            "/v1/triggers",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"transcript_path": "podcasts/ep245/transcript.txt"},
                "requested_capabilities": [],
                "interval_seconds": 60
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_trigger_rejects_invalid_interval() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant(
            "POST",
            "/v1/triggers",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {},
                "interval_seconds": 0
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_trigger_enforces_tenant_trigger_capacity_limit() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO triggers (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                trigger_type, interval_seconds, input_json, requested_capabilities,
                granted_capabilities, next_fire_at
            )
            VALUES (
                $1, 'single', $2, $3, 'show_notes_v1', 'enabled',
                'interval', 60, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, now() + interval '60 seconds'
            )
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router_with_limits(test_db.app_pool.clone(), None, Some(1));
        let req = request_with_tenant(
            "POST",
            "/v1/triggers",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {},
                "requested_capabilities": [],
                "interval_seconds": 60
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("error")
                .and_then(|value| value.get("code"))
                .and_then(Value::as_str)
                .ok_or("missing error.code")?,
            "TENANT_TRIGGER_LIMITED"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_memory_record_enforces_tenant_memory_capacity_limit(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        agent_core::create_memory_record(
            &test_db.app_pool,
            &agent_core::NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "semantic".to_string(),
                scope: "memory:project/quota".to_string(),
                content_json: json!({"seed":"existing"}),
                summary_text: Some("existing".to_string()),
                source: "seed".to_string(),
                redaction_applied: false,
                expires_at: Some(time::OffsetDateTime::now_utc() + time::Duration::hours(1)),
            },
        )
        .await?;

        let app = api::app_router_with_memory_limit(test_db.app_pool.clone(), Some(1));
        let req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "memory_kind": "semantic",
                "scope": "memory:project/quota",
                "content_json": {"note":"new"}
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("error")
                .and_then(|value| value.get("code"))
                .and_then(Value::as_str)
                .ok_or("missing error.code")?,
            "TENANT_MEMORY_LIMITED"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn trigger_mutation_endpoints_are_tenant_isolated() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO triggers (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                trigger_type, interval_seconds, input_json, requested_capabilities,
                granted_capabilities, next_fire_at
            )
            VALUES (
                $1, 'single', $2, $3, 'show_notes_v1', 'enabled',
                'interval', 60, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, now() + interval '60 seconds'
            )
            "#,
        )
        .bind(trigger_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());

        let patch_req = request_with_tenant(
            "PATCH",
            &format!("/v1/triggers/{trigger_id}"),
            Some("other"),
            json!({"max_attempts": 4}),
        )?;
        let patch_resp = app.clone().oneshot(patch_req).await?;
        assert_eq!(patch_resp.status(), StatusCode::NOT_FOUND);

        let disable_req = request_with_tenant(
            "POST",
            &format!("/v1/triggers/{trigger_id}/disable"),
            Some("other"),
            Value::Null,
        )?;
        let disable_resp = app.clone().oneshot(disable_req).await?;
        assert_eq!(disable_resp.status(), StatusCode::NOT_FOUND);

        let fire_req = request_with_tenant(
            "POST",
            &format!("/v1/triggers/{trigger_id}/fire"),
            Some("other"),
            json!({"idempotency_key":"tenant-isolation-fire-001"}),
        )?;
        let fire_resp = app.clone().oneshot(fire_req).await?;
        assert_eq!(fire_resp.status(), StatusCode::NOT_FOUND);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_cron_trigger_persists_record() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant_and_role_and_user(
            "POST",
            "/v1/triggers/cron",
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"source":"cron"},
                "requested_capabilities": [],
                "cron_expression": "0/1 * * * * * *",
                "schedule_timezone": "UTC",
                "max_attempts": 3
            }),
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("trigger_type")
                .and_then(Value::as_str)
                .ok_or("missing trigger_type")?,
            "cron"
        );
        assert_eq!(
            body.get("schedule_timezone")
                .and_then(Value::as_str)
                .ok_or("missing schedule_timezone")?,
            "UTC"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn update_and_toggle_trigger_status() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant_and_role_and_user(
            "POST",
            "/v1/triggers",
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"source":"update"},
                "requested_capabilities": [],
                "interval_seconds": 60
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;
        let trigger_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing trigger id")?,
        )?;

        let patch_req = request_with_tenant_and_role_and_user(
            "PATCH",
            &format!("/v1/triggers/{trigger_id}"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({
                "interval_seconds": 120,
                "max_inflight_runs": 2
            }),
        )?;
        let patch_resp = app.clone().oneshot(patch_req).await?;
        assert_eq!(patch_resp.status(), StatusCode::OK);
        let patch_json = response_json(patch_resp).await?;
        assert_eq!(
            patch_json
                .get("interval_seconds")
                .and_then(Value::as_i64)
                .ok_or("missing interval_seconds")?,
            120
        );
        assert_eq!(
            patch_json
                .get("max_inflight_runs")
                .and_then(Value::as_i64)
                .ok_or("missing max_inflight_runs")?,
            2
        );

        let disable_req = request_with_tenant_and_role_and_user(
            "POST",
            &format!("/v1/triggers/{trigger_id}/disable"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({}),
        )?;
        let disable_resp = app.clone().oneshot(disable_req).await?;
        assert_eq!(disable_resp.status(), StatusCode::OK);
        let disable_json = response_json(disable_resp).await?;
        assert_eq!(
            disable_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing disabled status")?,
            "disabled"
        );

        let enable_req = request_with_tenant_and_role_and_user(
            "POST",
            &format!("/v1/triggers/{trigger_id}/enable"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({}),
        )?;
        let enable_resp = app.clone().oneshot(enable_req).await?;
        assert_eq!(enable_resp.status(), StatusCode::OK);
        let enable_json = response_json(enable_resp).await?;
        assert_eq!(
            enable_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing enabled status")?,
            "enabled"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn webhook_trigger_accepts_events_with_secret_and_dedupes_event_id(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());
        std::env::set_var("SECUREAGNT_TRIGGER_SECRET_TEST", "super-secret");

        let create_req = request_with_tenant(
            "POST",
            "/v1/triggers/webhook",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"request_write": false},
                "requested_capabilities": [],
                "webhook_secret_ref": "env:SECUREAGNT_TRIGGER_SECRET_TEST",
                "max_attempts": 3
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;
        let trigger_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing trigger id")?,
        )?;

        let wrong_secret_req = request_with_tenant_and_role_and_secret(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events"),
            Some("single"),
            None,
            Some("wrong"),
            json!({
                "event_id": "evt-001",
                "payload": {"hello":"world"}
            }),
        )?;
        let wrong_secret_resp = app.clone().oneshot(wrong_secret_req).await?;
        assert_eq!(wrong_secret_resp.status(), StatusCode::UNAUTHORIZED);

        let ingest_req = request_with_tenant_and_role_and_secret(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events"),
            Some("single"),
            None,
            Some("super-secret"),
            json!({
                "event_id": "evt-001",
                "payload": {"hello":"world"}
            }),
        )?;
        let ingest_resp = app.clone().oneshot(ingest_req).await?;
        assert_eq!(ingest_resp.status(), StatusCode::ACCEPTED);
        let ingest_json = response_json(ingest_resp).await?;
        assert_eq!(
            ingest_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing ingest status")?,
            "queued"
        );

        let duplicate_req = request_with_tenant_and_role_and_secret(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events"),
            Some("single"),
            None,
            Some("super-secret"),
            json!({
                "event_id": "evt-001",
                "payload": {"hello":"world"}
            }),
        )?;
        let duplicate_resp = app.clone().oneshot(duplicate_req).await?;
        assert_eq!(duplicate_resp.status(), StatusCode::ACCEPTED);
        let duplicate_json = response_json(duplicate_resp).await?;
        assert_eq!(
            duplicate_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing duplicate status")?,
            "duplicate"
        );

        std::env::remove_var("SECUREAGNT_TRIGGER_SECRET_TEST");
        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn manual_trigger_fire_creates_run_and_dedupes() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant(
            "POST",
            "/v1/triggers",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"from":"manual-fire"},
                "requested_capabilities": [],
                "interval_seconds": 60
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;
        let trigger_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing trigger id")?,
        )?;

        let fire_req = request_with_tenant_and_role_and_user(
            "POST",
            &format!("/v1/triggers/{trigger_id}/fire"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({
                "idempotency_key": "manual-001",
                "payload": {"kind":"adhoc"}
            }),
        )?;
        let fire_resp = app.clone().oneshot(fire_req).await?;
        assert_eq!(fire_resp.status(), StatusCode::ACCEPTED);
        let fire_json = response_json(fire_resp).await?;
        assert_eq!(
            fire_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing fire status")?,
            "created"
        );
        let first_run_id = fire_json
            .get("run_id")
            .and_then(Value::as_str)
            .ok_or("missing run_id")?
            .to_string();

        let duplicate_req = request_with_tenant_and_role_and_user(
            "POST",
            &format!("/v1/triggers/{trigger_id}/fire"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({
                "idempotency_key": "manual-001",
                "payload": {"kind":"adhoc"}
            }),
        )?;
        let duplicate_resp = app.clone().oneshot(duplicate_req).await?;
        assert_eq!(duplicate_resp.status(), StatusCode::OK);
        let duplicate_json = response_json(duplicate_resp).await?;
        assert_eq!(
            duplicate_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing duplicate status")?,
            "duplicate"
        );
        assert_eq!(
            duplicate_json
                .get("run_id")
                .and_then(Value::as_str)
                .ok_or("missing duplicate run_id")?,
            first_run_id
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn webhook_trigger_event_ingest_returns_conflict_when_trigger_is_disabled(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant(
            "POST",
            "/v1/triggers/webhook",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"request_write": false},
                "requested_capabilities": [],
                "max_attempts": 3
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;
        let trigger_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing trigger id")?,
        )?;

        let disable_req = request_with_tenant(
            "POST",
            &format!("/v1/triggers/{trigger_id}/disable"),
            Some("single"),
            json!({}),
        )?;
        let disable_resp = app.clone().oneshot(disable_req).await?;
        assert_eq!(disable_resp.status(), StatusCode::OK);

        let ingest_req = request_with_tenant(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events"),
            Some("single"),
            json!({
                "event_id": "evt-disabled-1",
                "payload": {"hello":"world"}
            }),
        )?;
        let ingest_resp = app.clone().oneshot(ingest_req).await?;
        assert_eq!(ingest_resp.status(), StatusCode::CONFLICT);
        let ingest_json = response_json(ingest_resp).await?;
        assert_eq!(
            ingest_json
                .get("error")
                .and_then(|value| value.get("code"))
                .and_then(Value::as_str)
                .ok_or("missing error.code")?,
            "CONFLICT"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn replay_dead_lettered_trigger_event_requeues_event() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant_and_role_and_user(
            "POST",
            "/v1/triggers/webhook",
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"request_write": false},
                "requested_capabilities": [],
                "max_attempts": 3
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;
        let trigger_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing trigger id")?,
        )?;

        let ingest_req = request_with_tenant(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events"),
            Some("single"),
            json!({
                "event_id": "evt-replay-1",
                "payload": {"hello":"world"}
            }),
        )?;
        let ingest_resp = app.clone().oneshot(ingest_req).await?;
        assert_eq!(ingest_resp.status(), StatusCode::ACCEPTED);

        let replay_before_dead_letter = request_with_tenant_and_role_and_user(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events/{}/replay", "evt-replay-1"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({}),
        )?;
        let replay_before_dead_letter_resp = app.clone().oneshot(replay_before_dead_letter).await?;
        assert_eq!(
            replay_before_dead_letter_resp.status(),
            StatusCode::CONFLICT
        );

        sqlx::query(
            r#"
            UPDATE trigger_events
            SET status = 'dead_lettered',
                attempts = 3,
                next_attempt_at = now() + interval '10 minutes',
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

        let replay_req = request_with_tenant_and_role_and_user(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events/{}/replay", "evt-replay-1"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({}),
        )?;
        let replay_resp = app.clone().oneshot(replay_req).await?;
        assert_eq!(replay_resp.status(), StatusCode::ACCEPTED);
        let replay_json = response_json(replay_resp).await?;
        assert_eq!(
            replay_json
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing replay status")?,
            "queued_for_replay"
        );

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
        let last_error: Option<Value> = row.get("last_error_json");
        let dead_lettered_at: Option<time::OffsetDateTime> = row.get("dead_lettered_at");
        assert_eq!(status, "pending");
        assert_eq!(attempts, 0);
        assert!(last_error.is_none());
        assert!(dead_lettered_at.is_none());

        let replay_again_req = request_with_tenant_and_role_and_user(
            "POST",
            &format!("/v1/triggers/{trigger_id}/events/{}/replay", "evt-replay-1"),
            Some("single"),
            Some("operator"),
            Some(user_id),
            json!({}),
        )?;
        let replay_again_resp = app.clone().oneshot(replay_again_req).await?;
        assert_eq!(replay_again_resp.status(), StatusCode::CONFLICT);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn manual_trigger_fire_rejects_viewer_role() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant(
            "POST",
            "/v1/triggers",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"from":"manual-fire"},
                "requested_capabilities": [],
                "interval_seconds": 60
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let create_json = response_json(create_resp).await?;
        let trigger_id = Uuid::parse_str(
            create_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing trigger id")?,
        )?;

        let fire_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/triggers/{trigger_id}/fire"),
            Some("single"),
            Some("viewer"),
            json!({"idempotency_key": "manual-001"}),
        )?;
        let fire_resp = app.clone().oneshot(fire_req).await?;
        assert_eq!(fire_resp.status(), StatusCode::FORBIDDEN);

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
fn get_llm_usage_tokens_returns_aggregates_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        let action_request_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'llm_remote_v1', 'succeeded', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO steps (id, run_id, tenant_id, agent_id, user_id, name, status, input_json) VALUES ($1, $2, 'single', $3, $4, 'llm', 'succeeded', '{}'::jsonb)",
        )
        .bind(step_id)
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO action_requests (id, step_id, action_type, args_json, status) VALUES ($1, $2, 'llm.infer', '{}'::jsonb, 'executed')",
        )
        .bind(action_request_id)
        .bind(step_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
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
            VALUES ($1, $2, $3, 'single', $4, 'remote', 'remote:mock-remote-model', 123, 0.0042, now() - interval '10 minutes', 3600)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(action_request_id)
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let usage_req = request_with_tenant_and_role(
            "GET",
            "/v1/usage/llm/tokens?window_secs=3600",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let usage_resp = app.clone().oneshot(usage_req).await?;
        assert_eq!(usage_resp.status(), StatusCode::OK);
        let usage_json = response_json(usage_resp).await?;
        assert_eq!(
            usage_json
                .get("tokens")
                .and_then(Value::as_i64)
                .ok_or("missing tokens")?,
            123
        );

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/usage/llm/tokens?window_secs=3600",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_ops_summary_returns_counts_and_enforces_role() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;

        let queued_run_id = Uuid::new_v4();
        let running_run_id = Uuid::new_v4();
        let succeeded_run_id = Uuid::new_v4();
        let failed_run_id = Uuid::new_v4();

        for (run_id, status, started_offset_s, finished_offset_s) in [
            (queued_run_id, "queued", None, None),
            (running_run_id, "running", Some(20), None),
            (succeeded_run_id, "succeeded", Some(120), Some(90)),
            (failed_run_id, "failed", Some(300), Some(240)),
        ] {
            sqlx::query(
                r#"
                INSERT INTO runs (
                    id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                    input_json, requested_capabilities, granted_capabilities, started_at, finished_at
                )
                VALUES (
                    $1, 'single', $2, $3, 'show_notes_v1', $4,
                    '{}'::jsonb, '[]'::jsonb, '[]'::jsonb,
                    CASE WHEN $5::bigint IS NULL THEN NULL ELSE now() - ($5::bigint * interval '1 second') END,
                    CASE WHEN $6::bigint IS NULL THEN NULL ELSE now() - ($6::bigint * interval '1 second') END
                )
                "#,
            )
            .bind(run_id)
            .bind(agent_id)
            .bind(user_id)
            .bind(status)
            .bind(started_offset_s)
            .bind(finished_offset_s)
            .execute(&test_db.app_pool)
            .await?;
        }

        let trigger_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO triggers (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                trigger_type, interval_seconds, input_json, requested_capabilities,
                granted_capabilities, next_fire_at
            )
            VALUES (
                $1, 'single', $2, $3, 'show_notes_v1', 'enabled',
                'interval', 60, '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, now() + interval '60 seconds'
            )
            "#,
        )
        .bind(trigger_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO trigger_events (id, trigger_id, tenant_id, event_id, payload_json, status) VALUES ($1, $2, 'single', 'ops-dead-1', '{}'::jsonb, 'dead_lettered')",
        )
        .bind(Uuid::new_v4())
        .bind(trigger_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/summary?window_secs=3600",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("queued_runs")
                .and_then(Value::as_i64)
                .ok_or("missing queued_runs")?,
            1
        );
        assert_eq!(
            body.get("running_runs")
                .and_then(Value::as_i64)
                .ok_or("missing running_runs")?,
            1
        );
        assert_eq!(
            body.get("succeeded_runs_window")
                .and_then(Value::as_i64)
                .ok_or("missing succeeded_runs_window")?,
            1
        );
        assert_eq!(
            body.get("failed_runs_window")
                .and_then(Value::as_i64)
                .ok_or("missing failed_runs_window")?,
            1
        );
        assert_eq!(
            body.get("dead_letter_trigger_events_window")
                .and_then(Value::as_i64)
                .ok_or("missing dead_letter_trigger_events_window")?,
            1
        );

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/summary?window_secs=3600",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn trusted_proxy_auth_enforces_proxy_token_on_role_scoped_endpoints(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router_with_trusted_proxy_auth(
            test_db.app_pool.clone(),
            true,
            Some("proxy-shared-secret".to_string()),
        );

        let missing_token_req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/summary?window_secs=3600",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let missing_token_resp = app.clone().oneshot(missing_token_req).await?;
        assert_eq!(missing_token_resp.status(), StatusCode::UNAUTHORIZED);
        let missing_token_body = response_json(missing_token_resp).await?;
        assert_eq!(
            missing_token_body
                .get("error")
                .and_then(|value| value.get("code"))
                .and_then(Value::as_str)
                .ok_or("missing error.code")?,
            "UNAUTHORIZED"
        );

        let invalid_token_req = request_with_tenant_and_role_and_proxy_token(
            "GET",
            "/v1/ops/summary?window_secs=3600",
            Some("single"),
            Some("operator"),
            Some("wrong-token"),
            Value::Null,
        )?;
        let invalid_token_resp = app.clone().oneshot(invalid_token_req).await?;
        assert_eq!(invalid_token_resp.status(), StatusCode::UNAUTHORIZED);

        let valid_token_req = request_with_tenant_and_role_and_proxy_token(
            "GET",
            "/v1/ops/summary?window_secs=3600",
            Some("single"),
            Some("operator"),
            Some("proxy-shared-secret"),
            Value::Null,
        )?;
        let valid_token_resp = app.clone().oneshot(valid_token_req).await?;
        assert_eq!(valid_token_resp.status(), StatusCode::OK);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_ops_latency_histogram_returns_bucket_counts_and_enforces_role(
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
            sqlx::query(
                r#"
                INSERT INTO runs (
                    id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                    input_json, requested_capabilities, granted_capabilities, started_at, finished_at
                )
                VALUES (
                    $1, 'single', $2, $3, 'show_notes_v1', 'succeeded',
                    '{}'::jsonb, '[]'::jsonb, '[]'::jsonb,
                    now() - (($4::bigint + 1000) * interval '1 millisecond'),
                    now() - (1000 * interval '1 millisecond')
                )
                "#,
            )
            .bind(run_id)
            .bind(agent_id)
            .bind(user_id)
            .bind(duration_ms)
            .execute(&test_db.app_pool)
            .await?;
        }

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/latency-histogram?window_secs=3600",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        let buckets = body
            .get("buckets")
            .and_then(Value::as_array)
            .ok_or("missing buckets")?;
        assert_eq!(buckets.len(), 6);
        let run_counts = buckets
            .iter()
            .map(|bucket| {
                bucket
                    .get("run_count")
                    .and_then(Value::as_i64)
                    .ok_or("missing bucket run_count")
            })
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(run_counts, vec![1, 1, 1, 1, 1, 1]);

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/latency-histogram?window_secs=3600",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_ops_latency_traces_returns_recent_run_durations_and_enforces_role(
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
        ] {
            sqlx::query(
                r#"
                INSERT INTO runs (
                    id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                    input_json, requested_capabilities, granted_capabilities, started_at, finished_at
                )
                VALUES (
                    $1, 'single', $2, $3, 'show_notes_v1', 'succeeded',
                    '{}'::jsonb, '[]'::jsonb, '[]'::jsonb,
                    now() - (($4::bigint + 1000) * interval '1 millisecond'),
                    now() - (1000 * interval '1 millisecond')
                )
                "#,
            )
            .bind(run_id)
            .bind(agent_id)
            .bind(user_id)
            .bind(duration_ms)
            .execute(&test_db.app_pool)
            .await?;
        }

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/latency-traces?window_secs=3600&limit=3",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        let traces = body
            .get("traces")
            .and_then(Value::as_array)
            .ok_or("missing traces")?;
        assert_eq!(traces.len(), 3);
        for trace in traces {
            assert!(trace.get("duration_ms").and_then(Value::as_i64).is_some());
        }

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/latency-traces?window_secs=3600&limit=3",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_ops_action_latency_returns_action_metrics_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities, started_at, finished_at
            )
            VALUES (
                $1, 'single', $2, $3, 'show_notes_v1', 'running',
                '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, now(), NULL
            )
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let step_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO steps (
                id, run_id, tenant_id, agent_id, user_id, name, status, input_json
            )
            VALUES (
                $1, $2, 'single', $3, $4, 'ops_action_metrics', 'running', '{}'::jsonb
            )
            "#,
        )
        .bind(step_id)
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let action_specs = vec![
            ("message.send", "executed", "executed", 110_i64),
            ("payment.send", "failed", "failed", 900_i64),
            ("payment.send", "denied", "denied", 75_i64),
        ];
        for (action_type, request_status, result_status, duration_ms) in action_specs {
            let action_request_id = Uuid::new_v4();
            let action_result_id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO action_requests (
                    id, step_id, action_type, args_json, justification, status, created_at
                )
                VALUES (
                    $1, $2, $3, '{}'::jsonb, 'integration', $4, now() - interval '5 minutes'
                )
                "#,
            )
            .bind(action_request_id)
            .bind(step_id)
            .bind(action_type)
            .bind(request_status)
            .execute(&test_db.app_pool)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO action_results (
                    id, action_request_id, status, result_json, executed_at
                )
                VALUES (
                    $1, $2, $3, '{}'::jsonb,
                    (now() - interval '5 minutes') + ($4::bigint * interval '1 millisecond')
                )
                "#,
            )
            .bind(action_result_id)
            .bind(action_request_id)
            .bind(result_status)
            .bind(duration_ms)
            .execute(&test_db.app_pool)
            .await?;
        }

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/action-latency?window_secs=3600",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        let actions = body
            .get("actions")
            .and_then(Value::as_array)
            .ok_or("missing actions")?;
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0]
                .get("action_type")
                .and_then(Value::as_str)
                .ok_or("missing action_type")?,
            "payment.send"
        );
        assert_eq!(
            actions[0]
                .get("failed_count")
                .and_then(Value::as_i64)
                .ok_or("missing failed_count")?,
            1
        );
        assert_eq!(
            actions[0]
                .get("denied_count")
                .and_then(Value::as_i64)
                .ok_or("missing denied_count")?,
            1
        );

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/action-latency?window_secs=3600",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_ops_action_latency_traces_returns_recent_actions_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities, started_at, finished_at
            )
            VALUES (
                $1, 'single', $2, $3, 'show_notes_v1', 'running',
                '{}'::jsonb, '[]'::jsonb, '[]'::jsonb, now(), NULL
            )
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let step_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO steps (
                id, run_id, tenant_id, agent_id, user_id, name, status, input_json
            )
            VALUES (
                $1, $2, 'single', $3, $4, 'ops_action_trace', 'running', '{}'::jsonb
            )
            "#,
        )
        .bind(step_id)
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        for (action_type, status, duration_ms, age_seconds) in [
            ("message.send", "executed", 180_i64, 600_i64),
            ("payment.send", "failed", 720_i64, 500_i64),
            ("payment.send", "denied", 95_i64, 400_i64),
        ] {
            let action_request_id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO action_requests (
                    id, step_id, action_type, args_json, justification, status, created_at
                )
                VALUES (
                    $1, $2, $3, '{}'::jsonb, 'integration', $4, now() - ($5::bigint * interval '1 second')
                )
                "#,
            )
            .bind(action_request_id)
            .bind(step_id)
            .bind(action_type)
            .bind(status)
            .bind(age_seconds)
            .execute(&test_db.app_pool)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO action_results (
                    id, action_request_id, status, result_json, executed_at
                )
                VALUES (
                    $1, $2, $3, '{}'::jsonb,
                    (SELECT created_at + ($4::bigint * interval '1 millisecond')
                     FROM action_requests
                     WHERE id = $2)
                )
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(action_request_id)
            .bind(status)
            .bind(duration_ms)
            .execute(&test_db.app_pool)
            .await?;
        }

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/action-latency-traces?window_secs=3600&limit=2&action_type=payment.send",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("action_type")
                .and_then(Value::as_str)
                .ok_or("missing action_type")?,
            "payment.send"
        );
        let traces = body
            .get("traces")
            .and_then(Value::as_array)
            .ok_or("missing traces")?;
        assert_eq!(traces.len(), 2);
        for trace in traces {
            assert_eq!(
                trace
                    .get("action_type")
                    .and_then(Value::as_str)
                    .ok_or("missing trace action_type")?,
                "payment.send"
            );
            assert!(trace.get("duration_ms").and_then(Value::as_i64).is_some());
            assert!(trace
                .get("action_request_id")
                .and_then(Value::as_str)
                .is_some());
        }

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/ops/action-latency-traces?window_secs=3600&limit=2",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_records_create_list_and_purge_flow() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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

        let app = api::app_router(test_db.app_pool.clone());

        let expired_create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "step_id": step_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"expired"},
                "summary_text": "expired",
                "redaction_applied": true,
                "expires_at": (time::OffsetDateTime::now_utc() - time::Duration::hours(1))
            }),
        )?;
        let expired_create_resp = app.clone().oneshot(expired_create_req).await?;
        assert_eq!(expired_create_resp.status(), StatusCode::CREATED);

        let retained_create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "step_id": step_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"retained"},
                "summary_text": "retained",
                "redaction_applied": true,
                "expires_at": (time::OffsetDateTime::now_utc() + time::Duration::hours(1))
            }),
        )?;
        let retained_create_resp = app.clone().oneshot(retained_create_req).await?;
        assert_eq!(retained_create_resp.status(), StatusCode::CREATED);

        let list_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/records?memory_kind=semantic&scope_prefix=memory:project&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let list_resp = app.clone().oneshot(list_req).await?;
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = response_json(list_resp).await?;
        assert_eq!(list_body.as_array().map(Vec::len), Some(1));

        let purge_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records/purge-expired",
            Some("single"),
            Some("owner"),
            json!({
                "as_of": time::OffsetDateTime::now_utc()
            }),
        )?;
        let purge_resp = app.clone().oneshot(purge_req).await?;
        assert_eq!(purge_resp.status(), StatusCode::OK);
        let purge_body = response_json(purge_resp).await?;
        assert_eq!(
            purge_body
                .get("deleted_count")
                .and_then(Value::as_i64)
                .ok_or("missing deleted_count")?,
            1
        );

        let post_purge_list_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/records?memory_kind=semantic&scope_prefix=memory:project&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let post_purge_list_resp = app.clone().oneshot(post_purge_list_req).await?;
        assert_eq!(post_purge_list_resp.status(), StatusCode::OK);
        let post_purge_list_body = response_json(post_purge_list_resp).await?;
        assert_eq!(post_purge_list_body.as_array().map(Vec::len), Some(1));

        let audit_req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/runs/{run_id}/audit?limit=200"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let audit_resp = app.clone().oneshot(audit_req).await?;
        assert_eq!(audit_resp.status(), StatusCode::OK);
        let audit_body = response_json(audit_resp).await?;
        let audit_rows = audit_body.as_array().ok_or("missing audit rows array")?;
        let has_memory_purged = audit_rows.iter().any(|row| {
            row.get("event_type")
                .and_then(Value::as_str)
                .is_some_and(|event_type| event_type == "memory.purged")
        });
        assert!(has_memory_purged);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_records_enforce_role_guardrails() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("viewer"),
            json!({
                "agent_id": agent_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"blocked"}
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);

        let list_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/records?limit=10",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let list_resp = app.clone().oneshot(list_req).await?;
        assert_eq!(list_resp.status(), StatusCode::FORBIDDEN);

        let purge_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records/purge-expired",
            Some("single"),
            Some("operator"),
            json!({}),
        )?;
        let purge_resp = app.clone().oneshot(purge_req).await?;
        assert_eq!(purge_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn handoff_packets_create_and_list_with_filters() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (to_agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let from_agent_a = Uuid::new_v4();
        let from_agent_b = Uuid::new_v4();
        let app = api::app_router(test_db.app_pool.clone());

        let first_create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/handoff-packets",
            Some("single"),
            Some("operator"),
            json!({
                "to_agent_id": to_agent_id,
                "from_agent_id": from_agent_a,
                "title": "handoff-a",
                "payload_json": {"task":"alpha"}
            }),
        )?;
        let first_create_resp = app.clone().oneshot(first_create_req).await?;
        assert_eq!(first_create_resp.status(), StatusCode::CREATED);
        let first_body = response_json(first_create_resp).await?;
        assert_eq!(
            first_body
                .get("to_agent_id")
                .and_then(Value::as_str)
                .ok_or("missing to_agent_id")?,
            to_agent_id.to_string()
        );
        assert_eq!(
            first_body
                .get("from_agent_id")
                .and_then(Value::as_str)
                .ok_or("missing from_agent_id")?,
            from_agent_a.to_string()
        );
        assert_eq!(
            first_body
                .get("payload_json")
                .and_then(|value| value.get("task"))
                .and_then(Value::as_str)
                .ok_or("missing payload_json.task")?,
            "alpha"
        );

        let second_create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/handoff-packets",
            Some("single"),
            Some("operator"),
            json!({
                "to_agent_id": to_agent_id,
                "from_agent_id": from_agent_b,
                "title": "handoff-b",
                "payload_json": {"task":"beta"}
            }),
        )?;
        let second_create_resp = app.clone().oneshot(second_create_req).await?;
        assert_eq!(second_create_resp.status(), StatusCode::CREATED);

        let list_req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/memory/handoff-packets?to_agent_id={to_agent_id}&limit=20"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let list_resp = app.clone().oneshot(list_req).await?;
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = response_json(list_resp).await?;
        let rows = list_body.as_array().ok_or("missing handoff rows")?;
        assert_eq!(rows.len(), 2);

        let filtered_req = request_with_tenant_and_role(
            "GET",
            &format!(
                "/v1/memory/handoff-packets?to_agent_id={to_agent_id}&from_agent_id={from_agent_a}&limit=20"
            ),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let filtered_resp = app.clone().oneshot(filtered_req).await?;
        assert_eq!(filtered_resp.status(), StatusCode::OK);
        let filtered_body = response_json(filtered_resp).await?;
        let filtered_rows = filtered_body
            .as_array()
            .ok_or("missing filtered handoff rows")?;
        assert_eq!(filtered_rows.len(), 1);
        assert_eq!(
            filtered_rows[0]
                .get("from_agent_id")
                .and_then(Value::as_str)
                .ok_or("missing filtered from_agent_id")?,
            from_agent_a.to_string()
        );
        assert_eq!(
            filtered_rows[0]
                .get("title")
                .and_then(Value::as_str)
                .ok_or("missing filtered title")?,
            "handoff-a"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn handoff_packets_enforce_role_and_tenant_guardrails() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (to_agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let from_agent_id = Uuid::new_v4();
        let app = api::app_router(test_db.app_pool.clone());

        let create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/handoff-packets",
            Some("single"),
            Some("operator"),
            json!({
                "to_agent_id": to_agent_id,
                "from_agent_id": from_agent_id,
                "title": "handoff-guardrail",
                "payload_json": {"task":"restricted"}
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);

        let viewer_create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/handoff-packets",
            Some("single"),
            Some("viewer"),
            json!({
                "to_agent_id": to_agent_id,
                "from_agent_id": from_agent_id,
                "title": "viewer-blocked",
                "payload_json": {"task":"blocked"}
            }),
        )?;
        let viewer_create_resp = app.clone().oneshot(viewer_create_req).await?;
        assert_eq!(viewer_create_resp.status(), StatusCode::FORBIDDEN);

        let viewer_list_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/handoff-packets?limit=10",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_list_resp = app.clone().oneshot(viewer_list_req).await?;
        assert_eq!(viewer_list_resp.status(), StatusCode::FORBIDDEN);

        let other_tenant_list_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/handoff-packets?limit=10",
            Some("other"),
            Some("operator"),
            Value::Null,
        )?;
        let other_tenant_list_resp = app.clone().oneshot(other_tenant_list_req).await?;
        assert_eq!(other_tenant_list_resp.status(), StatusCode::OK);
        let other_tenant_body = response_json(other_tenant_list_resp).await?;
        assert_eq!(other_tenant_body.as_array().map(Vec::len), Some(0));

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_records_auto_redact_sensitive_content_before_persist(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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

        let app = api::app_router(test_db.app_pool.clone());
        let create_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "memory_kind": "semantic",
                "scope": "memory:project/secrets",
                "content_json": {
                    "token": "plain-token",
                    "note": "Bearer super-secret-token"
                },
                "summary_text": "keep nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq hidden"
            }),
        )?;
        let create_resp = app.clone().oneshot(create_req).await?;
        assert_eq!(create_resp.status(), StatusCode::CREATED);

        let list_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/records?scope_prefix=memory:project/secrets&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let list_resp = app.clone().oneshot(list_req).await?;
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = response_json(list_resp).await?;
        let rows = list_body.as_array().ok_or("missing rows")?;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(
            row.get("redaction_applied")
                .and_then(Value::as_bool)
                .ok_or("missing redaction_applied")?,
            true
        );
        assert_eq!(
            row.get("content_json")
                .and_then(|value| value.get("token"))
                .and_then(Value::as_str)
                .ok_or("missing content_json.token")?,
            "[REDACTED]"
        );
        assert_eq!(
            row.get("content_json")
                .and_then(|value| value.get("note"))
                .and_then(Value::as_str)
                .ok_or("missing content_json.note")?,
            "Bearer [REDACTED]"
        );
        assert_eq!(
            row.get("summary_text")
                .and_then(Value::as_str)
                .ok_or("missing summary_text")?,
            "keep [REDACTED] hidden"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_retrieve_returns_ranked_items_with_citations() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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

        let app = api::app_router(test_db.app_pool.clone());
        let first_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "step_id": step_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"older"},
                "summary_text": "older"
            }),
        )?;
        let first_resp = app.clone().oneshot(first_req).await?;
        assert_eq!(first_resp.status(), StatusCode::CREATED);
        let first_json = response_json(first_resp).await?;
        let first_id = Uuid::parse_str(
            first_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing first id")?,
        )?;

        let second_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "step_id": step_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"newer"},
                "summary_text": "newer"
            }),
        )?;
        let second_resp = app.clone().oneshot(second_req).await?;
        assert_eq!(second_resp.status(), StatusCode::CREATED);
        let second_json = response_json(second_resp).await?;
        let second_id = Uuid::parse_str(
            second_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing second id")?,
        )?;

        sqlx::query(
            "UPDATE memory_records SET created_at = now() - interval '2 minutes' WHERE id = $1",
        )
        .bind(first_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "UPDATE memory_records SET created_at = now() - interval '1 minute' WHERE id = $1",
        )
        .bind(second_id)
        .execute(&test_db.app_pool)
        .await?;

        let retrieve_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/retrieve?memory_kind=semantic&scope_prefix=memory:project&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let retrieve_resp = app.clone().oneshot(retrieve_req).await?;
        assert_eq!(retrieve_resp.status(), StatusCode::OK);
        let body = response_json(retrieve_resp).await?;
        assert_eq!(
            body.get("retrieved_count")
                .and_then(Value::as_i64)
                .ok_or("missing retrieved_count")?,
            2
        );
        let items = body
            .get("items")
            .and_then(Value::as_array)
            .ok_or("missing items")?;
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0]
                .get("rank")
                .and_then(Value::as_i64)
                .ok_or("missing first rank")?,
            1
        );
        assert_eq!(
            Uuid::parse_str(
                items[0]
                    .get("citation")
                    .and_then(|value| value.get("memory_id"))
                    .and_then(Value::as_str)
                    .ok_or("missing citation memory_id")?,
            )?,
            second_id
        );
        assert_eq!(
            items[0]
                .get("content_json")
                .and_then(|value| value.get("note"))
                .and_then(Value::as_str)
                .ok_or("missing content note")?,
            "newer"
        );
        assert!(
            items[0]
                .get("score")
                .and_then(Value::as_f64)
                .ok_or("missing first score")?
                >= items[1]
                    .get("score")
                    .and_then(Value::as_f64)
                    .ok_or("missing second score")?
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_retrieve_supports_query_score_and_source_filters(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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

        let app = api::app_router(test_db.app_pool.clone());
        let generic_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "step_id": step_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"generic context"},
                "source": "api"
            }),
        )?;
        let generic_resp = app.clone().oneshot(generic_req).await?;
        assert_eq!(generic_resp.status(), StatusCode::CREATED);

        let partial_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "step_id": step_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"alpha-only"},
                "summary_text": "alpha signal",
                "source": "ingest.partial"
            }),
        )?;
        let partial_resp = app.clone().oneshot(partial_req).await?;
        assert_eq!(partial_resp.status(), StatusCode::CREATED);
        let partial_json = response_json(partial_resp).await?;
        let partial_id = Uuid::parse_str(
            partial_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing partial id")?,
        )?;

        let full_req = request_with_tenant_and_role(
            "POST",
            "/v1/memory/records",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "run_id": run_id,
                "step_id": step_id,
                "memory_kind": "semantic",
                "scope": "memory:project/roadmap",
                "content_json": {"note":"contains alpha budget decision"},
                "summary_text": "alpha budget plan",
                "source": "ingest.full"
            }),
        )?;
        let full_resp = app.clone().oneshot(full_req).await?;
        assert_eq!(full_resp.status(), StatusCode::CREATED);
        let full_json = response_json(full_resp).await?;
        let full_id = Uuid::parse_str(
            full_json
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing full id")?,
        )?;

        sqlx::query(
            "UPDATE memory_records SET created_at = now() - interval '2 minutes' WHERE id = $1",
        )
        .bind(full_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "UPDATE memory_records SET created_at = now() - interval '1 minute' WHERE id = $1",
        )
        .bind(partial_id)
        .execute(&test_db.app_pool)
        .await?;

        let retrieve_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/retrieve?memory_kind=semantic&scope_prefix=memory:project&query_text=alpha%20budget&source_prefix=ingest.&require_summary=true&min_score=1.0&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let retrieve_resp = app.clone().oneshot(retrieve_req).await?;
        assert_eq!(retrieve_resp.status(), StatusCode::OK);
        let body = response_json(retrieve_resp).await?;
        assert_eq!(
            body.get("query_text")
                .and_then(Value::as_str)
                .ok_or("missing query_text")?,
            "alpha budget"
        );
        assert_eq!(
            body.get("source_prefix")
                .and_then(Value::as_str)
                .ok_or("missing source_prefix")?,
            "ingest."
        );
        assert_eq!(
            body.get("require_summary")
                .and_then(Value::as_bool)
                .ok_or("missing require_summary")?,
            true
        );
        assert!(
            (body
                .get("min_score")
                .and_then(Value::as_f64)
                .ok_or("missing min_score")?
                - 1.0)
                .abs()
                < f64::EPSILON
        );
        assert_eq!(
            body.get("retrieved_count")
                .and_then(Value::as_i64)
                .ok_or("missing retrieved_count")?,
            1
        );
        let items = body
            .get("items")
            .and_then(Value::as_array)
            .ok_or("missing items")?;
        assert_eq!(items.len(), 1);
        let first_id = Uuid::parse_str(
            items[0]
                .get("citation")
                .and_then(|value| value.get("memory_id"))
                .and_then(Value::as_str)
                .ok_or("missing citation memory_id")?,
        )?;
        assert_eq!(first_id, full_id);
        assert!(
            items[0]
                .get("score")
                .and_then(Value::as_f64)
                .ok_or("missing score")?
                >= 1.0
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_retrieve_rejects_invalid_min_score() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/retrieve?scope_prefix=memory:project&min_score=2.5",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_retrieve_enforces_scope_role_and_tenant_isolation(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        agent_core::create_memory_record(
            &test_db.app_pool,
            &agent_core::NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "single".to_string(),
                agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "semantic".to_string(),
                scope: "memory:project/roadmap".to_string(),
                content_json: json!({"note":"single"}),
                summary_text: None,
                source: "api".to_string(),
                redaction_applied: false,
                expires_at: None,
            },
        )
        .await?;

        let other_agent_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'other', 'other-agent', 'active')",
        )
        .bind(other_agent_id)
        .execute(&test_db.app_pool)
        .await?;
        agent_core::create_memory_record(
            &test_db.app_pool,
            &agent_core::NewMemoryRecord {
                id: Uuid::new_v4(),
                tenant_id: "other".to_string(),
                agent_id: other_agent_id,
                run_id: None,
                step_id: None,
                memory_kind: "semantic".to_string(),
                scope: "memory:project/private".to_string(),
                content_json: json!({"note":"other"}),
                summary_text: None,
                source: "api".to_string(),
                redaction_applied: false,
                expires_at: None,
            },
        )
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let retrieve_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/retrieve?scope_prefix=memory:project&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let retrieve_resp = app.clone().oneshot(retrieve_req).await?;
        assert_eq!(retrieve_resp.status(), StatusCode::OK);
        let retrieve_body = response_json(retrieve_resp).await?;
        assert_eq!(
            retrieve_body
                .get("retrieved_count")
                .and_then(Value::as_i64)
                .ok_or("missing retrieved_count")?,
            1
        );

        let bad_scope_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/retrieve?scope_prefix=podcasts/&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let bad_scope_resp = app.clone().oneshot(bad_scope_req).await?;
        assert_eq!(bad_scope_resp.status(), StatusCode::BAD_REQUEST);

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/retrieve?scope_prefix=memory:project&limit=10",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn memory_compaction_stats_returns_counts_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, _user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO memory_records (
                id, tenant_id, agent_id, memory_kind, scope, content_json, source, redaction_applied
            )
            VALUES
                ($1, 'single', $2, 'session', 'memory:session/a', '{}'::jsonb, 'worker', false),
                ($3, 'single', $2, 'session', 'memory:session/a', '{}'::jsonb, 'worker', false)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(agent_id)
        .bind(Uuid::new_v4())
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO memory_compactions (
                id, tenant_id, agent_id, memory_kind, scope, source_count, source_entry_ids, summary_json
            )
            VALUES ($1, 'single', $2, 'session', 'memory:session/a', 2, '[]'::jsonb, '{}'::jsonb)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let stats_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/compactions/stats?window_secs=3600",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let stats_resp = app.clone().oneshot(stats_req).await?;
        assert_eq!(stats_resp.status(), StatusCode::OK);
        let stats_body = response_json(stats_resp).await?;
        assert_eq!(
            stats_body
                .get("compacted_groups_window")
                .and_then(Value::as_i64)
                .ok_or("missing compacted_groups_window")?,
            1
        );
        assert_eq!(
            stats_body
                .get("pending_uncompacted_records")
                .and_then(Value::as_i64)
                .ok_or("missing pending_uncompacted_records")?,
            2
        );

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/memory/compactions/stats?window_secs=3600",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_payments_returns_tenant_scoped_ledger_with_latest_result(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        let action_request_id = Uuid::new_v4();
        let payment_request_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'payments_v1', 'succeeded', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO steps (id, run_id, tenant_id, agent_id, user_id, name, status, input_json) VALUES ($1, $2, 'single', $3, $4, 'payment', 'succeeded', '{}'::jsonb)",
        )
        .bind(step_id)
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO action_requests (id, step_id, action_type, args_json, status) VALUES ($1, $2, 'payment.send', '{}'::jsonb, 'executed')",
        )
        .bind(action_request_id)
        .bind(step_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO payment_requests (
                id, action_request_id, run_id, tenant_id, agent_id, provider, operation,
                destination, idempotency_key, amount_msat, request_json, status
            )
            VALUES ($1, $2, $3, 'single', $4, 'nwc', 'pay_invoice', 'nwc:wallet-main', 'pay-ledger-001', 2100, '{"operation":"pay_invoice"}'::jsonb, 'executed')
            "#,
        )
        .bind(payment_request_id)
        .bind(action_request_id)
        .bind(run_id)
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO payment_results (id, payment_request_id, status, error_json, created_at) VALUES ($1, $2, 'failed', '{\"code\":\"TEMP\"}'::jsonb, now() - interval '5 minutes')",
        )
        .bind(Uuid::new_v4())
        .bind(payment_request_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO payment_results (id, payment_request_id, status, result_json, created_at) VALUES ($1, $2, 'executed', '{\"settlement_status\":\"settled\",\"payment_preimage\":\"abc\"}'::jsonb, now() - interval '1 minutes')",
        )
        .bind(Uuid::new_v4())
        .bind(payment_request_id)
        .execute(&test_db.app_pool)
        .await?;

        let other_agent_id = Uuid::new_v4();
        let other_user_id = Uuid::new_v4();
        let other_run_id = Uuid::new_v4();
        let other_step_id = Uuid::new_v4();
        let other_action_request_id = Uuid::new_v4();
        let other_payment_request_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'other', 'secureagnt_api_test_other', 'active')",
        )
        .bind(other_agent_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO users (id, tenant_id, external_subject, display_name, status) VALUES ($1, 'other', 'api:test:other', 'Other User', 'active')",
        )
        .bind(other_user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO runs (id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, input_json, requested_capabilities, granted_capabilities) VALUES ($1, 'other', $2, $3, 'payments_v1', 'succeeded', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)",
        )
        .bind(other_run_id)
        .bind(other_agent_id)
        .bind(other_user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO steps (id, run_id, tenant_id, agent_id, user_id, name, status, input_json) VALUES ($1, $2, 'other', $3, $4, 'payment', 'succeeded', '{}'::jsonb)",
        )
        .bind(other_step_id)
        .bind(other_run_id)
        .bind(other_agent_id)
        .bind(other_user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO action_requests (id, step_id, action_type, args_json, status) VALUES ($1, $2, 'payment.send', '{}'::jsonb, 'executed')",
        )
        .bind(other_action_request_id)
        .bind(other_step_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO payment_requests (id, action_request_id, run_id, tenant_id, agent_id, provider, operation, destination, idempotency_key, amount_msat, request_json, status) VALUES ($1, $2, $3, 'other', $4, 'nwc', 'pay_invoice', 'nwc:wallet-other', 'pay-ledger-001', 1000, '{}'::jsonb, 'executed')",
        )
        .bind(other_payment_request_id)
        .bind(other_action_request_id)
        .bind(other_run_id)
        .bind(other_agent_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/payments?idempotency_key=pay-ledger-001&limit=10",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        let rows = body.as_array().ok_or("ledger response must be an array")?;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .get("tenant_id")
                .and_then(Value::as_str)
                .ok_or("missing tenant id")?,
            "single"
        );
        assert_eq!(
            rows[0]
                .get("latest_result_status")
                .and_then(Value::as_str)
                .ok_or("missing latest_result_status")?,
            "executed"
        );
        assert_eq!(
            rows[0]
                .get("settlement_status")
                .and_then(Value::as_str)
                .ok_or("missing settlement_status")?,
            "settled"
        );
        assert_eq!(
            rows[0]
                .get("settlement_rail")
                .and_then(Value::as_str)
                .ok_or("missing settlement_rail")?,
            "nwc"
        );
        assert_eq!(
            rows[0]
                .get("normalized_outcome")
                .and_then(Value::as_str)
                .ok_or("missing normalized_outcome")?,
            "executed"
        );
        assert_eq!(rows[0].get("normalized_error_code"), Some(&Value::Null));
        assert_eq!(rows[0].get("normalized_error_class"), Some(&Value::Null));

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_payments_rejects_viewer_role() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router(test_db.app_pool.clone());
        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/payments?limit=10",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_payment_summary_returns_counts_and_spend() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO runs (id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, input_json, requested_capabilities, granted_capabilities) VALUES ($1, 'single', $2, $3, 'payments_v1', 'succeeded', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)",
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO steps (id, run_id, tenant_id, agent_id, user_id, name, status, input_json) VALUES ($1, $2, 'single', $3, $4, 'payment', 'succeeded', '{}'::jsonb)",
        )
        .bind(step_id)
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let action_executed = Uuid::new_v4();
        let action_failed = Uuid::new_v4();
        let action_duplicate = Uuid::new_v4();
        let action_requested = Uuid::new_v4();
        for action_id in [
            action_executed,
            action_failed,
            action_duplicate,
            action_requested,
        ] {
            sqlx::query(
                "INSERT INTO action_requests (id, step_id, action_type, args_json, status) VALUES ($1, $2, 'payment.send', '{}'::jsonb, 'requested')",
            )
            .bind(action_id)
            .bind(step_id)
            .execute(&test_db.app_pool)
            .await?;
        }

        sqlx::query(
            "INSERT INTO payment_requests (id, action_request_id, run_id, tenant_id, agent_id, provider, operation, destination, idempotency_key, amount_msat, request_json, status) VALUES ($1, $2, $3, 'single', $4, 'nwc', 'pay_invoice', 'nwc:wallet-main', 'sum-001', 2500, '{}'::jsonb, 'executed')",
        )
        .bind(Uuid::new_v4())
        .bind(action_executed)
        .bind(run_id)
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO payment_requests (id, action_request_id, run_id, tenant_id, agent_id, provider, operation, destination, idempotency_key, amount_msat, request_json, status) VALUES ($1, $2, $3, 'single', $4, 'nwc', 'pay_invoice', 'nwc:wallet-main', 'sum-002', 1000, '{}'::jsonb, 'failed')",
        )
        .bind(Uuid::new_v4())
        .bind(action_failed)
        .bind(run_id)
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO payment_requests (id, action_request_id, run_id, tenant_id, agent_id, provider, operation, destination, idempotency_key, amount_msat, request_json, status) VALUES ($1, $2, $3, 'single', $4, 'nwc', 'make_invoice', 'nwc:wallet-main', 'sum-003', 1200, '{}'::jsonb, 'duplicate')",
        )
        .bind(Uuid::new_v4())
        .bind(action_duplicate)
        .bind(run_id)
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO payment_requests (id, action_request_id, run_id, tenant_id, agent_id, provider, operation, destination, idempotency_key, amount_msat, request_json, status) VALUES ($1, $2, $3, 'single', $4, 'nwc', 'get_balance', 'nwc:wallet-main', 'sum-004', null, '{}'::jsonb, 'requested')",
        )
        .bind(Uuid::new_v4())
        .bind(action_requested)
        .bind(run_id)
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/payments/summary?window_secs=3600",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;

        assert_eq!(
            body.get("total_requests")
                .and_then(Value::as_i64)
                .ok_or("missing total_requests")?,
            4
        );
        assert_eq!(
            body.get("requested_count")
                .and_then(Value::as_i64)
                .ok_or("missing requested_count")?,
            1
        );
        assert_eq!(
            body.get("executed_count")
                .and_then(Value::as_i64)
                .ok_or("missing executed_count")?,
            1
        );
        assert_eq!(
            body.get("failed_count")
                .and_then(Value::as_i64)
                .ok_or("missing failed_count")?,
            1
        );
        assert_eq!(
            body.get("duplicate_count")
                .and_then(Value::as_i64)
                .ok_or("missing duplicate_count")?,
            1
        );
        assert_eq!(
            body.get("executed_spend_msat")
                .and_then(Value::as_i64)
                .ok_or("missing executed_spend_msat")?,
            2500
        );

        let op_req = request_with_tenant_and_role(
            "GET",
            "/v1/payments/summary?operation=pay_invoice",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let op_resp = app.clone().oneshot(op_req).await?;
        assert_eq!(op_resp.status(), StatusCode::OK);
        let op_body = response_json(op_resp).await?;
        assert_eq!(
            op_body
                .get("total_requests")
                .and_then(Value::as_i64)
                .ok_or("missing filtered total_requests")?,
            2
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_payment_summary_rejects_invalid_operation() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/payments/summary?operation=unknown",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_returns_high_risk_events() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        let action_request_id = Uuid::new_v4();
        let payment_request_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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

        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "request_id": "req-123",
                    "session_id": "sess-abc",
                    "action_request_id": action_request_id,
                    "payment_request_id": payment_request_id
                }),
            },
        )
        .await?;
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance?run_id={run_id}&limit=10"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        let rows = body.as_array().ok_or("compliance response must be array")?;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .get("event_type")
                .and_then(Value::as_str)
                .ok_or("missing event_type")?,
            "action.executed"
        );
        assert!(rows[0].get("source_audit_event_id").is_some());
        assert_eq!(
            rows[0]
                .get("tamper_chain_seq")
                .and_then(Value::as_i64)
                .ok_or("missing tamper_chain_seq")?,
            1
        );
        assert_eq!(
            rows[0].get("tamper_prev_hash").and_then(Value::as_str),
            None
        );
        assert_eq!(
            rows[0]
                .get("tamper_hash")
                .and_then(Value::as_str)
                .ok_or("missing tamper_hash")?
                .len(),
            32
        );
        assert_eq!(
            rows[0].get("request_id").and_then(Value::as_str),
            Some("req-123")
        );
        assert_eq!(
            rows[0].get("session_id").and_then(Value::as_str),
            Some("sess-abc")
        );
        assert_eq!(
            Uuid::parse_str(
                rows[0]
                    .get("action_request_id")
                    .and_then(Value::as_str)
                    .ok_or("missing action_request_id")?,
            )?,
            action_request_id
        );
        assert_eq!(
            Uuid::parse_str(
                rows[0]
                    .get("payment_request_id")
                    .and_then(Value::as_str)
                    .ok_or("missing payment_request_id")?,
            )?,
            payment_request_id
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_export_returns_ndjson() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-main"
                }),
            },
        )
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance/export?run_id={run_id}&limit=10"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert_eq!(content_type, "application/x-ndjson");

        let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
        let body = String::from_utf8(bytes.to_vec())?;
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 1);
        let row: Value = serde_json::from_str(lines[0])?;
        assert_eq!(
            row.get("event_type")
                .and_then(Value::as_str)
                .ok_or("missing event_type")?,
            "action.executed"
        );
        assert_eq!(
            row.get("tamper_chain_seq")
                .and_then(Value::as_i64)
                .ok_or("missing tamper_chain_seq")?,
            1
        );
        assert!(row.get("tamper_hash").and_then(Value::as_str).is_some());

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_siem_export_supports_adapter_formats(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-main"
                }),
            },
        )
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let splunk_req = request_with_tenant_and_role(
            "GET",
            &format!(
                "/v1/audit/compliance/siem/export?run_id={run_id}&limit=10&adapter=splunk_hec"
            ),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let splunk_resp = app.clone().oneshot(splunk_req).await?;
        assert_eq!(splunk_resp.status(), StatusCode::OK);
        let splunk_bytes = to_bytes(splunk_resp.into_body(), usize::MAX).await?;
        let splunk_body = String::from_utf8(splunk_bytes.to_vec())?;
        let splunk_lines: Vec<&str> = splunk_body.lines().collect();
        assert_eq!(splunk_lines.len(), 1);
        let splunk_row: Value = serde_json::from_str(splunk_lines[0])?;
        assert_eq!(
            splunk_row
                .get("sourcetype")
                .and_then(Value::as_str)
                .ok_or("missing splunk sourcetype")?,
            "secureagnt:compliance"
        );
        assert_eq!(
            splunk_row
                .get("event")
                .and_then(|event| event.get("event_type"))
                .and_then(Value::as_str)
                .ok_or("missing splunk event type")?,
            "action.executed"
        );

        let elastic_req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance/siem/export?run_id={run_id}&limit=10&adapter=elastic_bulk&elastic_index=tenant-audit"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let elastic_resp = app.clone().oneshot(elastic_req).await?;
        assert_eq!(elastic_resp.status(), StatusCode::OK);
        let elastic_bytes = to_bytes(elastic_resp.into_body(), usize::MAX).await?;
        let elastic_body = String::from_utf8(elastic_bytes.to_vec())?;
        let elastic_lines: Vec<&str> = elastic_body.lines().collect();
        assert_eq!(elastic_lines.len(), 2);
        let action_row: Value = serde_json::from_str(elastic_lines[0])?;
        let doc_row: Value = serde_json::from_str(elastic_lines[1])?;
        assert_eq!(
            action_row
                .get("index")
                .and_then(|value| value.get("_index"))
                .and_then(Value::as_str)
                .ok_or("missing elastic index")?,
            "tenant-audit"
        );
        assert_eq!(
            doc_row
                .get("event_type")
                .and_then(Value::as_str)
                .ok_or("missing elastic event type")?,
            "action.executed"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn post_compliance_audit_siem_delivery_queues_outbox_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-main"
                }),
            },
        )
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let post_req = request_with_tenant_and_role(
            "POST",
            "/v1/audit/compliance/siem/deliveries",
            Some("single"),
            Some("operator"),
            json!({
                "run_id": run_id,
                "adapter": "splunk_hec",
                "delivery_target": "mock://success",
                "max_attempts": 4
            }),
        )?;
        let post_resp = app.clone().oneshot(post_req).await?;
        assert_eq!(post_resp.status(), StatusCode::ACCEPTED);
        let post_body = response_json(post_resp).await?;
        assert_eq!(
            post_body
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing status")?,
            "pending"
        );
        let outbox_id = Uuid::parse_str(
            post_body
                .get("id")
                .and_then(Value::as_str)
                .ok_or("missing outbox id")?,
        )?;

        let row = sqlx::query(
            "SELECT adapter, delivery_target, status, max_attempts, payload_ndjson FROM compliance_siem_delivery_outbox WHERE id = $1",
        )
        .bind(outbox_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        let adapter: String = row.get("adapter");
        let delivery_target: String = row.get("delivery_target");
        let status: String = row.get("status");
        let max_attempts: i32 = row.get("max_attempts");
        let payload_ndjson: String = row.get("payload_ndjson");
        assert_eq!(adapter, "splunk_hec");
        assert_eq!(delivery_target, "mock://success");
        assert_eq!(status, "pending");
        assert_eq!(max_attempts, 4);
        assert!(!payload_ndjson.trim().is_empty());

        let viewer_req = request_with_tenant_and_role(
            "POST",
            "/v1/audit/compliance/siem/deliveries",
            Some("single"),
            Some("viewer"),
            json!({
                "run_id": run_id,
                "delivery_target": "mock://success"
            }),
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        let invalid_req = request_with_tenant_and_role(
            "POST",
            "/v1/audit/compliance/siem/deliveries",
            Some("single"),
            Some("operator"),
            json!({
                "run_id": run_id,
                "delivery_target": "   "
            }),
        )?;
        let invalid_resp = app.clone().oneshot(invalid_req).await?;
        assert_eq!(invalid_resp.status(), StatusCode::BAD_REQUEST);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_siem_deliveries_is_tenant_scoped_and_role_guarded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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

        let other_agent_id = Uuid::new_v4();
        let other_user_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agents (id, tenant_id, name, status) VALUES ($1, 'other', 'other-agent', 'active')",
        )
        .bind(other_agent_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO users (id, tenant_id, external_subject, display_name, status) VALUES ($1, 'other', 'other-user', 'Other User', 'active')",
        )
        .bind(other_user_id)
        .execute(&test_db.app_pool)
        .await?;
        let other_run_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO runs (id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status, input_json, requested_capabilities, granted_capabilities) VALUES ($1, 'other', $2, $3, 'payments_v1', 'running', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)",
        )
        .bind(other_run_id)
        .bind(other_agent_id)
        .bind(other_user_id)
        .execute(&test_db.app_pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO compliance_siem_delivery_outbox (
                id, tenant_id, run_id, adapter, delivery_target, payload_ndjson, status
            )
            VALUES
                ($1, 'single', $2, 'secureagnt_ndjson', 'mock://success', '{"event":"a"}\n', 'pending'),
                ($3, 'other', $4, 'secureagnt_ndjson', 'mock://success', '{"event":"b"}\n', 'failed')
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(Uuid::new_v4())
        .bind(other_run_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let list_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries?limit=20",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let list_resp = app.clone().oneshot(list_req).await?;
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = response_json(list_resp).await?;
        let rows = list_body.as_array().ok_or("missing rows")?;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0]
                .get("tenant_id")
                .and_then(Value::as_str)
                .ok_or("missing tenant_id")?,
            "single"
        );

        let status_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries?status=pending",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let status_resp = app.clone().oneshot(status_req).await?;
        assert_eq!(status_resp.status(), StatusCode::OK);

        let bad_status_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries?status=unknown",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let bad_status_resp = app.clone().oneshot(bad_status_req).await?;
        assert_eq!(bad_status_resp.status(), StatusCode::BAD_REQUEST);

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries?limit=20",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_siem_deliveries_summary_returns_counts_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let run_id = Uuid::new_v4();
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'payments_v1', 'running', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        let app = api::app_router(test_db.app_pool.clone());

        for (status, target) in [
            ("pending", "mock://pending"),
            ("processing", "mock://processing"),
            ("delivered", "mock://delivered"),
            ("dead_lettered", "mock://dead-lettered"),
        ] {
            let record_id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO compliance_siem_delivery_outbox (
                    id, tenant_id, run_id, adapter, delivery_target, content_type, payload_ndjson,
                    status, attempts, max_attempts, next_attempt_at
                )
                VALUES (
                    $1, 'single', $2, 'secureagnt_ndjson', $3, 'application/x-ndjson', '{"event":"x"}',
                    $4, 0, 3, now()
                )
                "#,
            )
            .bind(record_id)
            .bind(run_id)
            .bind(target)
            .bind(status)
            .execute(&test_db.app_pool)
            .await?;
        }

        let req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance/siem/deliveries/summary?run_id={run_id}"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("pending_count")
                .and_then(Value::as_i64)
                .ok_or("missing pending_count")?,
            1
        );
        assert_eq!(
            body.get("processing_count")
                .and_then(Value::as_i64)
                .ok_or("missing processing_count")?,
            1
        );
        assert_eq!(
            body.get("delivered_count")
                .and_then(Value::as_i64)
                .ok_or("missing delivered_count")?,
            1
        );
        assert_eq!(
            body.get("dead_lettered_count")
                .and_then(Value::as_i64)
                .ok_or("missing dead_lettered_count")?,
            1
        );

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries/summary",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        let other_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries/summary",
            Some("other"),
            Some("operator"),
            Value::Null,
        )?;
        let other_resp = app.clone().oneshot(other_req).await?;
        assert_eq!(other_resp.status(), StatusCode::OK);
        let other_body = response_json(other_resp).await?;
        assert_eq!(
            other_body
                .get("pending_count")
                .and_then(Value::as_i64)
                .ok_or("missing other pending_count")?,
            0
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_siem_deliveries_slo_returns_rates_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let run_id = Uuid::new_v4();
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'payments_v1', 'running', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;
        let app = api::app_router(test_db.app_pool.clone());

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
                    $1, 'single', $2, 'secureagnt_ndjson', $3, 'application/x-ndjson', '{"event":"x"}',
                    $4, 0, 3, now(), now() - interval '5 seconds', now()
                )
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(run_id)
            .bind(target)
            .bind(status)
            .execute(&test_db.app_pool)
            .await?;
        }

        let req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance/siem/deliveries/slo?run_id={run_id}&window_secs=3600"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("total_count")
                .and_then(Value::as_i64)
                .ok_or("missing total_count")?,
            4
        );
        assert_eq!(
            body.get("delivery_success_rate_pct")
                .and_then(Value::as_f64)
                .ok_or("missing delivery_success_rate_pct")?,
            25.0
        );
        assert_eq!(
            body.get("hard_failure_rate_pct")
                .and_then(Value::as_f64)
                .ok_or("missing hard_failure_rate_pct")?,
            25.0
        );
        assert_eq!(
            body.get("dead_letter_rate_pct")
                .and_then(Value::as_f64)
                .ok_or("missing dead_letter_rate_pct")?,
            25.0
        );

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries/slo",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_siem_delivery_targets_returns_grouped_counters(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let run_id = Uuid::new_v4();
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'payments_v1', 'running', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO compliance_siem_delivery_outbox (
                id, tenant_id, run_id, adapter, delivery_target, content_type, payload_ndjson,
                status, attempts, max_attempts, next_attempt_at, last_error, last_http_status
            )
            VALUES
                ($1, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"a"}', 'failed', 1, 3, now(), 'auth denied', 401),
                ($3, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"b"}', 'dead_lettered', 3, 3, now(), 'dead letter', 500),
                ($4, 'single', $2, 'splunk_hec', 'https://siem-b.example/hec', 'application/x-ndjson', '{"event":"c"}', 'delivered', 1, 3, now(), null, 200)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance/siem/deliveries/targets?run_id={run_id}&limit=10"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        let rows = body.as_array().ok_or("missing targets rows")?;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0]
                .get("delivery_target")
                .and_then(Value::as_str)
                .ok_or("missing target")?,
            "https://siem-a.example/hec"
        );
        assert_eq!(
            rows[0]
                .get("failed_count")
                .and_then(Value::as_i64)
                .ok_or("missing failed_count")?,
            1
        );
        assert_eq!(
            rows[0]
                .get("dead_lettered_count")
                .and_then(Value::as_i64)
                .ok_or("missing dead_lettered_count")?,
            1
        );

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries/targets",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_siem_delivery_alerts_returns_breaches_and_enforces_role(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let run_id = Uuid::new_v4();
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'payments_v1', 'running', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO compliance_siem_delivery_outbox (
                id, tenant_id, run_id, adapter, delivery_target, content_type, payload_ndjson,
                status, attempts, max_attempts, next_attempt_at, last_error, last_http_status, created_at
            )
            VALUES
                ($1, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"a"}', 'failed', 1, 3, now(), 'auth denied', 401, now() - interval '5 minutes'),
                ($3, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"b"}', 'dead_lettered', 3, 3, now(), 'dead letter', 500, now() - interval '4 minutes'),
                ($4, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"c"}', 'pending', 0, 3, now(), null, null, now() - interval '3 minutes'),
                ($5, 'single', $2, 'splunk_hec', 'https://siem-b.example/hec', 'application/x-ndjson', '{"event":"d"}', 'delivered', 1, 3, now(), null, 200, now() - interval '2 minutes')
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            &format!(
                "/v1/audit/compliance/siem/deliveries/alerts?run_id={run_id}&window_secs=3600&limit=10&max_hard_failure_rate_pct=10&max_dead_letter_rate_pct=5&max_pending_count=0"
            ),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        let alerts = body
            .get("alerts")
            .and_then(Value::as_array)
            .ok_or("missing alerts")?;
        assert_eq!(alerts.len(), 1);
        assert_eq!(
            alerts[0]
                .get("delivery_target")
                .and_then(Value::as_str)
                .ok_or("missing delivery_target")?,
            "https://siem-a.example/hec"
        );
        assert_eq!(
            alerts[0]
                .get("severity")
                .and_then(Value::as_str)
                .ok_or("missing severity")?,
            "critical"
        );
        let triggered = alerts[0]
            .get("triggered_rules")
            .and_then(Value::as_array)
            .ok_or("missing triggered_rules")?;
        assert!(!triggered.is_empty());

        let viewer_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/deliveries/alerts",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let viewer_resp = app.clone().oneshot(viewer_req).await?;
        assert_eq!(viewer_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_alert_acknowledge_marks_alert_and_enforces_user_header(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let run_id = Uuid::new_v4();
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'payments_v1', 'running', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO compliance_siem_delivery_outbox (
                id, tenant_id, run_id, adapter, delivery_target, content_type, payload_ndjson,
                status, attempts, max_attempts, next_attempt_at, last_error, last_http_status, created_at
            )
            VALUES
                ($1, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"a"}', 'failed', 1, 3, now(), 'auth denied', 401, now() - interval '5 minutes'),
                ($3, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"b"}', 'dead_lettered', 3, 3, now(), 'dead letter', 500, now() - interval '4 minutes'),
                ($4, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"c"}', 'pending', 0, 3, now(), null, null, now() - interval '3 minutes')
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(Uuid::new_v4())
        .bind(Uuid::new_v4())
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let list_req = request_with_tenant_and_role(
            "GET",
            &format!(
                "/v1/audit/compliance/siem/deliveries/alerts?run_id={run_id}&window_secs=3600&limit=10&max_hard_failure_rate_pct=10&max_dead_letter_rate_pct=5&max_pending_count=0"
            ),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let list_resp = app.clone().oneshot(list_req).await?;
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = response_json(list_resp).await?;
        let alerts = list_body
            .get("alerts")
            .and_then(Value::as_array)
            .ok_or("missing alerts")?;
        assert_eq!(alerts.len(), 1);
        assert_eq!(
            alerts[0]
                .get("acknowledged")
                .and_then(Value::as_bool)
                .ok_or("missing acknowledged")?,
            false
        );

        let missing_user_req = request_with_tenant_and_role(
            "POST",
            "/v1/audit/compliance/siem/deliveries/alerts/ack",
            Some("single"),
            Some("operator"),
            json!({
                "run_id": run_id,
                "delivery_target": "https://siem-a.example/hec",
                "note": "mitigated"
            }),
        )?;
        let missing_user_resp = app.clone().oneshot(missing_user_req).await?;
        assert_eq!(missing_user_resp.status(), StatusCode::FORBIDDEN);

        let ack_req = request_with_tenant_and_role_and_user_and_secret(
            "POST",
            "/v1/audit/compliance/siem/deliveries/alerts/ack",
            Some("single"),
            Some("operator"),
            Some(user_id),
            None,
            json!({
                "run_id": run_id,
                "delivery_target": "https://siem-a.example/hec",
                "note": "mitigation applied"
            }),
        )?;
        let ack_resp = app.clone().oneshot(ack_req).await?;
        assert_eq!(ack_resp.status(), StatusCode::OK);
        let ack_body = response_json(ack_resp).await?;
        assert_eq!(
            ack_body
                .get("delivery_target")
                .and_then(Value::as_str)
                .ok_or("missing delivery_target")?,
            "https://siem-a.example/hec"
        );
        assert_eq!(
            ack_body
                .get("acknowledged_by_user_id")
                .and_then(Value::as_str)
                .ok_or("missing acknowledged_by_user_id")?,
            user_id.to_string()
        );
        assert_eq!(
            ack_body
                .get("acknowledged_by_role")
                .and_then(Value::as_str)
                .ok_or("missing acknowledged_by_role")?,
            "operator"
        );

        let list_after_req = request_with_tenant_and_role(
            "GET",
            &format!(
                "/v1/audit/compliance/siem/deliveries/alerts?run_id={run_id}&window_secs=3600&limit=10&max_hard_failure_rate_pct=10&max_dead_letter_rate_pct=5&max_pending_count=0"
            ),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let list_after_resp = app.clone().oneshot(list_after_req).await?;
        assert_eq!(list_after_resp.status(), StatusCode::OK);
        let list_after_body = response_json(list_after_resp).await?;
        let after_alerts = list_after_body
            .get("alerts")
            .and_then(Value::as_array)
            .ok_or("missing alerts after ack")?;
        assert_eq!(after_alerts.len(), 1);
        assert_eq!(
            after_alerts[0]
                .get("acknowledged")
                .and_then(Value::as_bool)
                .ok_or("missing acknowledged")?,
            true
        );
        assert_eq!(
            after_alerts[0]
                .get("acknowledged_by_user_id")
                .and_then(Value::as_str)
                .ok_or("missing acknowledged_by_user_id")?,
            user_id.to_string()
        );

        let viewer_ack_req = request_with_tenant_and_role_and_user_and_secret(
            "POST",
            "/v1/audit/compliance/siem/deliveries/alerts/ack",
            Some("single"),
            Some("viewer"),
            Some(user_id),
            None,
            json!({
                "run_id": run_id,
                "delivery_target": "https://siem-a.example/hec"
            }),
        )?;
        let viewer_ack_resp = app.clone().oneshot(viewer_ack_req).await?;
        assert_eq!(viewer_ack_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn replay_compliance_audit_siem_delivery_requeues_dead_letter_only(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let run_id = Uuid::new_v4();
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
                input_json, requested_capabilities, granted_capabilities
            )
            VALUES ($1, 'single', $2, $3, 'payments_v1', 'running', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
            "#,
        )
        .bind(run_id)
        .bind(agent_id)
        .bind(user_id)
        .execute(&test_db.app_pool)
        .await?;

        let dead_letter_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO compliance_siem_delivery_outbox (
                id, tenant_id, run_id, adapter, delivery_target, content_type, payload_ndjson,
                status, attempts, max_attempts, next_attempt_at, last_error, last_http_status
            )
            VALUES (
                $1, 'single', $2, 'splunk_hec', 'https://siem-a.example/hec', 'application/x-ndjson', '{"event":"a"}',
                'dead_lettered', 3, 3, now(), 'failed permanently', 500
            )
            "#,
        )
        .bind(dead_letter_id)
        .bind(run_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let replay_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/audit/compliance/siem/deliveries/{dead_letter_id}/replay"),
            Some("single"),
            Some("operator"),
            json!({"delay_secs": 1}),
        )?;
        let replay_resp = app.clone().oneshot(replay_req).await?;
        assert_eq!(replay_resp.status(), StatusCode::ACCEPTED);
        let replay_body = response_json(replay_resp).await?;
        assert_eq!(
            replay_body
                .get("status")
                .and_then(Value::as_str)
                .ok_or("missing replay status")?,
            "pending"
        );
        assert_eq!(
            replay_body
                .get("attempts")
                .and_then(Value::as_i64)
                .ok_or("missing replay attempts")?,
            0
        );

        let second_replay_req = request_with_tenant_and_role(
            "POST",
            &format!("/v1/audit/compliance/siem/deliveries/{dead_letter_id}/replay"),
            Some("single"),
            Some("operator"),
            json!({}),
        )?;
        let second_replay_resp = app.clone().oneshot(second_replay_req).await?;
        assert_eq!(second_replay_resp.status(), StatusCode::NOT_FOUND);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_verify_returns_chain_status() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-main"
                }),
            },
        )
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/verify",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("verified")
                .and_then(Value::as_bool)
                .ok_or("missing verified")?,
            true
        );
        assert_eq!(
            body.get("checked_events")
                .and_then(Value::as_i64)
                .ok_or("missing checked_events")?,
            1
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_replay_package_returns_correlated_payload(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        let action_request_id = Uuid::new_v4();
        let payment_request_id = Uuid::new_v4();
        let payment_result_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-main"
                }),
            },
        )
        .await?;

        sqlx::query(
            r#"
            INSERT INTO action_requests (id, step_id, action_type, args_json, status)
            VALUES ($1, $2, 'payment.send', '{}'::jsonb, 'executed')
            "#,
        )
        .bind(action_request_id)
        .bind(step_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO payment_requests (
                id, action_request_id, run_id, tenant_id, agent_id, provider, operation,
                destination, idempotency_key, amount_msat, request_json, status
            )
            VALUES (
                $1, $2, $3, 'single', $4, 'nwc', 'pay_invoice',
                'nwc:wallet-main', 'replay-key-1', 1000, '{}'::jsonb, 'executed'
            )
            "#,
        )
        .bind(payment_request_id)
        .bind(action_request_id)
        .bind(run_id)
        .bind(agent_id)
        .execute(&test_db.app_pool)
        .await?;
        sqlx::query(
            "INSERT INTO payment_results (id, payment_request_id, status, result_json) VALUES ($1, $2, 'executed', '{\"settlement_status\":\"settled\"}'::jsonb)",
        )
        .bind(payment_result_id)
        .bind(payment_request_id)
        .execute(&test_db.app_pool)
        .await?;

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance/replay-package?run_id={run_id}&audit_limit=50&compliance_limit=50&payment_limit=50"),
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("tenant_id")
                .and_then(Value::as_str)
                .ok_or("missing tenant_id")?,
            "single"
        );
        assert_eq!(
            body.get("run")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                .ok_or("missing run.id")?,
            run_id.to_string()
        );
        assert_eq!(
            body.get("run_audit_events")
                .and_then(Value::as_array)
                .map(Vec::len)
                .ok_or("missing run_audit_events")?,
            2
        );
        assert_eq!(
            body.get("compliance_audit_events")
                .and_then(Value::as_array)
                .map(Vec::len)
                .ok_or("missing compliance_audit_events")?,
            1
        );
        assert_eq!(
            body.get("payment_ledger")
                .and_then(Value::as_array)
                .map(Vec::len)
                .ok_or("missing payment_ledger")?,
            1
        );
        assert_eq!(
            body.get("correlation")
                .and_then(|value| value.get("payment_event_count"))
                .and_then(Value::as_u64)
                .ok_or("missing correlation.payment_event_count")?,
            1
        );
        assert_eq!(
            body.get("manifest")
                .and_then(|value| value.get("version"))
                .and_then(Value::as_str)
                .ok_or("missing manifest.version")?,
            "v1"
        );
        assert_eq!(
            body.get("manifest")
                .and_then(|value| value.get("signing_mode"))
                .and_then(Value::as_str)
                .ok_or("missing manifest.signing_mode")?,
            "unsigned"
        );
        assert!(body
            .get("manifest")
            .and_then(|value| value.get("signature"))
            .is_some_and(Value::is_null));
        assert_eq!(
            body.get("manifest")
                .and_then(|value| value.get("digest_sha256"))
                .and_then(Value::as_str)
                .ok_or("missing manifest.digest_sha256")?
                .len(),
            64
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_policy_returns_defaults() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/policy",
            Some("single"),
            Some("operator"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_json(resp).await?;
        assert_eq!(
            body.get("compliance_hot_retention_days")
                .and_then(Value::as_i64)
                .ok_or("missing hot retention")?,
            180
        );
        assert_eq!(
            body.get("compliance_archive_retention_days")
                .and_then(Value::as_i64)
                .ok_or("missing archive retention")?,
            2555
        );
        assert_eq!(
            body.get("legal_hold")
                .and_then(Value::as_bool)
                .ok_or("missing legal_hold")?,
            false
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn put_compliance_audit_policy_updates_and_requires_owner() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router(test_db.app_pool.clone());

        let operator_req = request_with_tenant_and_role(
            "PUT",
            "/v1/audit/compliance/policy",
            Some("single"),
            Some("operator"),
            json!({
                "compliance_hot_retention_days": 45,
                "compliance_archive_retention_days": 400,
                "legal_hold": true,
                "legal_hold_reason": "investigation"
            }),
        )?;
        let operator_resp = app.clone().oneshot(operator_req).await?;
        assert_eq!(operator_resp.status(), StatusCode::FORBIDDEN);

        let owner_req = request_with_tenant_and_role(
            "PUT",
            "/v1/audit/compliance/policy",
            Some("single"),
            Some("owner"),
            json!({
                "compliance_hot_retention_days": 45,
                "compliance_archive_retention_days": 400,
                "legal_hold": true,
                "legal_hold_reason": "investigation"
            }),
        )?;
        let owner_resp = app.clone().oneshot(owner_req).await?;
        assert_eq!(owner_resp.status(), StatusCode::OK);
        let owner_body = response_json(owner_resp).await?;
        assert_eq!(
            owner_body
                .get("compliance_hot_retention_days")
                .and_then(Value::as_i64)
                .ok_or("missing updated hot retention")?,
            45
        );
        assert_eq!(
            owner_body
                .get("legal_hold")
                .and_then(Value::as_bool)
                .ok_or("missing updated legal hold")?,
            true
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn post_compliance_audit_purge_respects_legal_hold() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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
        let first = agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-main"
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

        let app = api::app_router(test_db.app_pool.clone());
        let purge_req = request_with_tenant_and_role(
            "POST",
            "/v1/audit/compliance/purge",
            Some("single"),
            Some("owner"),
            Value::Null,
        )?;
        let purge_resp = app.clone().oneshot(purge_req).await?;
        assert_eq!(purge_resp.status(), StatusCode::OK);
        let purge_body = response_json(purge_resp).await?;
        assert_eq!(
            purge_body
                .get("deleted_count")
                .and_then(Value::as_i64)
                .ok_or("missing deleted_count")?,
            1
        );

        let second = agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-hold"
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

        let hold_req = request_with_tenant_and_role(
            "PUT",
            "/v1/audit/compliance/policy",
            Some("single"),
            Some("owner"),
            json!({
                "legal_hold": true,
                "legal_hold_reason": "investigation-lock"
            }),
        )?;
        let hold_resp = app.clone().oneshot(hold_req).await?;
        assert_eq!(hold_resp.status(), StatusCode::OK);

        let purge_with_hold_req = request_with_tenant_and_role(
            "POST",
            "/v1/audit/compliance/purge",
            Some("single"),
            Some("owner"),
            Value::Null,
        )?;
        let purge_with_hold_resp = app.clone().oneshot(purge_with_hold_req).await?;
        assert_eq!(purge_with_hold_resp.status(), StatusCode::OK);
        let purge_with_hold_body = response_json(purge_with_hold_resp).await?;
        assert_eq!(
            purge_with_hold_body
                .get("deleted_count")
                .and_then(Value::as_i64)
                .ok_or("missing deleted_count with hold")?,
            0
        );
        assert_eq!(
            purge_with_hold_body
                .get("legal_hold")
                .and_then(Value::as_bool)
                .ok_or("missing legal_hold with hold")?,
            true
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn get_compliance_audit_rejects_viewer_role() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let app = api::app_router(test_db.app_pool.clone());
        let req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance?limit=10",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let export_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/export?limit=10",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let export_resp = app.clone().oneshot(export_req).await?;
        assert_eq!(export_resp.status(), StatusCode::FORBIDDEN);

        let siem_export_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/export?limit=10",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let siem_export_resp = app.clone().oneshot(siem_export_req).await?;
        assert_eq!(siem_export_resp.status(), StatusCode::FORBIDDEN);

        let verify_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/verify",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let verify_resp = app.clone().oneshot(verify_req).await?;
        assert_eq!(verify_resp.status(), StatusCode::FORBIDDEN);

        let policy_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/policy",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let policy_resp = app.clone().oneshot(policy_req).await?;
        assert_eq!(policy_resp.status(), StatusCode::FORBIDDEN);

        let policy_put_req = request_with_tenant_and_role(
            "PUT",
            "/v1/audit/compliance/policy",
            Some("single"),
            Some("viewer"),
            json!({"legal_hold": true}),
        )?;
        let policy_put_resp = app.clone().oneshot(policy_put_req).await?;
        assert_eq!(policy_put_resp.status(), StatusCode::FORBIDDEN);

        let purge_req = request_with_tenant_and_role(
            "POST",
            "/v1/audit/compliance/purge",
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let purge_resp = app.clone().oneshot(purge_req).await?;
        assert_eq!(purge_resp.status(), StatusCode::FORBIDDEN);

        let replay_req = request_with_tenant_and_role(
            "GET",
            &format!(
                "/v1/audit/compliance/replay-package?run_id={}",
                Uuid::new_v4()
            ),
            Some("single"),
            Some("viewer"),
            Value::Null,
        )?;
        let replay_resp = app.clone().oneshot(replay_req).await?;
        assert_eq!(replay_resp.status(), StatusCode::FORBIDDEN);

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn compliance_endpoints_are_tenant_isolated() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let step_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
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
        agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
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
        agent_core::append_audit_event(
            &test_db.app_pool,
            &agent_core::NewAuditEvent {
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
                    "destination": "nwc:wallet-main"
                }),
            },
        )
        .await?;

        let app = api::app_router(test_db.app_pool.clone());

        let list_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance?limit=10",
            Some("other"),
            Some("operator"),
            Value::Null,
        )?;
        let list_resp = app.clone().oneshot(list_req).await?;
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = response_json(list_resp).await?;
        assert_eq!(list_body.as_array().map(Vec::len), Some(0));

        let export_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/export?limit=10",
            Some("other"),
            Some("operator"),
            Value::Null,
        )?;
        let export_resp = app.clone().oneshot(export_req).await?;
        assert_eq!(export_resp.status(), StatusCode::OK);
        let export_bytes = to_bytes(export_resp.into_body(), usize::MAX).await?;
        assert_eq!(String::from_utf8(export_bytes.to_vec())?, "");

        let siem_export_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/siem/export?limit=10&adapter=splunk_hec",
            Some("other"),
            Some("operator"),
            Value::Null,
        )?;
        let siem_export_resp = app.clone().oneshot(siem_export_req).await?;
        assert_eq!(siem_export_resp.status(), StatusCode::OK);
        let siem_export_bytes = to_bytes(siem_export_resp.into_body(), usize::MAX).await?;
        assert_eq!(String::from_utf8(siem_export_bytes.to_vec())?, "");

        let verify_req = request_with_tenant_and_role(
            "GET",
            "/v1/audit/compliance/verify",
            Some("other"),
            Some("operator"),
            Value::Null,
        )?;
        let verify_resp = app.clone().oneshot(verify_req).await?;
        assert_eq!(verify_resp.status(), StatusCode::OK);
        let verify_body = response_json(verify_resp).await?;
        assert_eq!(
            verify_body
                .get("checked_events")
                .and_then(Value::as_i64)
                .ok_or("missing checked_events")?,
            0
        );
        assert_eq!(
            verify_body
                .get("verified")
                .and_then(Value::as_bool)
                .ok_or("missing verified")?,
            true
        );

        let replay_req = request_with_tenant_and_role(
            "GET",
            &format!("/v1/audit/compliance/replay-package?run_id={run_id}"),
            Some("other"),
            Some("operator"),
            Value::Null,
        )?;
        let replay_resp = app.clone().oneshot(replay_req).await?;
        assert_eq!(replay_resp.status(), StatusCode::NOT_FOUND);

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

#[test]
fn create_run_uses_recipe_bundle_when_requested_capabilities_empty(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "notify_v1",
                "input": {"text":"hello"},
                "requested_capabilities": []
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = response_json(resp).await?;
        let granted = body
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;

        assert_eq!(granted.len(), 2);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 0")?,
            "message.send"
        );
        assert_eq!(
            granted[1]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 1")?,
            "llm.infer"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_payments_bundle_grants_payment_send() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "payments_v1",
                "input": {"operation":"pay_invoice"},
                "requested_capabilities": []
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = response_json(resp).await?;
        let granted = body
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;

        assert_eq!(granted.len(), 1);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 0")?,
            "payment.send"
        );
        assert_eq!(
            granted[0]
                .get("scope")
                .and_then(Value::as_str)
                .ok_or("missing scope 0")?,
            "nwc:*"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_payments_cashu_bundle_grants_cashu_scope() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "payments_cashu_v1",
                "input": {"operation":"pay_invoice"},
                "requested_capabilities": []
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = response_json(resp).await?;
        let granted = body
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;

        assert_eq!(granted.len(), 1);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 0")?,
            "payment.send"
        );
        assert_eq!(
            granted[0]
                .get("scope")
                .and_then(Value::as_str)
                .ok_or("missing scope 0")?,
            "cashu:*"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_intersects_requested_capabilities_with_recipe_bundle(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant(
            "POST",
            "/v1/runs",
            Some("single"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"text":"hello"},
                "requested_capabilities": [
                    {"capability":"message.send","scope":"whitenoise:npub1allowed"},
                    {"capability":"message.send","scope":"slack:C123456"},
                    {"capability":"llm.infer","scope":"remote:*"},
                    {"capability":"local.exec","scope":"local.exec:file.word_count"}
                ]
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = response_json(resp).await?;
        let granted = body
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;

        assert_eq!(granted.len(), 1);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 0")?,
            "message.send"
        );
        assert_eq!(
            granted[0]
                .get("scope")
                .and_then(Value::as_str)
                .ok_or("missing scope 0")?,
            "whitenoise:npub1allowed"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_applies_operator_role_preset() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant_and_role(
            "POST",
            "/v1/runs",
            Some("single"),
            Some("operator"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"text":"hello"},
                "requested_capabilities": []
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = response_json(resp).await?;
        let granted = body
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;

        assert_eq!(granted.len(), 4);
        assert!(granted
            .iter()
            .all(|item| item.get("capability").and_then(Value::as_str) != Some("local.exec")));

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_applies_viewer_role_preset() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant_and_role(
            "POST",
            "/v1/runs",
            Some("single"),
            Some("viewer"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"text":"hello"},
                "requested_capabilities": []
            }),
        )?;

        let resp = app.clone().oneshot(req).await?;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = response_json(resp).await?;
        let granted = body
            .get("granted_capabilities")
            .and_then(Value::as_array)
            .ok_or("missing granted_capabilities")?;

        assert_eq!(granted.len(), 2);
        assert_eq!(
            granted[0]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 0")?,
            "object.read"
        );
        assert_eq!(
            granted[1]
                .get("capability")
                .and_then(Value::as_str)
                .ok_or("missing capability 1")?,
            "llm.infer"
        );
        assert_eq!(
            granted[1]
                .get("scope")
                .and_then(Value::as_str)
                .ok_or("missing scope 1")?,
            "local:*"
        );

        teardown_test_db(test_db).await?;
        Ok(())
    })
}

#[test]
fn create_run_rejects_invalid_user_role_header() -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let app = api::app_router(test_db.app_pool.clone());

        let req = request_with_tenant_and_role(
            "POST",
            "/v1/runs",
            Some("single"),
            Some("superadmin"),
            json!({
                "agent_id": agent_id,
                "triggered_by_user_id": user_id,
                "recipe_id": "show_notes_v1",
                "input": {"text":"hello"},
                "requested_capabilities": []
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
    .bind("secureagnt_api_test")
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
    request_with_tenant_and_role_and_user_and_secret(
        method, uri, tenant_id, None, None, None, json_body,
    )
}

fn request_with_tenant_and_role(
    method: &str,
    uri: &str,
    tenant_id: Option<&str>,
    user_role: Option<&str>,
    json_body: Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    request_with_tenant_and_role_and_user_and_secret(
        method, uri, tenant_id, user_role, None, None, json_body,
    )
}

fn request_with_tenant_and_role_and_user(
    method: &str,
    uri: &str,
    tenant_id: Option<&str>,
    user_role: Option<&str>,
    user_id: Option<Uuid>,
    json_body: Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    request_with_tenant_and_role_and_user_and_secret(
        method, uri, tenant_id, user_role, user_id, None, json_body,
    )
}

fn request_with_tenant_and_role_and_secret(
    method: &str,
    uri: &str,
    tenant_id: Option<&str>,
    user_role: Option<&str>,
    trigger_secret: Option<&str>,
    json_body: Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    request_with_tenant_and_role_and_user_and_secret(
        method,
        uri,
        tenant_id,
        user_role,
        None,
        trigger_secret,
        json_body,
    )
}

fn request_with_tenant_and_role_and_proxy_token(
    method: &str,
    uri: &str,
    tenant_id: Option<&str>,
    user_role: Option<&str>,
    proxy_token: Option<&str>,
    json_body: Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    let mut request = request_with_tenant_and_role(method, uri, tenant_id, user_role, json_body)?;
    if let Some(proxy_token) = proxy_token {
        request
            .headers_mut()
            .insert("x-auth-proxy-token", HeaderValue::from_str(proxy_token)?);
    }
    Ok(request)
}

fn request_with_tenant_and_role_and_user_and_secret(
    method: &str,
    uri: &str,
    tenant_id: Option<&str>,
    user_role: Option<&str>,
    user_id: Option<Uuid>,
    trigger_secret: Option<&str>,
    json_body: Value,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(tenant_id) = tenant_id {
        builder = builder.header("x-tenant-id", tenant_id);
    }
    if let Some(user_role) = user_role {
        builder = builder.header("x-user-role", user_role);
    }
    if let Some(user_id) = user_id {
        builder = builder.header("x-user-id", user_id.to_string());
    }
    if let Some(trigger_secret) = trigger_secret {
        builder = builder.header("x-trigger-secret", trigger_secret);
    }

    let request = if method == "GET" {
        builder.body(Body::empty())?
    } else {
        builder
            .header("content-type", "application/json")
            .body(Body::from(json_body.to_string()))?
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

fn make_temp_context_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "secureagnt_api_context_test_{}_{}",
        label,
        Uuid::new_v4()
    ));
    fs::create_dir_all(&root).expect("create temp context root");
    root
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
