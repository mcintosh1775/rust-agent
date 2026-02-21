#!/usr/bin/env python3
"""Profile-parity smoke test for channel-scoped llm.infer defaults."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass

import solo_lite_agent_run as runner


TERMINAL_RUN_STATUSES = {"succeeded", "failed", "cancelled", "canceled"}


@dataclass
class ProfileConfig:
    name: str
    base_url: str
    make_target_down: str
    make_target: str
    make_target_build: str


def _apply_llm_env_defaults(stack_env: dict[str, str]) -> None:
    defaults = {
        "LLM_MODE": "local_first",
        "LLM_MAX_INPUT_BYTES": "262144",
        "LLM_MAX_PROMPT_BYTES": "32000",
        "LLM_MAX_OUTPUT_BYTES": "64000",
        "LLM_LARGE_INPUT_THRESHOLD_BYTES": "12000",
        "LLM_LARGE_INPUT_POLICY": "summarize_first",
        "LLM_LARGE_INPUT_SUMMARY_TARGET_BYTES": "8000",
        "LLM_CONTEXT_RETRIEVAL_TOP_K": "6",
        "LLM_CONTEXT_RETRIEVAL_MAX_BYTES": "32000",
        "LLM_CONTEXT_RETRIEVAL_CHUNK_BYTES": "2048",
        "LLM_ADMISSION_ENABLED": "1",
        "LLM_ADMISSION_INTERACTIVE_MAX_INFLIGHT": "8",
        "LLM_ADMISSION_BATCH_MAX_INFLIGHT": "2",
        "LLM_CACHE_ENABLED": "0",
        "LLM_CACHE_TTL_SECS": "300",
        "LLM_CACHE_MAX_ENTRIES": "1024",
        "LLM_DISTRIBUTED_ENABLED": "0",
        "LLM_DISTRIBUTED_FAIL_OPEN": "1",
        "LLM_DISTRIBUTED_OWNER": "",
        "LLM_DISTRIBUTED_ADMISSION_ENABLED": "0",
        "LLM_DISTRIBUTED_ADMISSION_LEASE_MS": "30000",
        "LLM_DISTRIBUTED_CACHE_ENABLED": "0",
        "LLM_DISTRIBUTED_CACHE_NAMESPACE_MAX_ENTRIES": "4096",
        "LLM_VERIFIER_ENABLED": "0",
        "LLM_VERIFIER_MODE": "heuristic",
        "LLM_VERIFIER_MIN_SCORE_PCT": "65",
        "LLM_VERIFIER_ESCALATE_REMOTE": "1",
        "LLM_VERIFIER_MIN_RESPONSE_CHARS": "48",
        "LLM_VERIFIER_JUDGE_BASE_URL": "",
        "LLM_VERIFIER_JUDGE_MODEL": "",
        "LLM_VERIFIER_JUDGE_API_KEY": "",
        "LLM_VERIFIER_JUDGE_API_KEY_REF": "",
        "LLM_VERIFIER_JUDGE_TIMEOUT_MS": "4000",
        "LLM_VERIFIER_JUDGE_FAIL_OPEN": "1",
        "LLM_SLO_INTERACTIVE_MAX_LATENCY_MS": "",
        "LLM_SLO_BATCH_MAX_LATENCY_MS": "",
        "LLM_SLO_ALERT_THRESHOLD_PCT": "",
        "LLM_SLO_BREACH_ESCALATE_REMOTE": "0",
        "LLM_LOCAL_BASE_URL": "",
        "LLM_LOCAL_MODEL": "qwen2.5:7b-instruct",
        "LLM_LOCAL_API_KEY": "",
        "LLM_LOCAL_API_KEY_REF": "",
        "LLM_LOCAL_SMALL_BASE_URL": "",
        "LLM_LOCAL_SMALL_MODEL": "",
        "LLM_LOCAL_SMALL_API_KEY": "",
        "LLM_LOCAL_SMALL_API_KEY_REF": "",
        "LLM_LOCAL_INTERACTIVE_TIER": "workhorse",
        "LLM_LOCAL_BATCH_TIER": "workhorse",
        "LLM_CHANNEL_DEFAULTS_JSON": "",
        "LLM_REMOTE_BASE_URL": "",
        "LLM_REMOTE_MODEL": "",
        "LLM_REMOTE_API_KEY": "",
        "LLM_REMOTE_API_KEY_REF": "",
        "LLM_REMOTE_EGRESS_ENABLED": "0",
        "LLM_REMOTE_EGRESS_CLASS": "cloud_allowed",
        "LLM_REMOTE_HOST_ALLOWLIST": "",
        "LLM_REMOTE_TOKEN_BUDGET_PER_RUN": "",
        "LLM_REMOTE_TOKEN_BUDGET_PER_TENANT": "",
        "LLM_REMOTE_TOKEN_BUDGET_PER_AGENT": "",
        "LLM_REMOTE_TOKEN_BUDGET_PER_MODEL": "",
        "LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS": "86400",
        "LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT": "",
        "LLM_REMOTE_COST_PER_1K_TOKENS_USD": "0.0",
        "NOSTR_SIGNER_MODE": "local_key",
        "NOSTR_SECRET_KEY": "",
        "NOSTR_SECRET_KEY_FILE": "",
        "NOSTR_NIP46_BUNKER_URI": "",
        "NOSTR_NIP46_PUBLIC_KEY": "",
        "NOSTR_NIP46_CLIENT_SECRET_KEY": "",
        "NOSTR_RELAYS": "",
        "NOSTR_PUBLISH_TIMEOUT_MS": "4000",
        "PAYMENT_NWC_ENABLED": "0",
        "PAYMENT_NWC_URI": "",
        "PAYMENT_NWC_URI_REF": "",
        "PAYMENT_APPROVAL_THRESHOLD_MSAT": "",
        "PAYMENT_CASHU_ENABLED": "0",
        "WORKER_LOCAL_EXEC_ENABLED": "0",
        "WORKER_APPROVAL_REQUIRED_ACTION_TYPES": "",
        "WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED": "0",
        "WORKER_COMPLIANCE_SIEM_HTTP_ENABLED": "0",
        "WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN": "",
        "WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN_REF": "",
        "WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST": "",
        "WORKER_MESSAGE_SLACK_DEST_ALLOWLIST": "",
        "WORKER_AGENT_CONTEXT_ENABLED": "0",
        "WORKER_AGENT_CONTEXT_REQUIRED": "0",
        "WORKER_AGENT_CONTEXT_ROOT": "/var/lib/secureagnt/agent-context",
        "WORKER_AGENT_CONTEXT_REQUIRED_FILES": "",
        "WORKER_AGENT_CONTEXT_MAX_FILE_BYTES": "65536",
        "WORKER_AGENT_CONTEXT_MAX_TOTAL_BYTES": "262144",
        "WORKER_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR": "8",
    }
    for key, value in defaults.items():
        stack_env.setdefault(key, value)


def _apply_env_file_defaults(stack_env: dict[str, str], env_file: pathlib.Path) -> None:
    if not env_file.exists():
        return
    for raw in env_file.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        if not key:
            continue
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
            value = value[1:-1]
        stack_env.setdefault(key, value)


def _http_json(
    *,
    base_url: str,
    method: str,
    path: str,
    tenant_id: str,
    user_role: str | None,
    timeout_secs: float,
    auth_proxy_token: str | None,
    json_body: dict[str, object] | None = None,
) -> tuple[int, dict[str, object] | list[object] | None, str]:
    payload = None
    headers = {"x-tenant-id": tenant_id}
    if user_role is not None:
        headers["x-user-role"] = user_role
    token = (auth_proxy_token or "").strip()
    if token:
        headers["x-auth-proxy-token"] = token
    if json_body is not None:
        payload = json.dumps(json_body).encode("utf-8")
        headers["content-type"] = "application/json"

    req = urllib.request.Request(
        url=f"{base_url.rstrip('/')}{path}",
        method=method,
        headers=headers,
        data=payload,
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout_secs) as resp:
            body_text = resp.read().decode("utf-8")
            parsed = None
            if body_text:
                parsed = json.loads(body_text)
            return resp.status, parsed, body_text
    except urllib.error.HTTPError as err:
        body_text = err.read().decode("utf-8")
        parsed = None
        if body_text:
            try:
                parsed = json.loads(body_text)
            except json.JSONDecodeError:
                parsed = None
        return err.code, parsed, body_text
    except urllib.error.URLError as err:
        return 0, None, str(err)


def _wait_for_api(
    *,
    base_url: str,
    tenant_id: str,
    timeout_secs: float,
    auth_proxy_token: str | None,
) -> None:
    started_at = time.monotonic()
    while time.monotonic() - started_at < timeout_secs:
        status, _, _ = _http_json(
            base_url=base_url,
            method="GET",
            path="/v1/ops/summary?window_secs=3600",
            tenant_id=tenant_id,
            user_role="owner",
            timeout_secs=5.0,
            auth_proxy_token=auth_proxy_token,
        )
        if status == 200:
            return
        time.sleep(1.0)
    raise RuntimeError(f"API did not become ready at {base_url}")


def _wait_for_worker_lite_exec(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    timeout_secs: float,
    poll_interval_secs: float,
) -> None:
    started_at = time.monotonic()
    while time.monotonic() - started_at < timeout_secs:
        if runner._is_worker_lite_exec_ready(repo_root=repo_root, compose_cmd=compose_cmd):
            return
        time.sleep(poll_interval_secs)
    raise RuntimeError("worker-lite exec endpoint did not become ready")


def _sql_literal(value: str) -> str:
    return value.replace("'", "''")


def _exec_postgres_psql(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    sql: str,
) -> str:
    compose_file = repo_root / "infra" / "containers" / "compose.yml"
    base_cmd = compose_cmd + [
        "-f",
        str(compose_file),
        "--profile",
        "stack",
        "exec",
        "-T",
        "postgres",
        "psql",
        "-U",
        "postgres",
        "-d",
        "agentdb",
        "-v",
        "ON_ERROR_STOP=1",
        "-At",
        "-F",
        "\t",
        "-c",
        sql,
    ]
    try:
        completed = runner._run(base_cmd, cwd=repo_root, capture_output=True)
    except subprocess.CalledProcessError as err:
        if "-T" in base_cmd and ("unknown flag" in err.stderr or "unknown shorthand flag" in err.stderr):
            fallback = [part for part in base_cmd if part != "-T"]
            completed = runner._run(fallback, cwd=repo_root, capture_output=True)
        else:
            raise RuntimeError(
                f"postgres exec failed:\nstdout:\n{err.stdout}\nstderr:\n{err.stderr}"
            ) from err
    return completed.stdout


def _seed_agent_user_postgres_via_compose(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    tenant_id: str,
    agent_id: str,
    agent_name: str,
    user_id: str,
    user_subject: str,
    user_display_name: str,
) -> tuple[str, str]:
    tenant_sql = _sql_literal(tenant_id)
    agent_id_sql = _sql_literal(agent_id)
    agent_name_sql = _sql_literal(agent_name)
    user_id_sql = _sql_literal(user_id)
    user_subject_sql = _sql_literal(user_subject)
    user_display_name_sql = _sql_literal(user_display_name)
    sql = f"""
WITH upsert_agent AS (
  INSERT INTO agents (id, tenant_id, name, status)
  VALUES ('{agent_id_sql}'::uuid, '{tenant_sql}', '{agent_name_sql}', 'active')
  ON CONFLICT (tenant_id, name) DO UPDATE
    SET status = EXCLUDED.status
  RETURNING id
),
existing_user AS (
  SELECT id
  FROM users
  WHERE tenant_id = '{tenant_sql}'
    AND external_subject = '{user_subject_sql}'
  ORDER BY id
  LIMIT 1
),
inserted_user AS (
  INSERT INTO users (id, tenant_id, external_subject, display_name, status)
  SELECT
    '{user_id_sql}'::uuid,
    '{tenant_sql}',
    '{user_subject_sql}',
    '{user_display_name_sql}',
    'active'
  WHERE NOT EXISTS (SELECT 1 FROM existing_user)
  RETURNING id
),
resolved_user AS (
  SELECT id FROM inserted_user
  UNION ALL
  SELECT id FROM existing_user
  LIMIT 1
),
updated_user AS (
  UPDATE users
  SET display_name = '{user_display_name_sql}',
      status = 'active'
  WHERE id = (SELECT id FROM resolved_user)
  RETURNING id
)
SELECT json_build_object(
  'tenant_id', '{tenant_sql}',
  'agent_id', (SELECT id::text FROM upsert_agent),
  'user_id', (SELECT id::text FROM updated_user)
)::text;
""".strip()
    output = _exec_postgres_psql(
        repo_root=repo_root,
        compose_cmd=compose_cmd,
        sql=sql,
    ).strip()
    payload = _parse_json_loose(output, label="postgres seed")
    seeded_agent = payload.get("agent_id")
    seeded_user = payload.get("user_id")
    if not isinstance(seeded_agent, str) or not isinstance(seeded_user, str):
        raise RuntimeError("postgres seed payload missing agent_id/user_id")
    return seeded_agent, seeded_user


def _parse_json_loose(raw: str, *, label: str) -> dict[str, object]:
    text = raw.strip()
    if not text:
        raise RuntimeError(f"{label} returned empty output")
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        start = text.find("{")
        end = text.rfind("}")
        if start == -1 or end == -1 or end <= start:
            raise RuntimeError(f"{label} did not return parseable JSON:\n{text}") from None
        parsed = json.loads(text[start : end + 1])
    if not isinstance(parsed, dict):
        raise RuntimeError(f"{label} JSON payload was not an object")
    return parsed


def _create_run(
    *,
    base_url: str,
    tenant_id: str,
    auth_proxy_token: str | None,
    agent_id: str,
    user_id: str,
    input_payload: dict[str, object],
    timeout_secs: float,
) -> str:
    status, payload, raw_body = _http_json(
        base_url=base_url,
        method="POST",
        path="/v1/runs",
        tenant_id=tenant_id,
        user_role="owner",
        timeout_secs=timeout_secs,
        auth_proxy_token=auth_proxy_token,
        json_body={
            "agent_id": agent_id,
            "triggered_by_user_id": user_id,
            "recipe_id": "llm_local_v1",
            "input": input_payload,
            "requested_capabilities": [],
        },
    )
    if status != 201 or not isinstance(payload, dict):
        raise RuntimeError(f"run creation failed status={status}: {raw_body}")
    run_id = payload.get("id")
    if not isinstance(run_id, str):
        raise RuntimeError("run creation response missing id")
    return run_id


def _poll_run(
    *,
    base_url: str,
    tenant_id: str,
    auth_proxy_token: str | None,
    run_id: str,
    timeout_secs: float,
    poll_interval_secs: float,
) -> dict[str, object]:
    started_at = time.monotonic()
    while time.monotonic() - started_at < timeout_secs:
        status, payload, raw_body = _http_json(
            base_url=base_url,
            method="GET",
            path=f"/v1/runs/{run_id}",
            tenant_id=tenant_id,
            user_role=None,
            timeout_secs=10.0,
            auth_proxy_token=auth_proxy_token,
        )
        if status != 200 or not isinstance(payload, dict):
            raise RuntimeError(f"run status fetch failed status={status}: {raw_body}")
        run_status = payload.get("status")
        if isinstance(run_status, str) and run_status in TERMINAL_RUN_STATUSES:
            return payload
        time.sleep(poll_interval_secs)
    raise RuntimeError(f"timed out waiting for run {run_id} to reach terminal state")


def _fetch_audit(
    *,
    base_url: str,
    tenant_id: str,
    auth_proxy_token: str | None,
    run_id: str,
    timeout_secs: float,
) -> list[dict[str, object]]:
    status, payload, raw_body = _http_json(
        base_url=base_url,
        method="GET",
        path=f"/v1/runs/{run_id}/audit?limit=400",
        tenant_id=tenant_id,
        user_role=None,
        timeout_secs=timeout_secs,
        auth_proxy_token=auth_proxy_token,
    )
    if status != 200 or not isinstance(payload, list):
        raise RuntimeError(f"audit fetch failed status={status}: {raw_body}")
    return [event for event in payload if isinstance(event, dict)]


def _extract_llm_gateway(audit_events: list[dict[str, object]]) -> dict[str, object]:
    for event in reversed(audit_events):
        if event.get("event_type") != "action.executed":
            continue
        payload = event.get("payload_json")
        if not isinstance(payload, dict):
            continue
        if payload.get("action_type") != "llm.infer":
            continue
        result = payload.get("result")
        if not isinstance(result, dict):
            continue
        gateway = result.get("gateway")
        if isinstance(gateway, dict):
            return gateway
    raise RuntimeError("audit did not include executed llm.infer gateway payload")


def _profile_defaults(name: str) -> ProfileConfig:
    if name == "solo-lite":
        return ProfileConfig(
            name=name,
            base_url="http://localhost:18080",
            make_target_down="stack-lite-down",
            make_target="stack-lite-up",
            make_target_build="stack-lite-up-build",
        )
    if name == "stack":
        return ProfileConfig(
            name=name,
            base_url="http://localhost:8080",
            make_target_down="stack-down",
            make_target="stack-up",
            make_target_build="stack-up-build",
        )
    raise RuntimeError(f"unsupported profile: {name}")


def _run_profile_smoke(args: argparse.Namespace, profile_name: str) -> dict[str, object]:
    profile = _profile_defaults(profile_name)
    repo_root = runner._repo_root()
    compose_cmd = runner._detect_compose_cmd()
    tenant_id = args.tenant_id
    base_url = args.base_url or profile.base_url

    stack_env = dict(os.environ)
    if profile_name == "solo-lite":
        stack_env = runner._build_stack_env(enable_context=False)
    else:
        _apply_env_file_defaults(stack_env, repo_root / "infra" / "config" / "profile.enterprise.env")
    _apply_llm_env_defaults(stack_env)
    stack_env["LLM_MODE"] = args.llm_mode
    stack_env["LLM_LOCAL_BASE_URL"] = args.llm_local_base_url
    stack_env["LLM_LOCAL_MODEL"] = args.llm_local_model
    stack_env["LLM_LOCAL_SMALL_BASE_URL"] = args.llm_local_small_base_url
    stack_env["LLM_LOCAL_SMALL_MODEL"] = args.llm_local_small_model
    stack_env["LLM_VERIFIER_ENABLED"] = "0"
    if args.llm_channel_defaults_json is not None:
        stack_env["LLM_CHANNEL_DEFAULTS_JSON"] = args.llm_channel_defaults_json
    if not (args.auth_proxy_token or "").strip():
        # Smoke runs use direct owner headers; disable trusted-proxy auth unless a token is supplied.
        stack_env["API_TRUSTED_PROXY_AUTH_ENABLED"] = "0"
        stack_env.setdefault("API_TRUSTED_PROXY_SHARED_SECRET", "")
        stack_env.setdefault("API_TRUSTED_PROXY_SHARED_SECRET_REF", "")

    if args.start_stack:
        # Ensure env overrides are applied to fresh containers.
        try:
            runner._run(["make", profile.make_target_down], cwd=repo_root, env=stack_env)
        except subprocess.CalledProcessError:
            pass
        make_target = profile.make_target_build if args.build else profile.make_target
        runner._run(["make", make_target], cwd=repo_root, env=stack_env)
    _wait_for_api(
        base_url=base_url,
        tenant_id=tenant_id,
        timeout_secs=args.ready_timeout_secs,
        auth_proxy_token=args.auth_proxy_token,
    )
    if profile_name == "solo-lite":
        _wait_for_worker_lite_exec(
            repo_root=repo_root,
            compose_cmd=compose_cmd,
            timeout_secs=args.ready_timeout_secs,
            poll_interval_secs=args.poll_interval_secs,
        )

    agent_id = str(uuid.uuid4())
    user_id = str(uuid.uuid4())
    if profile_name == "solo-lite":
        seeded_agent_id, seeded_user_id = runner._seed_agent_user_sqlite_via_worker(
            repo_root=repo_root,
            compose_cmd=compose_cmd,
            tenant_id=tenant_id,
            agent_id=agent_id,
            agent_name=f"llm-channel-{profile_name}-agent",
            user_id=user_id,
            user_subject=f"llm-channel-{profile_name}-user",
            user_display_name=f"LLM Channel {profile_name} User",
            sqlite_path=args.sqlite_path,
        )
    else:
        seeded_agent_id, seeded_user_id = _seed_agent_user_postgres_via_compose(
            repo_root=repo_root,
            compose_cmd=compose_cmd,
            tenant_id=tenant_id,
            agent_id=agent_id,
            agent_name=f"llm-channel-{profile_name}-agent",
            user_id=user_id,
            user_subject=f"llm-channel-{profile_name}-user",
            user_display_name=f"LLM Channel {profile_name} User",
        )

    cases = [
        {
            "name": "event_payload_inbox",
            "input": {
                "text": "Operator inbox note",
                "request_llm": True,
                "llm_prompt": "Triage inbox message",
                "event_payload": {"channel": "#inbox"},
            },
            "expect": {
                "channel": "inbox",
                "channel_defaults_applied": True,
                "request_class": "interactive",
                "local_tier_selected": "small",
                "selected_route": "local",
            },
        },
        {
            "name": "explicit_llm_channel_precedence",
            "input": {
                "text": "General task",
                "request_llm": True,
                "llm_prompt": "Summarize for general channel",
                "llm_channel": "#general",
                "_trigger": {"channel": "monitoring"},
                "event_payload": {"channel": "inbox"},
            },
            "expect": {
                "channel": "general",
                "channel_defaults_applied": True,
                "request_class": "interactive",
                "local_tier_selected": "workhorse",
                "selected_route": "local",
            },
        },
        {
            "name": "trigger_monitoring_batch",
            "input": {
                "text": "Monitoring digest",
                "request_llm": True,
                "llm_prompt": "Evaluate queue health",
                "_trigger": {"channel": "monitoring"},
            },
            "expect": {
                "channel": "monitoring",
                "channel_defaults_applied": True,
                "request_class": "batch",
                "local_tier_selected": "small",
                "selected_route": "local",
            },
        },
    ]

    case_results: list[dict[str, object]] = []
    for case in cases:
        run_id = _create_run(
            base_url=base_url,
            tenant_id=tenant_id,
            auth_proxy_token=args.auth_proxy_token,
            agent_id=seeded_agent_id,
            user_id=seeded_user_id,
            input_payload=case["input"],
            timeout_secs=10.0,
        )
        run_payload = _poll_run(
            base_url=base_url,
            tenant_id=tenant_id,
            auth_proxy_token=args.auth_proxy_token,
            run_id=run_id,
            timeout_secs=args.run_timeout_secs,
            poll_interval_secs=args.poll_interval_secs,
        )
        run_status = run_payload.get("status")
        if run_status != "succeeded":
            error_detail = run_payload.get("error")
            if error_detail is None:
                error_detail = run_payload.get("error_json")
            raise RuntimeError(
                f"{profile_name}:{case['name']} expected succeeded run, got {run_status}; "
                f"run_id={run_id}; error={error_detail!r}"
            )

        audit_events = _fetch_audit(
            base_url=base_url,
            tenant_id=tenant_id,
            auth_proxy_token=args.auth_proxy_token,
            run_id=run_id,
            timeout_secs=10.0,
        )
        gateway = _extract_llm_gateway(audit_events)
        expected = case["expect"]
        for key, expected_value in expected.items():
            observed = gateway.get(key)
            if observed != expected_value:
                raise RuntimeError(
                    f"{profile_name}:{case['name']} expected gateway.{key}={expected_value!r}, got {observed!r}"
                )

        case_results.append(
            {
                "name": case["name"],
                "run_id": run_id,
                "gateway": {
                    "channel": gateway.get("channel"),
                    "channel_defaults_applied": gateway.get("channel_defaults_applied"),
                    "request_class": gateway.get("request_class"),
                    "local_tier_selected": gateway.get("local_tier_selected"),
                    "selected_route": gateway.get("selected_route"),
                    "reason_code": gateway.get("reason_code"),
                },
            }
        )

    return {
        "profile": profile_name,
        "base_url": base_url,
        "tenant_id": tenant_id,
        "agent_id": seeded_agent_id,
        "user_id": seeded_user_id,
        "cases": case_results,
    }


def _validate_cross_profile_parity(results: list[dict[str, object]]) -> None:
    if len(results) < 2:
        return
    baseline = results[0]
    baseline_cases = {
        entry["name"]: entry["gateway"] for entry in baseline.get("cases", []) if isinstance(entry, dict)
    }
    for candidate in results[1:]:
        candidate_cases = {
            entry["name"]: entry["gateway"]
            for entry in candidate.get("cases", [])
            if isinstance(entry, dict)
        }
        for case_name, baseline_gateway in baseline_cases.items():
            candidate_gateway = candidate_cases.get(case_name)
            if candidate_gateway is None:
                raise RuntimeError(
                    f"profile parity missing case {case_name} in {candidate.get('profile')}"
                )
            if candidate_gateway != baseline_gateway:
                raise RuntimeError(
                    "profile parity mismatch for case "
                    f"{case_name}: baseline={baseline_gateway!r} candidate={candidate_gateway!r}"
                )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--profile",
        action="append",
        choices=["solo-lite", "stack"],
        help="Profile(s) to validate. Defaults to both.",
    )
    parser.add_argument("--base-url", default=None, help="Optional override for a single profile run.")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--auth-proxy-token", default=None)
    parser.add_argument("--start-stack", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--ready-timeout-secs", type=float, default=180.0)
    parser.add_argument("--run-timeout-secs", type=float, default=120.0)
    parser.add_argument("--poll-interval-secs", type=float, default=1.0)
    parser.add_argument("--sqlite-path", default=runner.DEFAULT_SQLITE_PATH)
    parser.add_argument("--llm-mode", default="local_first")
    parser.add_argument("--llm-local-base-url", default="mock://workhorse")
    parser.add_argument("--llm-local-model", default="mock-local-workhorse")
    parser.add_argument("--llm-local-small-base-url", default="mock://small")
    parser.add_argument("--llm-local-small-model", default="mock-local-small")
    parser.add_argument(
        "--llm-channel-defaults-json",
        default="",
        help="Optional override for LLM_CHANNEL_DEFAULTS_JSON (empty keeps built-ins).",
    )
    args = parser.parse_args()

    profiles = args.profile or ["solo-lite", "stack"]
    if args.base_url and len(profiles) != 1:
        raise RuntimeError("--base-url override can only be used when exactly one --profile is set")

    results: list[dict[str, object]] = []
    for profile_name in profiles:
        profile_result = _run_profile_smoke(args, profile_name)
        results.append(profile_result)

    _validate_cross_profile_parity(results)
    print("llm channel parity smoke passed")
    print(json.dumps({"profiles": results}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
