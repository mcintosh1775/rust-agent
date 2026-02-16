use agent_core::{
    append_audit_event as append_raw_audit_event, claim_next_queued_run, create_action_request,
    create_action_result, create_step, mark_run_failed, mark_run_succeeded, mark_step_failed,
    mark_step_succeeded, persist_artifact_metadata, redact_json, redact_text, renew_run_lease,
    requeue_expired_runs, update_action_request_status, ActionRequest as PolicyActionRequest,
    CapabilityGrant as PolicyCapabilityGrant, CapabilityKind as PolicyCapabilityKind, GrantSet,
    NewActionRequest, NewActionResult, NewArtifact, NewAuditEvent, NewStep, PolicyDecision,
};
use anyhow::{anyhow, Context, Result};
use core as agent_core;
use nostr::{PublicKey, SecretKey};
use serde_json::{json, Value};
use skillrunner::{
    CapabilityGrant as SkillCapabilityGrant, InvokeContext, InvokeRequest, RunnerConfig,
    SkillRunner,
};
use sqlx::PgPool;
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

pub mod nip46_signer;
pub mod nostr_transport;
pub mod signer;

use nip46_signer::sign_event_with_bunker;
use nostr_transport::{build_text_note_unsigned, publish_signed_event, publish_text_note};
use signer::{NostrSignerConfig, NostrSignerMode};

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub worker_id: String,
    pub lease_for: Duration,
    pub requeue_limit: i64,
    pub poll_interval: Duration,
    pub skill_command: String,
    pub skill_args: Vec<String>,
    pub skill_timeout: Duration,
    pub skill_max_output_bytes: usize,
    pub skill_env_allowlist: Vec<String>,
    pub artifact_root: PathBuf,
    pub nostr_signer: NostrSignerConfig,
    pub nostr_relays: Vec<String>,
    pub nostr_publish_timeout: Duration,
}

impl WorkerConfig {
    pub fn from_env() -> Result<Self> {
        let skill_command =
            env::var("WORKER_SKILL_COMMAND").unwrap_or_else(|_| "python3".to_string());
        let default_skill_script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../skills/python/summarize_transcript/main.py")
            .to_string_lossy()
            .to_string();
        let skill_script = env::var("WORKER_SKILL_SCRIPT").unwrap_or(default_skill_script);
        let mut skill_args = vec![skill_script];
        if let Ok(extra) = env::var("WORKER_SKILL_ARGS") {
            skill_args.extend(extra.split_whitespace().map(ToString::to_string));
        }

        Ok(Self {
            worker_id: env::var("WORKER_ID")
                .unwrap_or_else(|_| format!("worker-{}", Uuid::new_v4())),
            lease_for: Duration::from_secs(read_env_u64("WORKER_LEASE_SECS", 30)?),
            requeue_limit: read_env_i64("WORKER_REQUEUE_LIMIT", 100)?,
            poll_interval: Duration::from_millis(read_env_u64("WORKER_POLL_MS", 750)?),
            skill_command,
            skill_args,
            skill_timeout: Duration::from_millis(read_env_u64("WORKER_SKILL_TIMEOUT_MS", 5000)?),
            skill_max_output_bytes: read_env_u64("WORKER_SKILL_MAX_OUTPUT_BYTES", 64 * 1024)?
                as usize,
            skill_env_allowlist: read_env_csv("WORKER_SKILL_ENV_ALLOWLIST"),
            artifact_root: PathBuf::from(
                env::var("WORKER_ARTIFACT_ROOT").unwrap_or_else(|_| "artifacts".to_string()),
            ),
            nostr_signer: NostrSignerConfig::from_env()?,
            nostr_relays: read_env_csv("NOSTR_RELAYS"),
            nostr_publish_timeout: Duration::from_millis(read_env_u64(
                "NOSTR_PUBLISH_TIMEOUT_MS",
                4000,
            )?),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerCycleOutcome {
    ClaimedAndSucceeded { run_id: Uuid },
    ClaimedAndFailed { run_id: Uuid },
    Idle { requeued_expired_runs: u64 },
}

pub async fn process_once(pool: &PgPool, config: &WorkerConfig) -> Result<WorkerCycleOutcome> {
    let requeued_expired_runs = requeue_expired_runs(pool, config.requeue_limit).await?;

    let Some(claimed_run) =
        claim_next_queued_run(pool, &config.worker_id, config.lease_for).await?
    else {
        return Ok(WorkerCycleOutcome::Idle {
            requeued_expired_runs,
        });
    };

    append_audit_event(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: claimed_run.id,
            step_id: None,
            tenant_id: claimed_run.tenant_id.clone(),
            agent_id: Some(claimed_run.agent_id),
            user_id: claimed_run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "run.claimed".to_string(),
            payload_json: json!({
                "attempts": claimed_run.attempts,
                "lease_owner": claimed_run.lease_owner,
                "lease_expires_at": claimed_run.lease_expires_at,
            }),
        },
    )
    .await?;

    let renewed =
        renew_run_lease(pool, claimed_run.id, &config.worker_id, config.lease_for).await?;
    if !renewed {
        append_audit_event(
            pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id: claimed_run.id,
                step_id: None,
                tenant_id: claimed_run.tenant_id.clone(),
                agent_id: Some(claimed_run.agent_id),
                user_id: claimed_run.triggered_by_user_id,
                actor: format!("worker:{}", config.worker_id),
                event_type: "run.lease_renew_failed".to_string(),
                payload_json: json!({}),
            },
        )
        .await?;

        let _ = mark_run_failed(
            pool,
            claimed_run.id,
            &config.worker_id,
            redact_json(&json!({
                "code": "LEASE_RENEW_FAILED",
                "message": "worker failed to renew run lease after claim"
            })),
        )
        .await?;

        return Ok(WorkerCycleOutcome::Idle {
            requeued_expired_runs,
        });
    }

    append_audit_event(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: claimed_run.id,
            step_id: None,
            tenant_id: claimed_run.tenant_id.clone(),
            agent_id: Some(claimed_run.agent_id),
            user_id: claimed_run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "run.processing_started".to_string(),
            payload_json: json!({
                "recipe_id": claimed_run.recipe_id,
                "attempts": claimed_run.attempts,
            }),
        },
    )
    .await?;

    let run_result = process_claimed_run(pool, config, &claimed_run).await;
    match run_result {
        Ok(()) => {
            let completed = mark_run_succeeded(pool, claimed_run.id, &config.worker_id).await?;
            if completed {
                append_audit_event(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: claimed_run.id,
                        step_id: None,
                        tenant_id: claimed_run.tenant_id,
                        agent_id: Some(claimed_run.agent_id),
                        user_id: claimed_run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "run.completed".to_string(),
                        payload_json: json!({
                            "status": "succeeded",
                            "attempts": claimed_run.attempts,
                        }),
                    },
                )
                .await?;

                Ok(WorkerCycleOutcome::ClaimedAndSucceeded {
                    run_id: claimed_run.id,
                })
            } else {
                mark_run_failed(
                    pool,
                    claimed_run.id,
                    &config.worker_id,
                    redact_json(&json!({
                        "code": "RUN_FINALIZE_FAILED",
                        "message": "worker could not mark run as succeeded"
                    })),
                )
                .await?;

                Ok(WorkerCycleOutcome::ClaimedAndFailed {
                    run_id: claimed_run.id,
                })
            }
        }
        Err(error) => {
            let error_message = redact_text(&format!("{error:#}"));
            mark_run_failed(
                pool,
                claimed_run.id,
                &config.worker_id,
                redact_json(&json!({
                    "code": "RUN_EXECUTION_FAILED",
                    "message": error_message,
                })),
            )
            .await?;

            append_audit_event(
                pool,
                &NewAuditEvent {
                    id: Uuid::new_v4(),
                    run_id: claimed_run.id,
                    step_id: None,
                    tenant_id: claimed_run.tenant_id,
                    agent_id: Some(claimed_run.agent_id),
                    user_id: claimed_run.triggered_by_user_id,
                    actor: format!("worker:{}", config.worker_id),
                    event_type: "run.failed".to_string(),
                    payload_json: json!({"error": error_message}),
                },
            )
            .await?;

            Ok(WorkerCycleOutcome::ClaimedAndFailed {
                run_id: claimed_run.id,
            })
        }
    }
}

async fn process_claimed_run(
    pool: &PgPool,
    config: &WorkerConfig,
    run: &agent_core::RunLeaseRecord,
) -> Result<()> {
    let step = create_step(
        pool,
        &NewStep {
            id: Uuid::new_v4(),
            run_id: run.id,
            tenant_id: run.tenant_id.clone(),
            agent_id: run.agent_id,
            user_id: run.triggered_by_user_id,
            name: "summarize_transcript".to_string(),
            status: "running".to_string(),
            input_json: run.input_json.clone(),
            error_json: None,
        },
    )
    .await?;

    append_audit_event(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: run.id,
            step_id: Some(step.id),
            tenant_id: run.tenant_id.clone(),
            agent_id: Some(run.agent_id),
            user_id: run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "step.started".to_string(),
            payload_json: json!({"step_name": step.name}),
        },
    )
    .await?;

    let grants = parse_grant_set(&run.granted_capabilities);
    let invoke_result =
        match invoke_skill(config, run, step.id, run.input_json.clone(), &grants).await {
            Ok(result) => result,
            Err(error) => {
                let error_message = redact_text(&format!("{error:#}"));
                mark_step_failed(
                    pool,
                    step.id,
                    redact_json(&json!({"code": "SKILL_INVOKE_FAILED", "message": error_message})),
                )
                .await?;
                append_audit_event(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "step.failed".to_string(),
                        payload_json: json!({"error": error_message}),
                    },
                )
                .await?;
                return Err(error);
            }
        };

    append_audit_event(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: run.id,
            step_id: Some(step.id),
            tenant_id: run.tenant_id.clone(),
            agent_id: Some(run.agent_id),
            user_id: run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "skill.invoked".to_string(),
            payload_json: json!({
                "action_request_count": invoke_result.action_requests.len(),
            }),
        },
    )
    .await?;

    for skill_action in invoke_result.action_requests {
        let action_request_id = Uuid::new_v4();
        create_action_request(
            pool,
            &NewActionRequest {
                id: action_request_id,
                step_id: step.id,
                action_type: skill_action.action_type.clone(),
                args_json: redact_json(&skill_action.args),
                justification: Some(redact_text(&skill_action.justification)),
                status: "requested".to_string(),
                decision_reason: None,
            },
        )
        .await?;

        append_audit_event(
            pool,
            &NewAuditEvent {
                id: Uuid::new_v4(),
                run_id: run.id,
                step_id: Some(step.id),
                tenant_id: run.tenant_id.clone(),
                agent_id: Some(run.agent_id),
                user_id: run.triggered_by_user_id,
                actor: format!("worker:{}", config.worker_id),
                event_type: "action.requested".to_string(),
                payload_json: json!({
                    "action_id": skill_action.action_id,
                    "action_type": skill_action.action_type,
                }),
            },
        )
        .await?;

        let policy_request = to_policy_request(&skill_action)?;
        match grants.is_action_allowed(&policy_request) {
            PolicyDecision::Allow => {
                update_action_request_status(pool, action_request_id, "allowed", None).await?;
                append_audit_event(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "action.allowed".to_string(),
                        payload_json: json!({"action_type": policy_request.action_type}),
                    },
                )
                .await?;

                let result_json = match execute_action(pool, run, &skill_action, config).await {
                    Ok(result_json) => result_json,
                    Err(error) => {
                        let error_message = redact_text(&format!("{error:#}"));
                        update_action_request_status(
                            pool,
                            action_request_id,
                            "failed",
                            Some("execution_failed"),
                        )
                        .await?;
                        create_action_result(
                            pool,
                            &NewActionResult {
                                id: Uuid::new_v4(),
                                action_request_id,
                                status: "failed".to_string(),
                                result_json: None,
                                error_json: Some(redact_json(&json!({
                                    "code": "ACTION_EXECUTION_FAILED",
                                    "message": error_message,
                                }))),
                            },
                        )
                        .await?;
                        append_audit_event(
                            pool,
                            &NewAuditEvent {
                                id: Uuid::new_v4(),
                                run_id: run.id,
                                step_id: Some(step.id),
                                tenant_id: run.tenant_id.clone(),
                                agent_id: Some(run.agent_id),
                                user_id: run.triggered_by_user_id,
                                actor: format!("worker:{}", config.worker_id),
                                event_type: "action.failed".to_string(),
                                payload_json: json!({
                                    "action_type": policy_request.action_type,
                                    "error": error_message,
                                }),
                            },
                        )
                        .await?;
                        let _ = mark_step_failed(
                            pool,
                            step.id,
                            redact_json(&json!({
                                "code": "ACTION_EXECUTION_FAILED",
                                "message": error_message,
                            })),
                        )
                        .await?;
                        return Err(anyhow!("action execution failed: {}", error_message));
                    }
                };

                update_action_request_status(pool, action_request_id, "executed", None).await?;
                create_action_result(
                    pool,
                    &NewActionResult {
                        id: Uuid::new_v4(),
                        action_request_id,
                        status: "executed".to_string(),
                        result_json: Some(redact_json(&result_json)),
                        error_json: None,
                    },
                )
                .await?;

                append_audit_event(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "action.executed".to_string(),
                        payload_json: json!({
                            "action_type": policy_request.action_type,
                            "result": result_json,
                        }),
                    },
                )
                .await?;
            }
            PolicyDecision::Deny(reason) => {
                let reason_str = reason.as_str();
                update_action_request_status(pool, action_request_id, "denied", Some(reason_str))
                    .await?;
                create_action_result(
                    pool,
                    &NewActionResult {
                        id: Uuid::new_v4(),
                        action_request_id,
                        status: "denied".to_string(),
                        result_json: None,
                        error_json: Some(redact_json(&json!({
                            "code": "POLICY_DENIED",
                            "reason": reason_str,
                        }))),
                    },
                )
                .await?;

                append_audit_event(
                    pool,
                    &NewAuditEvent {
                        id: Uuid::new_v4(),
                        run_id: run.id,
                        step_id: Some(step.id),
                        tenant_id: run.tenant_id.clone(),
                        agent_id: Some(run.agent_id),
                        user_id: run.triggered_by_user_id,
                        actor: format!("worker:{}", config.worker_id),
                        event_type: "action.denied".to_string(),
                        payload_json: json!({
                            "action_type": policy_request.action_type,
                            "reason": reason_str,
                        }),
                    },
                )
                .await?;

                let _ = mark_step_failed(
                    pool,
                    step.id,
                    redact_json(&json!({
                        "code": "ACTION_DENIED",
                        "reason": reason_str,
                    })),
                )
                .await?;

                return Err(anyhow!("action denied by policy: {}", reason_str));
            }
        }
    }

    mark_step_succeeded(pool, step.id, invoke_result.output.clone()).await?;
    append_audit_event(
        pool,
        &NewAuditEvent {
            id: Uuid::new_v4(),
            run_id: run.id,
            step_id: Some(step.id),
            tenant_id: run.tenant_id.clone(),
            agent_id: Some(run.agent_id),
            user_id: run.triggered_by_user_id,
            actor: format!("worker:{}", config.worker_id),
            event_type: "step.completed".to_string(),
            payload_json: json!({}),
        },
    )
    .await?;

    Ok(())
}

async fn invoke_skill(
    config: &WorkerConfig,
    run: &agent_core::RunLeaseRecord,
    step_id: Uuid,
    input: Value,
    grants: &GrantSet,
) -> Result<skillrunner::InvokeResult> {
    let runner = SkillRunner::new(RunnerConfig {
        command: config.skill_command.clone(),
        args: config.skill_args.clone(),
        timeout: config.skill_timeout,
        max_output_bytes: config.skill_max_output_bytes,
        env_allowlist: config.skill_env_allowlist.clone(),
    });

    let granted_capabilities = grants
        .grants
        .iter()
        .map(|grant| SkillCapabilityGrant {
            capability: capability_kind_to_action_type(&grant.kind).to_string(),
            scope: grant.scope.clone(),
        })
        .collect();

    let request = InvokeRequest {
        id: Uuid::new_v4().to_string(),
        context: InvokeContext {
            tenant_id: run.tenant_id.clone(),
            run_id: run.id.to_string(),
            step_id: step_id.to_string(),
            time_budget_ms: config.skill_timeout.as_millis().clamp(1, u64::MAX as u128) as u64,
            granted_capabilities,
        },
        input,
    };

    let result = runner.invoke(request).await?;
    Ok(result.invoke_result)
}

async fn execute_action(
    pool: &PgPool,
    run: &agent_core::RunLeaseRecord,
    action: &skillrunner::ActionRequest,
    config: &WorkerConfig,
) -> Result<Value> {
    match action.action_type.as_str() {
        "object.write" => {
            execute_object_write_action(pool, run.id, &action.args, &config.artifact_root).await
        }
        "message.send" => execute_message_send_action(pool, run.id, &action.args, config).await,
        other => Err(anyhow!("unsupported action type: {}", other)),
    }
}

async fn execute_object_write_action(
    pool: &PgPool,
    run_id: Uuid,
    args: &Value,
    artifact_root: &Path,
) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("object.write args.path is required"))?;
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("object.write args.content is required"))?;

    let safe_rel_path = sanitize_relative_path(path)?;
    let full_path = artifact_root.join(&safe_rel_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }
    fs::write(&full_path, content)
        .with_context(|| format!("failed writing artifact {}", full_path.display()))?;

    let artifact = persist_artifact_metadata(
        pool,
        &NewArtifact {
            id: Uuid::new_v4(),
            run_id,
            path: safe_rel_path.to_string_lossy().to_string(),
            content_type: "text/markdown".to_string(),
            size_bytes: content.len() as i64,
            checksum: None,
            storage_ref: full_path.to_string_lossy().to_string(),
        },
    )
    .await?;

    Ok(json!({
        "artifact_id": artifact.id,
        "path": artifact.path,
        "size_bytes": artifact.size_bytes,
        "storage_ref": artifact.storage_ref,
    }))
}

async fn execute_message_send_action(
    pool: &PgPool,
    run_id: Uuid,
    args: &Value,
    config: &WorkerConfig,
) -> Result<Value> {
    let destination = args
        .get("destination")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("message.send args.destination is required"))?;
    let content = args
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| args.get("content").and_then(Value::as_str))
        .ok_or_else(|| anyhow!("message.send args.text (or args.content) is required"))?;

    let parsed_destination = ParsedMessageDestination::parse(destination)?;
    let signer_identity = match parsed_destination.provider {
        MessageProvider::WhiteNoise => {
            Some(config.nostr_signer.resolve_identity()?.ok_or_else(|| {
                anyhow!("message.send to White Noise requires a configured Nostr signer identity")
            })?)
        }
        MessageProvider::Slack => None,
    };
    let (publish_result, publish_error, publish_context) = match parsed_destination.provider {
        MessageProvider::WhiteNoise => {
            if config.nostr_relays.is_empty() {
                (None, None, None)
            } else {
                attempt_whitenoise_publish(
                    config,
                    signer_identity
                        .as_ref()
                        .expect("whitenoise path always has signer identity"),
                    parsed_destination.target,
                    content,
                )
                .await
            }
        }
        MessageProvider::Slack => (None, None, None),
    };

    let outbox_message = json!({
        "provider": parsed_destination.provider.as_str(),
        "destination": destination,
        "target": parsed_destination.target,
        "text": content,
        "nostr_signer_mode": signer_identity.as_ref().map(|identity| identity.mode.as_str()),
        "nostr_public_key": signer_identity.as_ref().map(|identity| identity.public_key.as_str()),
        "publish_result": publish_result,
        "publish_error": publish_error,
        "publish_context": publish_context,
    });
    let outbox_bytes = serde_json::to_vec_pretty(&outbox_message)
        .with_context(|| "failed serializing message.send outbox payload")?;

    let relative_path = PathBuf::from("messages")
        .join(parsed_destination.provider.as_str())
        .join(format!("{}.json", Uuid::new_v4()));
    let full_path = config.artifact_root.join(&relative_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create message outbox dir {}", parent.display()))?;
    }
    fs::write(&full_path, &outbox_bytes)
        .with_context(|| format!("failed writing message outbox {}", full_path.display()))?;

    let artifact = persist_artifact_metadata(
        pool,
        &NewArtifact {
            id: Uuid::new_v4(),
            run_id,
            path: relative_path.to_string_lossy().to_string(),
            content_type: "application/json".to_string(),
            size_bytes: outbox_bytes.len() as i64,
            checksum: None,
            storage_ref: full_path.to_string_lossy().to_string(),
        },
    )
    .await?;

    Ok(json!({
        "provider": parsed_destination.provider.as_str(),
        "destination": destination,
        "delivery_state": if publish_result.is_some() { "published_nostr" } else { "queued_local_outbox" },
        "artifact_id": artifact.id,
        "path": artifact.path,
        "size_bytes": artifact.size_bytes,
        "storage_ref": artifact.storage_ref,
        "nostr_public_key": signer_identity.as_ref().map(|identity| identity.public_key.as_str()),
        "published_event_id": publish_result.as_ref().map(|result| result.event_id.as_str()),
        "accepted_relays": publish_result.as_ref().map(|result| result.accepted_relays),
        "publish_error": publish_error,
        "publish_context": publish_context,
    }))
}

async fn attempt_whitenoise_publish(
    config: &WorkerConfig,
    signer_identity: &signer::NostrSignerIdentity,
    recipient: &str,
    content: &str,
) -> (
    Option<nostr_transport::NostrPublishResult>,
    Option<String>,
    Option<Value>,
) {
    let recipient_pubkey = match PublicKey::parse(recipient)
        .with_context(|| "message.send destination target must be npub/hex for whitenoise")
    {
        Ok(pubkey) => pubkey,
        Err(error) => return (None, Some(format!("{error:#}")), None),
    };

    match config.nostr_signer.mode {
        NostrSignerMode::LocalKey => match resolve_local_secret_key_for_publish(config) {
            Ok(local_secret_key) => match publish_text_note(
                &local_secret_key,
                recipient,
                content,
                &config.nostr_relays,
                config.nostr_publish_timeout,
            )
            .await
            {
                Ok(result) => (Some(result), None, None),
                Err(error) => (None, Some(format!("{error:#}")), None),
            },
            Err(error) => (None, Some(format!("{error:#}")), None),
        },
        NostrSignerMode::Nip46Signer => {
            let signer_pubkey = match PublicKey::parse(signer_identity.public_key.as_str()) {
                Ok(pubkey) => pubkey,
                Err(error) => return (None, Some(format!("{error:#}")), None),
            };
            let unsigned = match build_text_note_unsigned(signer_pubkey, recipient_pubkey, content)
            {
                Ok(event) => event,
                Err(error) => return (None, Some(format!("{error:#}")), None),
            };

            let Some(bunker_uri) = config.nostr_signer.nip46_bunker_uri.as_deref() else {
                return (
                    None,
                    Some("NOSTR_NIP46_BUNKER_URI is required for NIP-46 publish".to_string()),
                    None,
                );
            };
            let signed_outcome = match sign_event_with_bunker(
                &unsigned,
                bunker_uri,
                config.nostr_signer.nip46_client_secret_key.as_deref(),
                config.nostr_publish_timeout,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(error) => return (None, Some(format!("{error:#}")), None),
            };

            match publish_signed_event(
                &signed_outcome.signed_event,
                &config.nostr_relays,
                config.nostr_publish_timeout,
            )
            .await
            {
                Ok(result) => (
                    Some(result),
                    None,
                    Some(json!({
                        "nip46_signer_relay": signed_outcome.signer_relay,
                        "nip46_client_public_key": signed_outcome.app_public_key,
                    })),
                ),
                Err(error) => (None, Some(format!("{error:#}")), None),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageProvider {
    WhiteNoise,
    Slack,
}

impl MessageProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::WhiteNoise => "whitenoise",
            Self::Slack => "slack", // Placeholder connector path; transport to be wired separately.
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedMessageDestination<'a> {
    provider: MessageProvider,
    target: &'a str,
}

impl<'a> ParsedMessageDestination<'a> {
    fn parse(raw: &'a str) -> Result<Self> {
        let (provider_raw, target_raw) = raw
            .split_once(':')
            .ok_or_else(|| anyhow!("message.send destination must be provider-scoped"))?;
        let provider = match provider_raw.trim().to_ascii_lowercase().as_str() {
            "whitenoise" => MessageProvider::WhiteNoise,
            "slack" => MessageProvider::Slack,
            other => {
                return Err(anyhow!(
                    "message.send provider `{}` is unsupported (expected whitenoise or slack)",
                    other
                ));
            }
        };
        let target = target_raw.trim();
        if target.is_empty() {
            return Err(anyhow!(
                "message.send destination target must not be empty: {}",
                raw
            ));
        }
        Ok(Self { provider, target })
    }
}

async fn append_audit_event(pool: &PgPool, new_event: &NewAuditEvent) -> Result<()> {
    let mut event = new_event.clone();
    event.payload_json = redact_json(&event.payload_json);
    append_raw_audit_event(pool, &event).await?;
    Ok(())
}

fn to_policy_request(action: &skillrunner::ActionRequest) -> Result<PolicyActionRequest> {
    let payload_bytes = serde_json::to_vec(&action.args)
        .with_context(|| "failed serializing action args for payload sizing")?
        .len() as u64;

    let scope = match action.action_type.as_str() {
        "object.write" => action
            .args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("object.write args.path is required"))?
            .to_string(),
        "message.send" => action
            .args
            .get("destination")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("message.send args.destination is required"))?
            .to_string(),
        _ => String::new(),
    };

    Ok(PolicyActionRequest::new(
        action.action_type.clone(),
        scope,
        payload_bytes,
    ))
}

fn parse_grant_set(raw: &Value) -> GrantSet {
    let Some(items) = raw.as_array() else {
        return GrantSet::default();
    };

    let grants = items
        .iter()
        .filter_map(parse_capability_grant)
        .collect::<Vec<_>>();

    GrantSet::new(grants)
}

fn parse_capability_grant(value: &Value) -> Option<PolicyCapabilityGrant> {
    let capability = value.get("capability")?.as_str()?;
    let scope = value.get("scope")?.as_str()?.to_string();

    let kind = parse_capability_kind(capability)?;
    let mut grant = PolicyCapabilityGrant::new(kind, scope);

    if let Some(max_payload_bytes) = value
        .get("limits")
        .and_then(|limits| limits.get("max_payload_bytes"))
        .and_then(Value::as_u64)
    {
        grant = grant.with_max_payload_bytes(max_payload_bytes);
    }

    Some(grant)
}

fn parse_capability_kind(value: &str) -> Option<PolicyCapabilityKind> {
    match value {
        "object.read" | "object_read" => Some(PolicyCapabilityKind::ObjectRead),
        "object.write" | "object_write" => Some(PolicyCapabilityKind::ObjectWrite),
        "message.send" | "message_send" => Some(PolicyCapabilityKind::MessageSend),
        "db.query" | "db_query" => Some(PolicyCapabilityKind::DbQuery),
        "http.request" | "http_request" => Some(PolicyCapabilityKind::HttpRequest),
        _ => None,
    }
}

fn capability_kind_to_action_type(kind: &PolicyCapabilityKind) -> &'static str {
    match kind {
        PolicyCapabilityKind::ObjectRead => "object.read",
        PolicyCapabilityKind::ObjectWrite => "object.write",
        PolicyCapabilityKind::MessageSend => "message.send",
        PolicyCapabilityKind::DbQuery => "db.query",
        PolicyCapabilityKind::HttpRequest => "http.request",
    }
}

fn sanitize_relative_path(path: &str) -> Result<PathBuf> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(anyhow!("absolute paths are not allowed: {}", path));
    }

    let mut cleaned = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => cleaned.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("unsafe path component in {}", path));
            }
        }
    }

    if cleaned.as_os_str().is_empty() {
        return Err(anyhow!("empty artifact path is not allowed"));
    }

    Ok(cleaned)
}

fn read_env_u64(key: &str, default: u64) -> Result<u64> {
    match env::var(key) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("invalid integer for {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn read_env_i64(key: &str, default: i64) -> Result<i64> {
    match env::var(key) {
        Ok(value) => value
            .parse::<i64>()
            .with_context(|| format!("invalid integer for {key}: {value}")),
        Err(_) => Ok(default),
    }
}

fn read_env_csv(key: &str) -> Vec<String> {
    let Ok(raw) = env::var(key) else {
        return Vec::new();
    };
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn resolve_local_secret_key_for_publish(config: &WorkerConfig) -> Result<SecretKey> {
    if let Some(secret) = config
        .nostr_signer
        .local_secret_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return SecretKey::parse(secret)
            .with_context(|| "failed to parse local Nostr secret key for publish");
    }

    if let Some(path) = &config.nostr_signer.local_secret_key_file {
        let secret = fs::read_to_string(path).with_context(|| {
            format!(
                "failed to read local Nostr secret key file for publish: {}",
                path.display()
            )
        })?;
        let secret = secret.trim();
        if secret.is_empty() {
            return Err(anyhow!(
                "local Nostr secret key file is empty: {}",
                path.display()
            ));
        }
        return SecretKey::parse(secret)
            .with_context(|| "failed to parse local Nostr secret key from file for publish");
    }

    Err(anyhow!(
        "Nostr relay publish requires local key material (NOSTR_SECRET_KEY or NOSTR_SECRET_KEY_FILE)"
    ))
}
