use serde_json::json;
use skillrunner::{InvokeContext, InvokeRequest, RunnerConfig, SkillRunner, SkillRunnerError};
use std::{env, fs, path::PathBuf, time::Duration};

#[tokio::test]
async fn invoke_successful_under_timeout() -> Result<(), Box<dyn std::error::Error>> {
    if !python3_available().await {
        eprintln!("skipping skillrunner integration test: python3 not available");
        return Ok(());
    }

    let runner = SkillRunner::new(runner_config(Duration::from_secs(2), 64 * 1024));
    let request = invoke_request(
        "success",
        json!({"text":"hello from transcript","request_write":true}),
    );

    let result = runner.invoke(request).await?;
    let markdown = result
        .invoke_result
        .output
        .get("markdown")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    assert!(markdown.starts_with("# Summary"));
    assert_eq!(result.invoke_result.action_requests.len(), 1);
    assert_eq!(
        result.invoke_result.action_requests[0].action_type,
        "object.write"
    );
    Ok(())
}

#[tokio::test]
async fn invoke_timeout_kills_skill() -> Result<(), Box<dyn std::error::Error>> {
    if !python3_available().await {
        eprintln!("skipping skillrunner integration test: python3 not available");
        return Ok(());
    }

    let runner = SkillRunner::new(runner_config(Duration::from_millis(150), 64 * 1024));
    let request = invoke_request("timeout", json!({"mode":"timeout","sleep_s":2.0}));

    let error = runner
        .invoke(request)
        .await
        .expect_err("timeout should fail");
    assert!(matches!(error, SkillRunnerError::Timeout));
    Ok(())
}

#[tokio::test]
async fn invoke_crash_returns_non_zero_exit() -> Result<(), Box<dyn std::error::Error>> {
    if !python3_available().await {
        eprintln!("skipping skillrunner integration test: python3 not available");
        return Ok(());
    }

    let runner = SkillRunner::new(runner_config(Duration::from_secs(2), 64 * 1024));
    let request = invoke_request("crash", json!({"mode":"crash"}));

    let error = runner.invoke(request).await.expect_err("crash should fail");
    assert!(matches!(
        error,
        SkillRunnerError::SkillExitedNonZero(Some(17))
    ));
    Ok(())
}

#[tokio::test]
async fn invoke_oversized_output_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    if !python3_available().await {
        eprintln!("skipping skillrunner integration test: python3 not available");
        return Ok(());
    }

    let runner = SkillRunner::new(runner_config(Duration::from_secs(2), 512));
    let request = invoke_request("oversize", json!({"mode":"oversize","bytes":10000}));

    let error = runner
        .invoke(request)
        .await
        .expect_err("oversized output should fail");
    assert!(matches!(error, SkillRunnerError::OutputTooLarge { .. }));
    Ok(())
}

#[tokio::test]
async fn invoke_scrubs_env_by_default_and_supports_allowlist(
) -> Result<(), Box<dyn std::error::Error>> {
    if !python3_available().await {
        eprintln!("skipping skillrunner integration test: python3 not available");
        return Ok(());
    }

    let env_key = "SECUREAGNT_TEST_SECRET";
    let prior = env::var(env_key).ok();
    env::set_var(env_key, "do-not-leak");

    let script_path = temp_env_probe_script()?;
    let mut config = RunnerConfig::new("python3");
    config.args = vec![script_path.to_string_lossy().to_string()];
    config.timeout = Duration::from_secs(2);
    config.max_output_bytes = 8 * 1024;

    let runner = SkillRunner::new(config.clone());
    let result = runner
        .invoke(invoke_request("env-scrub", json!({})))
        .await?;
    assert_eq!(
        result
            .invoke_result
            .output
            .get("seen_secret")
            .and_then(|v| v.as_str()),
        Some("")
    );
    assert_eq!(
        result
            .invoke_result
            .output
            .get("sandboxed")
            .and_then(|v| v.as_str()),
        Some("1")
    );
    assert_eq!(
        result
            .invoke_result
            .output
            .get("sandboxed_legacy")
            .and_then(|v| v.as_str()),
        Some("1")
    );

    config.env_allowlist = vec![env_key.to_string()];
    let allowlisted_runner = SkillRunner::new(config);
    let allowlisted = allowlisted_runner
        .invoke(invoke_request("env-allow", json!({})))
        .await?;
    assert_eq!(
        allowlisted
            .invoke_result
            .output
            .get("seen_secret")
            .and_then(|v| v.as_str()),
        Some("do-not-leak")
    );

    let _ = fs::remove_file(script_path);
    match prior {
        Some(value) => env::set_var(env_key, value),
        None => env::remove_var(env_key),
    }
    Ok(())
}

#[tokio::test]
async fn invoke_can_disable_legacy_aegis_marker() -> Result<(), Box<dyn std::error::Error>> {
    if !python3_available().await {
        eprintln!("skipping skillrunner integration test: python3 not available");
        return Ok(());
    }

    let script_path = temp_env_probe_script()?;
    let mut config = RunnerConfig::new("python3");
    config.args = vec![script_path.to_string_lossy().to_string()];
    config.timeout = Duration::from_secs(2);
    config.max_output_bytes = 8 * 1024;
    config.emit_legacy_aegis_skill_sandbox_marker = false;

    let runner = SkillRunner::new(config);
    let result = runner
        .invoke(invoke_request("env-legacy-disabled", json!({})))
        .await?;
    assert_eq!(
        result
            .invoke_result
            .output
            .get("sandboxed")
            .and_then(|v| v.as_str()),
        Some("1")
    );
    assert_eq!(
        result
            .invoke_result
            .output
            .get("sandboxed_legacy")
            .and_then(|v| v.as_str()),
        Some("")
    );

    let _ = fs::remove_file(script_path);
    Ok(())
}

fn runner_config(timeout: Duration, max_output_bytes: usize) -> RunnerConfig {
    let mut config = RunnerConfig::new("python3");
    config.args = vec![skill_script_path().to_string_lossy().to_string()];
    config.timeout = timeout;
    config.max_output_bytes = max_output_bytes;
    config
}

fn invoke_request(id_suffix: &str, input: serde_json::Value) -> InvokeRequest {
    InvokeRequest {
        id: format!("req-{id_suffix}"),
        context: InvokeContext {
            tenant_id: "single".to_string(),
            run_id: "run-1".to_string(),
            step_id: "step-1".to_string(),
            time_budget_ms: 5_000,
            granted_capabilities: vec![],
        },
        input,
    }
}

fn skill_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../skills/python/summarize_transcript/main.py")
}

async fn python3_available() -> bool {
    tokio::process::Command::new("python3")
        .arg("--version")
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn temp_env_probe_script() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let script_path = env::temp_dir().join(format!(
        "secureagnt_env_probe_{}_{}.py",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));

    let script = r#"#!/usr/bin/env python3
import json
import os
import sys

for line in sys.stdin:
    message = json.loads(line)
    if message.get("type") != "invoke":
        continue
    print(json.dumps({
        "type": "invoke_result",
        "id": message["id"],
        "output": {
            "seen_secret": os.environ.get("SECUREAGNT_TEST_SECRET", ""),
            "sandboxed": os.environ.get("SECUREAGNT_SKILL_SANDBOXED", ""),
            "sandboxed_legacy": os.environ.get("AEGIS_SKILL_SANDBOXED", ""),
        },
        "action_requests": []
    }), flush=True)
"#;
    fs::write(&script_path, script)?;
    Ok(script_path)
}
