use core as agent_core;
use futures_util::{SinkExt, StreamExt};
use nostr::nips::nip04;
use nostr::nips::nip44;
use nostr::nips::nip46::{
    NostrConnectMessage, NostrConnectRequest, NostrConnectResponse,
    ResponseResult as Nip46ResponseResult,
};
use nostr::nips::nip47::{
    Method as NwcMethod, Request as NwcRequest, RequestParams as NwcRequestParams,
    Response as NwcResponse, ResponseResult as NwcResponseResult,
};
use nostr::{
    ClientMessage, EventBuilder, JsonUtil, Keys, Kind, PublicKey, RelayMessage, SecretKey, Tag,
    ToBech32,
};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool, Row,
};
use std::{collections::BTreeMap, env, fs, path::PathBuf, str::FromStr, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};
use uuid::Uuid;
use worker::llm::{LlmConfig, LlmEndpointConfig, LlmMode};
use worker::local_exec::LocalExecConfig;
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
fn worker_process_once_dispatches_due_interval_trigger_and_processes_run(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_trigger_dispatch_success");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();

        agent_core::create_interval_trigger(
            &test_db.app_pool,
            &agent_core::NewIntervalTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
                interval_seconds: 60,
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
                next_fire_at: time::OffsetDateTime::now_utc() - time::Duration::seconds(1),
                status: "enabled".to_string(),
                misfire_policy: "fire_now".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-trigger-1", artifact_root.clone());
        config.trigger_scheduler_enabled = true;

        let outcome = process_once(&test_db.app_pool, &config).await?;
        let run_id = match outcome {
            WorkerCycleOutcome::ClaimedAndSucceeded { run_id } => run_id,
            other => return Err(format!("expected claimed success outcome, got {other:?}").into()),
        };

        let run_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM runs WHERE id = $1 AND status = 'succeeded'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(run_count, 1);

        let trigger_run_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM trigger_runs WHERE trigger_id = $1 AND run_id = $2",
        )
        .bind(trigger_id)
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(trigger_run_count, 1);

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_dispatches_webhook_trigger_event_and_processes_run(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_webhook_trigger_dispatch_success");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let trigger_id = Uuid::new_v4();

        agent_core::create_webhook_trigger(
            &test_db.app_pool,
            &agent_core::NewWebhookTrigger {
                id: trigger_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "show_notes_v1".to_string(),
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
                status: "enabled".to_string(),
                max_attempts: 3,
                max_inflight_runs: 1,
                jitter_seconds: 0,
                webhook_secret_ref: None,
            },
        )
        .await?;

        let enqueue = agent_core::enqueue_trigger_event(
            &test_db.app_pool,
            "single",
            trigger_id,
            "evt-worker-1",
            json!({"source":"worker-test"}),
        )
        .await?;
        assert_eq!(enqueue, agent_core::TriggerEventEnqueueOutcome::Enqueued);

        let mut config = worker_test_config("worker-test-trigger-webhook-1", artifact_root.clone());
        config.trigger_scheduler_enabled = true;

        let outcome = process_once(&test_db.app_pool, &config).await?;
        let run_id = match outcome {
            WorkerCycleOutcome::ClaimedAndSucceeded { run_id } => run_id,
            other => return Err(format!("expected claimed success outcome, got {other:?}").into()),
        };

        let run_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM runs WHERE id = $1 AND status = 'succeeded'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(run_count, 1);

        let processed_events: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM trigger_events WHERE trigger_id = $1 AND status = 'processed'",
        )
        .bind(trigger_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(processed_events, 1);

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
                    "destination": format!("whitenoise:{}", destination_npub())
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
                    "destination": format!("whitenoise:{}", destination_npub())
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
fn worker_process_once_executes_payment_send_with_nwc_mock(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_send_success");
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
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-001",
                    "payment_amount_msat": 2100,
                    "payment_invoice": "lnbc1mock"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-payment", artifact_root.clone());
        config.payment_nwc_enabled = true;
        config.payment_max_spend_msat_per_run = Some(50_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let payment_status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar JOIN steps s ON s.id = ar.step_id WHERE s.run_id = $1 AND ar.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(payment_status, "executed");

        let payment_request_status: String =
            sqlx::query_scalar("SELECT status FROM payment_requests WHERE run_id = $1 LIMIT 1")
                .bind(run_id)
                .fetch_one(&test_db.app_pool)
                .await?;
        assert_eq!(payment_request_status, "executed");

        let payment_result_status: String = sqlx::query_scalar(
            "SELECT pr.status FROM payment_results pr JOIN payment_requests pq ON pq.id = pr.payment_request_id WHERE pq.run_id = $1 LIMIT 1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(payment_result_status, "executed");

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_executes_payment_send_with_nwc_relay(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_send_nwc_relay");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (nwc_uri, nwc_request_rx) = spawn_mock_nwc_wallet_relay().await?;

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-live-001",
                    "payment_amount_msat": 2100,
                    "payment_invoice": "lnbc1mocklive"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-payment-live", artifact_root.clone());
        config.payment_nwc_enabled = true;
        config.payment_nwc_uri = Some(nwc_uri);
        config.payment_nwc_timeout = Duration::from_millis(2_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let relay_request = tokio::time::timeout(Duration::from_secs(2), nwc_request_rx)
            .await
            .map_err(|_| "timed out waiting for NWC relay request payload")?
            .map_err(|_| "NWC relay sender dropped")?;

        assert_eq!(relay_request.method, NwcMethod::PayInvoice);
        let NwcRequestParams::PayInvoice(pay_request) = relay_request.params else {
            return Err("unexpected NWC request params".into());
        };
        assert_eq!(pay_request.invoice, "lnbc1mocklive");
        assert_eq!(pay_request.amount, Some(2100));

        let payment_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;

        assert_eq!(
            payment_result
                .get("result")
                .and_then(|value| value.get("rail"))
                .and_then(serde_json::Value::as_str),
            Some("nwc_nip47")
        );
        assert!(payment_result
            .get("result")
            .and_then(|value| value.get("nwc"))
            .and_then(|value| value.get("request_event_id"))
            .and_then(serde_json::Value::as_str)
            .is_some());

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_routes_payment_send_via_wallet_map_over_default(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_send_wallet_map_route");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (mapped_nwc_uri, mapped_request_rx) = spawn_mock_nwc_wallet_relay().await?;
        let (default_nwc_uri, default_request_rx) = spawn_mock_nwc_wallet_relay().await?;

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-wallet-map-001",
                    "payment_amount_msat": 2100,
                    "payment_invoice": "lnbc1walletmap"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config =
            worker_test_config("worker-test-payment-wallet-map", artifact_root.clone());
        config.payment_nwc_enabled = true;
        config.payment_nwc_uri = Some(default_nwc_uri);
        config
            .payment_nwc_wallet_uris
            .insert("wallet-main".to_string(), mapped_nwc_uri);
        config.payment_nwc_timeout = Duration::from_millis(2_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let mapped_request = tokio::time::timeout(Duration::from_secs(2), mapped_request_rx)
            .await
            .map_err(|_| "timed out waiting for mapped wallet relay request")?
            .map_err(|_| "mapped wallet relay sender dropped")?;
        assert_eq!(mapped_request.method, NwcMethod::PayInvoice);

        let default_seen =
            tokio::time::timeout(Duration::from_millis(750), default_request_rx).await;
        if default_seen.is_ok() {
            return Err("default PAYMENT_NWC_URI relay should not have received request".into());
        }

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_fails_payment_send_when_wallet_map_missing_target(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_send_wallet_map_missing");
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
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-wallet-map-missing-001",
                    "payment_amount_msat": 2100,
                    "payment_invoice": "lnbc1walletmissing"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config(
            "worker-test-payment-wallet-map-missing",
            artifact_root.clone(),
        );
        config.payment_nwc_enabled = true;
        config.payment_nwc_wallet_uris.insert(
            "wallet-alt".to_string(),
            "nostr+walletconnect://not-used".to_string(),
        );

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let failed_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(failed_result
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("not configured"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_fails_over_between_wallet_routes_when_enabled(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_send_wallet_failover_enabled");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (mapped_nwc_uri, mapped_request_rx) = spawn_mock_nwc_wallet_relay().await?;

        let (prefix, _) = mapped_nwc_uri
            .split_once("&relay=")
            .ok_or("mock nwc uri missing relay parameter")?;
        let unreachable_route = format!("{prefix}&relay=ws://127.0.0.1:9");
        let routed_value = format!("{unreachable_route}|{mapped_nwc_uri}");

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-wallet-route-failover-001",
                    "payment_amount_msat": 2100,
                    "payment_invoice": "lnbc1walletfailover"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config(
            "worker-test-payment-wallet-failover-enabled",
            artifact_root.clone(),
        );
        config.payment_nwc_enabled = true;
        config.payment_nwc_timeout = Duration::from_millis(500);
        config
            .payment_nwc_wallet_uris
            .insert("wallet-main".to_string(), routed_value);
        config.payment_nwc_route_fallback_enabled = true;

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let mapped_request = tokio::time::timeout(Duration::from_secs(2), mapped_request_rx)
            .await
            .map_err(|_| "timed out waiting for mapped wallet relay request")?
            .map_err(|_| "mapped wallet relay sender dropped")?;
        assert_eq!(mapped_request.method, NwcMethod::PayInvoice);

        let payment_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;

        assert_eq!(
            payment_result
                .get("result")
                .and_then(|value| value.get("nwc"))
                .and_then(|value| value.get("route"))
                .and_then(|value| value.get("selected_route_index"))
                .and_then(serde_json::Value::as_u64),
            Some(2)
        );

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_does_not_fail_over_between_wallet_routes_when_disabled(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_send_wallet_failover_disabled");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (mapped_nwc_uri, mapped_request_rx) = spawn_mock_nwc_wallet_relay().await?;

        let (prefix, _) = mapped_nwc_uri
            .split_once("&relay=")
            .ok_or("mock nwc uri missing relay parameter")?;
        let unreachable_route = format!("{prefix}&relay=ws://127.0.0.1:9");
        let routed_value = format!("{unreachable_route}|{mapped_nwc_uri}");

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-wallet-route-no-failover-001",
                    "payment_amount_msat": 2100,
                    "payment_invoice": "lnbc1walletnofailover"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config(
            "worker-test-payment-wallet-failover-disabled",
            artifact_root.clone(),
        );
        config.payment_nwc_enabled = true;
        config.payment_nwc_timeout = Duration::from_millis(500);
        config
            .payment_nwc_wallet_uris
            .insert("wallet-main".to_string(), routed_value);
        config.payment_nwc_route_fallback_enabled = false;

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let not_seen = tokio::time::timeout(Duration::from_millis(750), mapped_request_rx).await;
        if not_seen.is_ok() {
            return Err(
                "secondary wallet route should not be attempted when failover is disabled".into(),
            );
        }

        let failed_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(failed_result
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("route 1"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_payment_send_when_approval_required(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_approval_required");
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
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-approval-001",
                    "payment_amount_msat": 5000,
                    "payment_invoice": "lnbc1mock"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config(
            "worker-test-payment-approval-required",
            artifact_root.clone(),
        );
        config.payment_nwc_enabled = true;
        config.payment_approval_threshold_msat = Some(2000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let failed_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(failed_result
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("requires approval"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_allows_payment_send_when_approved_over_threshold(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_approval_granted");
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
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-approval-002",
                    "payment_amount_msat": 5000,
                    "payment_approved": true,
                    "payment_invoice": "lnbc1mock"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config(
            "worker-test-payment-approval-granted",
            artifact_root.clone(),
        );
        config.payment_nwc_enabled = true;
        config.payment_approval_threshold_msat = Some(2000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let payment_result_status: String = sqlx::query_scalar(
            "SELECT pr.status FROM payment_results pr JOIN payment_requests pq ON pq.id = pr.payment_request_id WHERE pq.run_id = $1 LIMIT 1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(payment_result_status, "executed");

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_payment_send_when_run_budget_exceeded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_budget_exceeded");
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
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-budget-001",
                    "payment_amount_msat": 5000,
                    "payment_invoice": "lnbc1mock"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-payment-budget", artifact_root.clone());
        config.payment_nwc_enabled = true;
        config.payment_max_spend_msat_per_run = Some(1000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let action_status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar JOIN steps s ON s.id = ar.step_id WHERE s.run_id = $1 AND ar.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(action_status, "failed");

        let failed_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(failed_result
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("budget exceeded"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_payment_send_when_tenant_budget_exceeded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_tenant_budget_exceeded");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;

        // Seed historical executed spend for tenant.
        let historical_run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: historical_run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "succeeded".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        let historical_step = agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
                id: Uuid::new_v4(),
                run_id: historical_run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "historical_payment".to_string(),
                status: "succeeded".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;
        let historical_action = agent_core::create_action_request(
            &test_db.app_pool,
            &agent_core::NewActionRequest {
                id: Uuid::new_v4(),
                step_id: historical_step.id,
                action_type: "payment.send".to_string(),
                args_json: json!({}),
                justification: None,
                status: "executed".to_string(),
                decision_reason: None,
            },
        )
        .await?;
        let historical_payment = agent_core::create_or_get_payment_request(
            &test_db.app_pool,
            &agent_core::NewPaymentRequest {
                id: Uuid::new_v4(),
                action_request_id: historical_action.id,
                run_id: historical_run_id,
                tenant_id: "single".to_string(),
                agent_id,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: "historical-tenant-001".to_string(),
                amount_msat: Some(9_000),
                request_json: json!({}),
                status: "requested".to_string(),
            },
        )
        .await?;
        let _ = agent_core::update_payment_request_status(
            &test_db.app_pool,
            historical_payment.id,
            "executed",
        )
        .await?;

        let run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-tenant-budget-001",
                    "payment_amount_msat": 2000,
                    "payment_invoice": "lnbc1mock"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config =
            worker_test_config("worker-test-payment-tenant-budget", artifact_root.clone());
        config.payment_nwc_enabled = true;
        config.payment_max_spend_msat_per_tenant = Some(10_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let failed_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(failed_result
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("tenant spend budget exceeded"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_payment_send_when_agent_budget_exceeded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_payment_agent_budget_exceeded");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;

        let historical_run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: historical_run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "succeeded".to_string(),
                input_json: json!({}),
                requested_capabilities: json!([]),
                granted_capabilities: json!([]),
                error_json: None,
            },
        )
        .await?;
        let historical_step = agent_core::create_step(
            &test_db.app_pool,
            &agent_core::NewStep {
                id: Uuid::new_v4(),
                run_id: historical_run_id,
                tenant_id: "single".to_string(),
                agent_id,
                user_id: Some(user_id),
                name: "historical_payment_agent".to_string(),
                status: "succeeded".to_string(),
                input_json: json!({}),
                error_json: None,
            },
        )
        .await?;
        let historical_action = agent_core::create_action_request(
            &test_db.app_pool,
            &agent_core::NewActionRequest {
                id: Uuid::new_v4(),
                step_id: historical_step.id,
                action_type: "payment.send".to_string(),
                args_json: json!({}),
                justification: None,
                status: "executed".to_string(),
                decision_reason: None,
            },
        )
        .await?;
        let historical_payment = agent_core::create_or_get_payment_request(
            &test_db.app_pool,
            &agent_core::NewPaymentRequest {
                id: Uuid::new_v4(),
                action_request_id: historical_action.id,
                run_id: historical_run_id,
                tenant_id: "single".to_string(),
                agent_id,
                provider: "nwc".to_string(),
                operation: "pay_invoice".to_string(),
                destination: "nwc:wallet-main".to_string(),
                idempotency_key: "historical-agent-001".to_string(),
                amount_msat: Some(9_000),
                request_json: json!({}),
                status: "requested".to_string(),
            },
        )
        .await?;
        let _ = agent_core::update_payment_request_status(
            &test_db.app_pool,
            historical_payment.id,
            "executed",
        )
        .await?;

        let run_id = Uuid::new_v4();
        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "payments_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "settle invoice",
                    "request_payment": true,
                    "payment_destination": "nwc:wallet-main",
                    "payment_operation": "pay_invoice",
                    "payment_idempotency_key": "pay-agent-budget-001",
                    "payment_amount_msat": 2000,
                    "payment_invoice": "lnbc1mock"
                }),
                requested_capabilities: json!([
                    {"capability":"payment.send","scope":"nwc:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"payment.send",
                        "scope":"nwc:*",
                        "limits":{"max_payload_bytes":16000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config =
            worker_test_config("worker-test-payment-agent-budget", artifact_root.clone());
        config.payment_nwc_enabled = true;
        config.payment_max_spend_msat_per_agent = Some(10_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let failed_result: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'payment.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(failed_result
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("agent spend budget exceeded"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_delivers_slack_message_via_webhook() -> Result<(), Box<dyn std::error::Error>>
{
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_slack_send_success");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (webhook_url, slack_request_rx) = spawn_mock_slack_webhook().await?;

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
                    "destination": "slack:C123456",
                    "message_text": "hello slack"
                }),
                requested_capabilities: json!([
                    {"capability":"message.send","scope":"slack:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"message.send",
                        "scope":"slack:*",
                        "limits":{"max_payload_bytes":500000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-slack", artifact_root.clone());
        config.slack_webhook_url = Some(webhook_url);
        config.slack_send_timeout = Duration::from_millis(2_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let slack_payload = tokio::time::timeout(Duration::from_secs(2), slack_request_rx)
            .await
            .map_err(|_| "timed out waiting for mock slack webhook payload")?
            .map_err(|_| "mock slack sender dropped")?;
        assert_eq!(
            slack_payload
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "C123456"
        );
        assert_eq!(
            slack_payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "hello slack"
        );

        let result_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(
            result_json
                .get("delivery_state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "delivered_slack"
        );
        assert_eq!(
            result_json
                .get("delivery_result")
                .and_then(|value| value.get("status_code"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
            200
        );

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_retries_slack_webhook_and_then_succeeds(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_slack_retry_success");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (webhook_url, slack_requests_rx) =
            spawn_mock_slack_webhook_sequence(vec![500, 200]).await?;

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
                    "destination": "slack:C123456",
                    "message_text": "hello slack retry"
                }),
                requested_capabilities: json!([
                    {"capability":"message.send","scope":"slack:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"message.send",
                        "scope":"slack:*",
                        "limits":{"max_payload_bytes":500000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-slack-retry", artifact_root.clone());
        config.slack_webhook_url = Some(webhook_url);
        config.slack_send_timeout = Duration::from_millis(2_000);
        config.slack_max_attempts = 3;
        config.slack_retry_backoff = Duration::from_millis(10);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let slack_requests = tokio::time::timeout(Duration::from_secs(2), slack_requests_rx)
            .await
            .map_err(|_| "timed out waiting for mock slack retry payloads")?
            .map_err(|_| "mock slack retry sender dropped")?;
        assert_eq!(slack_requests.len(), 2);
        assert!(slack_requests.iter().all(|payload| {
            payload
                .get("channel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                == "C123456"
        }));

        let result_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(
            result_json
                .get("delivery_state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "delivered_slack"
        );
        assert_eq!(
            result_json
                .get("delivery_result")
                .and_then(|value| value.get("attempts"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
            2
        );

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_dead_letters_slack_after_retry_exhaustion(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_slack_dead_letter");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (webhook_url, slack_requests_rx) =
            spawn_mock_slack_webhook_sequence(vec![500, 500, 500]).await?;

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
                    "destination": "slack:C123456",
                    "message_text": "hello dead letter"
                }),
                requested_capabilities: json!([
                    {"capability":"message.send","scope":"slack:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"message.send",
                        "scope":"slack:*",
                        "limits":{"max_payload_bytes":500000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-slack-dead-letter", artifact_root.clone());
        config.slack_webhook_url = Some(webhook_url);
        config.slack_send_timeout = Duration::from_millis(2_000);
        config.slack_max_attempts = 3;
        config.slack_retry_backoff = Duration::from_millis(10);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let slack_requests = tokio::time::timeout(Duration::from_secs(2), slack_requests_rx)
            .await
            .map_err(|_| "timed out waiting for mock slack dead-letter payloads")?
            .map_err(|_| "mock slack dead-letter sender dropped")?;
        assert_eq!(slack_requests.len(), 3);

        let result_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(
            result_json
                .get("delivery_state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "dead_lettered_local_outbox"
        );
        assert_eq!(
            result_json
                .get("delivery_context")
                .and_then(|value| value.get("attempts"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
            3
        );
        assert_eq!(
            result_json
                .get("delivery_context")
                .and_then(|value| value.get("status"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "dead_lettered"
        );
        assert!(result_json
            .get("delivery_error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("HTTP 500"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_publishes_whitenoise_message_send_to_relay(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_message_send_relay_publish");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (relay_url, relay_msg_rx) = spawn_mock_relay().await?;
        let destination = format!("whitenoise:{}", destination_npub());

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
                    "destination": destination
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

        let mut config = worker_test_config("worker-test-msg-relay", artifact_root.clone());
        config.nostr_signer = local_signer_config();
        config.nostr_relays = vec![relay_url];
        config.nostr_publish_timeout = Duration::from_millis(2_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let relay_payload = tokio::time::timeout(Duration::from_secs(2), relay_msg_rx)
            .await
            .map_err(|_| "timed out waiting for mock relay event payload")?
            .map_err(|_| "mock relay sender dropped")?;
        assert_eq!(
            relay_payload
                .get(0)
                .and_then(serde_json::Value::as_str)
                .ok_or("missing relay message kind")?,
            "EVENT"
        );

        let result_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(
            result_json
                .get("delivery_state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "published_nostr"
        );
        assert_eq!(
            result_json
                .get("delivery_result")
                .and_then(|value| value.get("accepted_relays"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
            1
        );
        assert!(
            result_json
                .get("delivery_result")
                .and_then(|value| value.get("event_id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .len()
                > 8
        );

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_publishes_whitenoise_message_send_with_nip46_signer(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_message_send_nip46_publish");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let client_secret =
            "4444444444444444444444444444444444444444444444444444444444444444".to_string();
        let (relay_url, bunker_uri, signer_npub, relay_msg_rx) =
            spawn_mock_nip46_bunker_relay().await?;
        let destination = format!("whitenoise:{}", destination_npub());

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
                    "destination": destination
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

        let mut config = worker_test_config("worker-test-msg-nip46", artifact_root.clone());
        config.nostr_signer = NostrSignerConfig {
            mode: NostrSignerMode::Nip46Signer,
            local_secret_key: None,
            local_secret_key_file: None,
            nip46_bunker_uri: Some(bunker_uri),
            nip46_public_key: Some(signer_npub.clone()),
            nip46_client_secret_key: Some(client_secret),
        };
        config.nostr_relays = vec![relay_url.clone()];
        config.nostr_publish_timeout = Duration::from_millis(2_000);

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let relay_payload = tokio::time::timeout(Duration::from_secs(2), relay_msg_rx)
            .await
            .map_err(|_| "timed out waiting for mock relay NIP-46 publish payload")?
            .map_err(|_| "mock relay sender dropped")?;
        assert_eq!(
            relay_payload
                .get(0)
                .and_then(serde_json::Value::as_str)
                .ok_or("missing relay message kind")?,
            "EVENT"
        );
        let published_pubkey = relay_payload
            .get(1)
            .and_then(|event| event.get("pubkey"))
            .and_then(serde_json::Value::as_str)
            .ok_or("published event missing pubkey")?;
        assert_eq!(
            published_pubkey,
            PublicKey::parse(&signer_npub).expect("npub parse").to_hex()
        );

        let result_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(
            result_json
                .get("delivery_state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "published_nostr"
        );
        assert_eq!(
            result_json
                .get("delivery_result")
                .and_then(|value| value.get("accepted_relays"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
            1
        );
        assert_eq!(
            result_json
                .get("nostr_public_key")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            signer_npub
        );
        assert_eq!(
            result_json
                .get("delivery_context")
                .and_then(|ctx| ctx.get("nip46_signer_relay"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            relay_url
        );
        assert!(
            result_json
                .get("delivery_context")
                .and_then(|ctx| ctx.get("nip46_client_public_key"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .len()
                > 12
        );

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_redacts_sensitive_message_payloads_in_db(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_message_send_redaction");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let secret_text =
            "ship this nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";

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
                    "destination": format!("whitenoise:{}", destination_npub()),
                    "message_text": secret_text
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

        let mut config = worker_test_config("worker-test-msg-redaction", artifact_root.clone());
        config.nostr_signer = local_signer_config();
        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let request_args: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.args_json FROM action_requests ar
             JOIN steps s ON s.id = ar.step_id
             WHERE s.run_id = $1 AND ar.action_type = 'message.send'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        let persisted_text = request_args
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        assert!(persisted_text.contains("[REDACTED]"));
        assert!(!persisted_text.contains("nsec1"));

        let audit_payload: serde_json::Value = sqlx::query_scalar(
            "SELECT payload_json FROM audit_events
             WHERE run_id = $1 AND event_type = 'action.executed'
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(!audit_payload.to_string().contains("nsec1"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_executes_local_exec_template_with_scoped_roots(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        if !command_available("head").await {
            eprintln!("skipping local.exec integration test; `head` is not available");
            return Ok(());
        }

        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_local_exec_success");
        let allowed_root = temp_artifact_root("worker_local_exec_allow");
        fs::create_dir_all(&allowed_root)?;
        let input_file = allowed_root.join("notes.txt");
        fs::write(&input_file, "line one\nline two\nline three\n")?;

        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "local_exec_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_local_exec": true,
                    "local_exec_template_id": "file.head",
                    "local_exec_path": input_file.to_string_lossy().to_string(),
                    "local_exec_lines": 1
                }),
                requested_capabilities: json!([
                    {"capability":"local.exec","scope":"local.exec:file.head"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"local.exec",
                        "scope":"local.exec:file.head",
                        "limits":{"max_payload_bytes":4096}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-local-exec-ok", artifact_root.clone());
        config.local_exec.enabled = true;
        config.local_exec.read_roots = vec![fs::canonicalize(&allowed_root)?];

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let result_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'local.exec'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(
            result_json
                .get("template_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "file.head"
        );
        assert_eq!(
            result_json
                .get("exit_code")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default(),
            0
        );
        assert!(result_json
            .get("stdout")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("line one"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        let _ = fs::remove_dir_all(&allowed_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_fails_local_exec_for_out_of_scope_path(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_local_exec_denied");
        let allowed_root = temp_artifact_root("worker_local_exec_allow_deny");
        fs::create_dir_all(&allowed_root)?;
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "local_exec_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_local_exec": true,
                    "local_exec_template_id": "file.head",
                    "local_exec_path": "/etc/passwd",
                    "local_exec_lines": 1
                }),
                requested_capabilities: json!([
                    {"capability":"local.exec","scope":"local.exec:file.head"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"local.exec",
                        "scope":"local.exec:file.head",
                        "limits":{"max_payload_bytes":4096}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-local-exec-deny", artifact_root.clone());
        config.local_exec.enabled = true;
        config.local_exec.read_roots = vec![fs::canonicalize(&allowed_root)?];

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar
             JOIN steps s ON s.id = ar.step_id
             WHERE s.run_id = $1 AND ar.action_type = 'local.exec'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(status, "failed");

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        let _ = fs::remove_dir_all(&allowed_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_executes_llm_infer_with_local_first_route(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_llm_local_first");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();
        let (llm_url, llm_request_rx) = spawn_mock_llm_server("local response").await?;

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "llm_local_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_llm": true,
                    "llm_prompt": "Summarize in one line"
                }),
                requested_capabilities: json!([
                    {"capability":"llm.infer","scope":"local:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"llm.infer",
                        "scope":"local:*",
                        "limits":{"max_payload_bytes":32000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-llm-local", artifact_root.clone());
        config.llm = LlmConfig {
            mode: LlmMode::LocalFirst,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: Some(LlmEndpointConfig {
                base_url: llm_url,
                model: "mock-local-model".to_string(),
                api_key: None,
            }),
            remote: None,
            remote_egress_enabled: false,
            remote_host_allowlist: Vec::new(),
            remote_token_budget_per_run: None,
            remote_token_budget_per_tenant: None,
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: None,
            remote_token_budget_window_secs: 86_400,
            remote_cost_per_1k_tokens_usd: 0.0,
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndSucceeded { run_id });

        let llm_request = tokio::time::timeout(Duration::from_secs(2), llm_request_rx)
            .await
            .map_err(|_| "timed out waiting for llm request payload")?
            .map_err(|_| "llm request sender dropped")?;
        assert_eq!(
            llm_request
                .get("model")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "mock-local-model"
        );

        let result_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.result_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(
            result_json
                .get("route")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "local"
        );
        assert_eq!(
            result_json
                .get("response_text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "local response"
        );

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_denies_llm_remote_when_only_local_scope_granted(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_llm_remote_denied");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "llm_local_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_llm": true,
                    "llm_prompt": "Summarize in one line",
                    "llm_prefer": "remote"
                }),
                requested_capabilities: json!([
                    {"capability":"llm.infer","scope":"local:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"llm.infer",
                        "scope":"local:*",
                        "limits":{"max_payload_bytes":32000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-llm-deny", artifact_root.clone());
        config.llm = LlmConfig {
            mode: LlmMode::LocalFirst,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: Some(LlmEndpointConfig {
                base_url: "http://127.0.0.1:9/v1".to_string(),
                model: "mock-local-model".to_string(),
                api_key: None,
            }),
            remote: Some(LlmEndpointConfig {
                base_url: "http://127.0.0.1:9/v1".to_string(),
                model: "mock-remote-model".to_string(),
                api_key: Some("x".to_string()),
            }),
            remote_egress_enabled: false,
            remote_host_allowlist: Vec::new(),
            remote_token_budget_per_run: None,
            remote_token_budget_per_tenant: None,
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: None,
            remote_token_budget_window_secs: 86_400,
            remote_cost_per_1k_tokens_usd: 0.0,
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar
             JOIN steps s ON s.id = ar.step_id
             WHERE s.run_id = $1 AND ar.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(status, "denied");

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_llm_remote_when_egress_disabled(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_llm_remote_egress_block");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "llm_remote_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_llm": true,
                    "llm_prompt": "Summarize in one line",
                    "llm_prefer": "remote"
                }),
                requested_capabilities: json!([
                    {"capability":"llm.infer","scope":"remote:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"llm.infer",
                        "scope":"remote:*",
                        "limits":{"max_payload_bytes":32000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-llm-remote-egress", artifact_root.clone());
        config.llm = LlmConfig {
            mode: LlmMode::LocalFirst,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: Some(LlmEndpointConfig {
                base_url: "http://127.0.0.1:9/v1".to_string(),
                model: "mock-local-model".to_string(),
                api_key: None,
            }),
            remote: Some(LlmEndpointConfig {
                base_url: "https://api.remote/v1".to_string(),
                model: "mock-remote-model".to_string(),
                api_key: Some("x".to_string()),
            }),
            remote_egress_enabled: false,
            remote_host_allowlist: vec!["api.remote".to_string()],
            remote_token_budget_per_run: None,
            remote_token_budget_per_tenant: None,
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: None,
            remote_token_budget_window_secs: 86_400,
            remote_cost_per_1k_tokens_usd: 0.0,
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar
             JOIN steps s ON s.id = ar.step_id
             WHERE s.run_id = $1 AND ar.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(status, "failed");

        let error_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(error_json
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("LLM_REMOTE_EGRESS_ENABLED"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_llm_remote_when_token_budget_exceeded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_llm_remote_budget_exceeded");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        let run_id = Uuid::new_v4();

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "llm_remote_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_llm": true,
                    "llm_prompt": "Summarize in one line",
                    "llm_prefer": "remote",
                    "llm_max_tokens": 1000
                }),
                requested_capabilities: json!([
                    {"capability":"llm.infer","scope":"remote:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"llm.infer",
                        "scope":"remote:*",
                        "limits":{"max_payload_bytes":32000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config("worker-test-llm-remote-budget", artifact_root.clone());
        config.llm = LlmConfig {
            mode: LlmMode::LocalFirst,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: Some(LlmEndpointConfig {
                base_url: "http://127.0.0.1:9/v1".to_string(),
                model: "mock-local-model".to_string(),
                api_key: None,
            }),
            remote: Some(LlmEndpointConfig {
                base_url: "https://api.remote/v1".to_string(),
                model: "mock-remote-model".to_string(),
                api_key: Some("x".to_string()),
            }),
            remote_egress_enabled: true,
            remote_host_allowlist: vec!["api.remote".to_string()],
            remote_token_budget_per_run: Some(100),
            remote_token_budget_per_tenant: None,
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: None,
            remote_token_budget_window_secs: 86_400,
            remote_cost_per_1k_tokens_usd: 0.0,
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let status: String = sqlx::query_scalar(
            "SELECT ar.status FROM action_requests ar
             JOIN steps s ON s.id = ar.step_id
             WHERE s.run_id = $1 AND ar.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert_eq!(status, "failed");

        let error_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(error_json
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("remote token budget exceeded"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_llm_remote_when_tenant_budget_window_exceeded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_llm_remote_tenant_budget_exceeded");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        seed_remote_llm_usage(
            &test_db.app_pool,
            "single",
            agent_id,
            user_id,
            "remote:mock-remote-model",
            90,
        )
        .await?;
        let run_id = Uuid::new_v4();
        let (llm_url, llm_request_rx) = spawn_mock_llm_server("remote response").await?;

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "llm_remote_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_llm": true,
                    "llm_prompt": "Summarize in one line",
                    "llm_prefer": "remote",
                    "llm_max_tokens": 20
                }),
                requested_capabilities: json!([
                    {"capability":"llm.infer","scope":"remote:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"llm.infer",
                        "scope":"remote:*",
                        "limits":{"max_payload_bytes":32000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config = worker_test_config(
            "worker-test-llm-tenant-budget-window",
            artifact_root.clone(),
        );
        config.llm = LlmConfig {
            mode: LlmMode::LocalFirst,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: None,
            remote: Some(LlmEndpointConfig {
                base_url: llm_url,
                model: "mock-remote-model".to_string(),
                api_key: Some("x".to_string()),
            }),
            remote_egress_enabled: true,
            remote_host_allowlist: vec!["127.0.0.1".to_string()],
            remote_token_budget_per_run: None,
            remote_token_budget_per_tenant: Some(100),
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: None,
            remote_token_budget_window_secs: 3600,
            remote_cost_per_1k_tokens_usd: 0.0,
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let not_seen = tokio::time::timeout(Duration::from_millis(750), llm_request_rx).await;
        if not_seen.is_ok() {
            return Err("remote LLM should not be called when tenant budget precheck fails".into());
        }

        let error_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(error_json
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("tenant token budget exceeded"));

        teardown_test_db(test_db).await?;
        let _ = fs::remove_dir_all(&artifact_root);
        Ok(())
    })
}

#[test]
fn worker_process_once_blocks_llm_remote_when_model_budget_window_exceeded(
) -> Result<(), Box<dyn std::error::Error>> {
    run_async(async {
        let Some(test_db) = setup_test_db().await? else {
            return Ok(());
        };

        let artifact_root = temp_artifact_root("worker_llm_remote_model_budget_exceeded");
        let (agent_id, user_id) = seed_agent_and_user(&test_db.app_pool).await?;
        seed_remote_llm_usage(
            &test_db.app_pool,
            "single",
            agent_id,
            user_id,
            "remote:mock-remote-model",
            50,
        )
        .await?;
        let run_id = Uuid::new_v4();
        let (llm_url, llm_request_rx) = spawn_mock_llm_server("remote response").await?;

        agent_core::create_run(
            &test_db.app_pool,
            &agent_core::NewRun {
                id: run_id,
                tenant_id: "single".to_string(),
                agent_id,
                triggered_by_user_id: Some(user_id),
                recipe_id: "llm_remote_v1".to_string(),
                status: "queued".to_string(),
                input_json: json!({
                    "text": "Episode transcript text for summary.",
                    "request_llm": true,
                    "llm_prompt": "Summarize in one line",
                    "llm_prefer": "remote",
                    "llm_max_tokens": 20
                }),
                requested_capabilities: json!([
                    {"capability":"llm.infer","scope":"remote:*"}
                ]),
                granted_capabilities: json!([
                    {
                        "capability":"llm.infer",
                        "scope":"remote:*",
                        "limits":{"max_payload_bytes":32000}
                    }
                ]),
                error_json: None,
            },
        )
        .await?;

        let mut config =
            worker_test_config("worker-test-llm-model-budget-window", artifact_root.clone());
        config.llm = LlmConfig {
            mode: LlmMode::LocalFirst,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: None,
            remote: Some(LlmEndpointConfig {
                base_url: llm_url,
                model: "mock-remote-model".to_string(),
                api_key: Some("x".to_string()),
            }),
            remote_egress_enabled: true,
            remote_host_allowlist: vec!["127.0.0.1".to_string()],
            remote_token_budget_per_run: None,
            remote_token_budget_per_tenant: None,
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: Some(60),
            remote_token_budget_window_secs: 3600,
            remote_cost_per_1k_tokens_usd: 0.0,
        };

        let outcome = process_once(&test_db.app_pool, &config).await?;
        assert_eq!(outcome, WorkerCycleOutcome::ClaimedAndFailed { run_id });

        let not_seen = tokio::time::timeout(Duration::from_millis(750), llm_request_rx).await;
        if not_seen.is_ok() {
            return Err("remote LLM should not be called when model budget precheck fails".into());
        }

        let error_json: serde_json::Value = sqlx::query_scalar(
            "SELECT ar.error_json FROM action_results ar
             JOIN action_requests aq ON aq.id = ar.action_request_id
             JOIN steps s ON s.id = aq.step_id
             WHERE s.run_id = $1 AND aq.action_type = 'llm.infer'",
        )
        .bind(run_id)
        .fetch_one(&test_db.app_pool)
        .await?;
        assert!(error_json
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .contains("model token budget exceeded"));

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
        nip46_client_secret_key: None,
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
        skill_env_allowlist: Vec::new(),
        llm: LlmConfig {
            mode: LlmMode::LocalFirst,
            timeout: Duration::from_secs(2),
            max_prompt_bytes: 32_000,
            max_output_bytes: 64_000,
            local: None,
            remote: None,
            remote_egress_enabled: false,
            remote_host_allowlist: Vec::new(),
            remote_token_budget_per_run: None,
            remote_token_budget_per_tenant: None,
            remote_token_budget_per_agent: None,
            remote_token_budget_per_model: None,
            remote_token_budget_window_secs: 86_400,
            remote_cost_per_1k_tokens_usd: 0.0,
        },
        local_exec: LocalExecConfig {
            enabled: false,
            timeout: Duration::from_millis(2_000),
            max_output_bytes: 16 * 1024,
            max_memory_bytes: 256 * 1024 * 1024,
            max_processes: 32,
            read_roots: Vec::new(),
            write_roots: Vec::new(),
        },
        artifact_root,
        nostr_signer: NostrSignerConfig::default(),
        nostr_relays: Vec::new(),
        nostr_publish_timeout: Duration::from_millis(2_000),
        slack_webhook_url: None,
        slack_send_timeout: Duration::from_millis(2_000),
        slack_max_attempts: 3,
        slack_retry_backoff: Duration::from_millis(10),
        payment_nwc_enabled: false,
        payment_nwc_uri: None,
        payment_nwc_wallet_uris: BTreeMap::new(),
        payment_nwc_timeout: Duration::from_millis(2_000),
        payment_nwc_route_strategy: worker::PaymentNwcRouteStrategy::Ordered,
        payment_nwc_route_fallback_enabled: true,
        payment_nwc_mock_balance_msat: 1_000_000,
        payment_max_spend_msat_per_run: None,
        payment_approval_threshold_msat: None,
        payment_max_spend_msat_per_tenant: None,
        payment_max_spend_msat_per_agent: None,
        trigger_scheduler_enabled: true,
        trigger_tenant_max_inflight_runs: 100,
        trigger_scheduler_lease_enabled: true,
        trigger_scheduler_lease_name: "test".to_string(),
        trigger_scheduler_lease_ttl: Duration::from_secs(2),
    }
}

async fn seed_remote_llm_usage(
    pool: &PgPool,
    tenant_id: &str,
    agent_id: Uuid,
    user_id: Uuid,
    model_key: &str,
    consumed_tokens: i64,
) -> Result<(), sqlx::Error> {
    let run_id = Uuid::new_v4();
    let step_id = Uuid::new_v4();
    let action_request_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO runs (
            id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
            input_json, requested_capabilities, granted_capabilities
        )
        VALUES ($1, $2, $3, $4, 'llm_remote_v1', 'succeeded', '{}'::jsonb, '[]'::jsonb, '[]'::jsonb)
        "#,
    )
    .bind(run_id)
    .bind(tenant_id)
    .bind(agent_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO steps (id, run_id, tenant_id, agent_id, user_id, name, status, input_json)
        VALUES ($1, $2, $3, $4, $5, 'llm', 'succeeded', '{}'::jsonb)
        "#,
    )
    .bind(step_id)
    .bind(run_id)
    .bind(tenant_id)
    .bind(agent_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO action_requests (id, step_id, action_type, args_json, status)
        VALUES ($1, $2, 'llm.infer', '{}'::jsonb, 'executed')
        "#,
    )
    .bind(action_request_id)
    .bind(step_id)
    .execute(pool)
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
        VALUES ($1, $2, $3, $4, $5, 'remote', $6, $7, 0.0, now() - interval '30 minutes', 3600)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(run_id)
    .bind(action_request_id)
    .bind(tenant_id)
    .bind(agent_id)
    .bind(model_key)
    .bind(consumed_tokens)
    .execute(pool)
    .await?;

    Ok(())
}

fn destination_npub() -> String {
    Keys::new(
        "2222222222222222222222222222222222222222222222222222222222222222"
            .parse()
            .expect("fixed secret key parse"),
    )
    .public_key()
    .to_bech32()
    .expect("npub encoding")
}

async fn spawn_mock_relay(
) -> Result<(String, oneshot::Receiver<serde_json::Value>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (tx, rx) = oneshot::channel::<serde_json::Value>();

    tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(mut ws) = accept_async(stream).await else {
            return;
        };
        if let Some(Ok(Message::Text(text))) = ws.next().await {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                let event_id = value
                    .get(1)
                    .and_then(|v| v.get("id"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let _ = tx.send(value);
                let ack = json!(["OK", event_id, true, "accepted"]).to_string();
                let _ = ws.send(Message::Text(ack)).await;
            }
        }
        let _ = ws.close(None).await;
    });

    Ok((format!("ws://{}", addr), rx))
}

async fn spawn_mock_nip46_bunker_relay() -> Result<
    (String, String, String, oneshot::Receiver<serde_json::Value>),
    Box<dyn std::error::Error>,
> {
    let signer_keys = Keys::new(SecretKey::parse(
        "3333333333333333333333333333333333333333333333333333333333333333",
    )?);
    let signer_npub = signer_keys.public_key().to_bech32()?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let relay_url = format!("ws://{}", addr);
    let bunker_uri = format!(
        "bunker://{}?relay={}",
        signer_keys.public_key().to_hex(),
        relay_url
    );

    let (tx, rx) = oneshot::channel::<serde_json::Value>();
    tokio::spawn(async move {
        let mut published_event_tx = Some(tx);

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let Ok(mut ws) = accept_async(stream).await else {
                continue;
            };

            let mut sub_id = None;
            while let Some(Ok(frame)) = ws.next().await {
                let Message::Text(text) = frame else {
                    continue;
                };
                let Ok(client_msg) = ClientMessage::from_json(&text) else {
                    continue;
                };

                match client_msg {
                    ClientMessage::Req {
                        subscription_id, ..
                    } => {
                        sub_id = Some(subscription_id.into_owned());
                    }
                    ClientMessage::Event(event) => {
                        let event = event.into_owned();
                        let ack = RelayMessage::ok(event.id, true, "accepted").as_json();
                        let _ = ws.send(Message::Text(ack)).await;

                        if event.kind == Kind::NostrConnect {
                            let plaintext = match nip44::decrypt(
                                signer_keys.secret_key(),
                                &event.pubkey,
                                &event.content,
                            ) {
                                Ok(plaintext) => plaintext,
                                Err(_) => continue,
                            };
                            let message = match NostrConnectMessage::from_json(plaintext) {
                                Ok(message) => message,
                                Err(_) => continue,
                            };
                            let id = message.id().to_string();
                            let request = match message.to_request() {
                                Ok(request) => request,
                                Err(_) => continue,
                            };

                            let response = match request {
                                NostrConnectRequest::Connect { .. } => {
                                    NostrConnectResponse::with_result(Nip46ResponseResult::Ack)
                                }
                                NostrConnectRequest::SignEvent(unsigned) => {
                                    let signed = match unsigned.sign_with_keys(&signer_keys) {
                                        Ok(event) => event,
                                        Err(_) => continue,
                                    };
                                    NostrConnectResponse::with_result(
                                        Nip46ResponseResult::SignEvent(Box::new(signed)),
                                    )
                                }
                                _ => NostrConnectResponse::with_error("unsupported"),
                            };
                            let response_message = NostrConnectMessage::response(id, response);
                            let response_event = match EventBuilder::nostr_connect(
                                &signer_keys,
                                event.pubkey,
                                response_message,
                            ) {
                                Ok(builder) => match builder.sign_with_keys(&signer_keys) {
                                    Ok(event) => event,
                                    Err(_) => continue,
                                },
                                Err(_) => continue,
                            };
                            if let Some(subscription_id) = sub_id.clone() {
                                let relay_event =
                                    RelayMessage::event(subscription_id, response_event).as_json();
                                let _ = ws.send(Message::Text(relay_event)).await;
                            }
                        } else if let Some(sender) = published_event_tx.take() {
                            let _ = sender.send(json!(["EVENT", event]));
                        }
                    }
                    ClientMessage::Close(_) => break,
                    _ => {}
                }
            }

            let _ = ws.close(None).await;
            if published_event_tx.is_none() {
                break;
            }
        }
    });

    Ok((relay_url, bunker_uri, signer_npub, rx))
}

async fn spawn_mock_nwc_wallet_relay(
) -> Result<(String, oneshot::Receiver<NwcRequest>), Box<dyn std::error::Error>> {
    let wallet_keys = Keys::new(SecretKey::parse(
        "9999999999999999999999999999999999999999999999999999999999999999",
    )?);
    let app_secret =
        SecretKey::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let relay_addr = listener.local_addr()?;
    let nwc_uri = format!(
        "nostr+walletconnect://{}?secret={}&relay=ws://{}",
        wallet_keys.public_key().to_hex(),
        app_secret.to_secret_hex(),
        relay_addr
    );
    let wallet_keys_for_task = wallet_keys.clone();
    let (tx, rx) = oneshot::channel::<NwcRequest>();

    tokio::spawn(async move {
        let Ok((stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(mut ws) = accept_async(stream).await else {
            return;
        };
        let mut sub_id = None;
        let mut request_sender = Some(tx);

        while let Some(Ok(Message::Text(text))) = ws.next().await {
            let Ok(client_msg) = ClientMessage::from_json(&text) else {
                continue;
            };
            match client_msg {
                ClientMessage::Req {
                    subscription_id, ..
                } => {
                    sub_id = Some(subscription_id.into_owned());
                }
                ClientMessage::Event(event) => {
                    let event = event.into_owned();
                    let ack = RelayMessage::ok(event.id, true, "accepted").as_json();
                    let _ = ws.send(Message::Text(ack)).await;
                    if event.kind != Kind::WalletConnectRequest {
                        continue;
                    }

                    let decrypted = match nip04::decrypt(
                        wallet_keys_for_task.secret_key(),
                        &event.pubkey,
                        &event.content,
                    ) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let request = match NwcRequest::from_json(&decrypted) {
                        Ok(request) => request,
                        Err(_) => continue,
                    };
                    if let Some(sender) = request_sender.take() {
                        let _ = sender.send(request.clone());
                    }

                    let response = NwcResponse {
                        result_type: NwcMethod::PayInvoice,
                        error: None,
                        result: Some(NwcResponseResult::PayInvoice(
                            nostr::nips::nip47::PayInvoiceResponse {
                                preimage: "relay-preimage-001".to_string(),
                                fees_paid: Some(7),
                            },
                        )),
                    };
                    let encrypted = match nip04::encrypt(
                        wallet_keys_for_task.secret_key(),
                        &event.pubkey,
                        response.as_json(),
                    ) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let response_event =
                        match EventBuilder::new(Kind::WalletConnectResponse, encrypted)
                            .tag(Tag::public_key(event.pubkey))
                            .tag(Tag::event(event.id))
                            .sign_with_keys(&wallet_keys_for_task)
                        {
                            Ok(event) => event,
                            Err(_) => continue,
                        };
                    if let Some(subscription_id) = sub_id.clone() {
                        let relay_event =
                            RelayMessage::event(subscription_id, response_event).as_json();
                        let _ = ws.send(Message::Text(relay_event)).await;
                    }
                }
                ClientMessage::Close(_) => break,
                _ => {}
            }
        }
    });

    Ok((nwc_uri, rx))
}

async fn spawn_mock_llm_server(
    completion: &str,
) -> Result<(String, oneshot::Receiver<serde_json::Value>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let completion = completion.to_string();
    let (tx, rx) = oneshot::channel::<serde_json::Value>();

    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };

        let mut headers = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            let Ok(read) = stream.read(&mut byte).await else {
                return;
            };
            if read == 0 {
                return;
            }
            headers.push(byte[0]);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
            if headers.len() > 65_536 {
                return;
            }
        }

        let header_text = String::from_utf8_lossy(&headers);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                let lower = line.to_ascii_lowercase();
                lower
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .and_then(|v| v.parse::<usize>().ok())
            })
            .unwrap_or(0);
        let mut body = vec![0_u8; content_length];
        if stream.read_exact(&mut body).await.is_err() {
            return;
        }
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
            let _ = tx.send(value);
        }

        let response_body = json!({
            "choices": [
                {"message": {"content": completion}}
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 4,
                "total_tokens": 14
            }
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });

    Ok((format!("http://{}/v1", addr), rx))
}

async fn spawn_mock_slack_webhook_sequence(
    statuses: Vec<u16>,
) -> Result<(String, oneshot::Receiver<Vec<serde_json::Value>>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (tx, rx) = oneshot::channel::<Vec<serde_json::Value>>();

    tokio::spawn(async move {
        let mut payloads = Vec::new();

        for status_code in statuses {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };

            let mut headers = Vec::new();
            let mut byte = [0_u8; 1];
            loop {
                let Ok(read) = stream.read(&mut byte).await else {
                    return;
                };
                if read == 0 {
                    return;
                }
                headers.push(byte[0]);
                if headers.ends_with(b"\r\n\r\n") {
                    break;
                }
                if headers.len() > 65_536 {
                    return;
                }
            }

            let header_text = String::from_utf8_lossy(&headers);
            let content_length = header_text
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .map(str::trim)
                        .and_then(|v| v.parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0_u8; content_length];
            if stream.read_exact(&mut body).await.is_err() {
                return;
            }
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
                payloads.push(value);
            }

            let (status_line, response_body) = if status_code == 200 {
                ("200 OK", "ok".to_string())
            } else {
                ("500 Internal Server Error", "error".to_string())
            };
            let response = format!(
                "HTTP/1.1 {}\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                status_line,
                response_body.len(),
                response_body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }

        let _ = tx.send(payloads);
    });

    Ok((format!("http://{}/services/mock/webhook", addr), rx))
}

async fn spawn_mock_slack_webhook(
) -> Result<(String, oneshot::Receiver<serde_json::Value>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (tx, rx) = oneshot::channel::<serde_json::Value>();

    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };

        let mut headers = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            let Ok(read) = stream.read(&mut byte).await else {
                return;
            };
            if read == 0 {
                return;
            }
            headers.push(byte[0]);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
            if headers.len() > 65_536 {
                return;
            }
        }

        let header_text = String::from_utf8_lossy(&headers);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                let lower = line.to_ascii_lowercase();
                lower
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .and_then(|v| v.parse::<usize>().ok())
            })
            .unwrap_or(0);
        let mut body = vec![0_u8; content_length];
        if stream.read_exact(&mut body).await.is_err() {
            return;
        }
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) {
            let _ = tx.send(value);
        }

        let response_body = "ok";
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });

    Ok((format!("http://{}/services/mock/webhook", addr), rx))
}

fn temp_artifact_root(suffix: &str) -> PathBuf {
    env::temp_dir().join(format!("secureagnt_{}_{}", suffix, Uuid::new_v4()))
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
    .bind("secureagnt_worker_test")
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

async fn command_available(cmd: &str) -> bool {
    tokio::process::Command::new(cmd)
        .arg("--version")
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}
