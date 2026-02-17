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
        std::env::set_var("AEGIS_TRIGGER_SECRET_TEST", "super-secret");

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
                "webhook_secret_ref": "env:AEGIS_TRIGGER_SECRET_TEST",
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

        std::env::remove_var("AEGIS_TRIGGER_SECRET_TEST");
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

fn run_async<F>(future: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(future)
}
