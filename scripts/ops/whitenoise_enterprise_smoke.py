#!/usr/bin/env python3
"""One-command White Noise operator->agent->reply smoke test (enterprise stack)."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import socket
import subprocess
import time
import urllib.error
import urllib.request
import uuid

import solo_lite_agent_run as runner


TERMINAL_RUN_STATUSES = {"succeeded", "failed", "cancelled", "canceled"}


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


def _run_capture(cmd: list[str], *, cwd: pathlib.Path) -> str:
    completed = runner._run(cmd, cwd=cwd, capture_output=True)
    return completed.stdout


def _load_env_file(path: pathlib.Path) -> dict[str, str]:
    values: dict[str, str] = {}
    if not path.exists():
        return values
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        if not key:
            continue
        values[key] = value.strip()
    return values


def _sql_literal(value: str) -> str:
    return value.replace("'", "''")


def _wait_for_tcp(*, host: str, port: int, timeout_secs: float) -> None:
    started = time.monotonic()
    while time.monotonic() - started < timeout_secs:
        try:
            with socket.create_connection((host, port), timeout=1.0):
                return
        except OSError:
            time.sleep(0.2)
    raise RuntimeError(f"timed out waiting for TCP listener at {host}:{port}")


def _spawn_mock_relay(*, repo_root: pathlib.Path, bind_addr: str) -> subprocess.Popen[str]:
    return subprocess.Popen(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "worker",
            "--bin",
            "secureagnt-mock-nostr-relay",
            "--",
            "--bind",
            bind_addr,
        ],
        cwd=repo_root,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


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
INSERT INTO agents (id, tenant_id, name, status)
VALUES ('{agent_id_sql}'::uuid, '{tenant_sql}', '{agent_name_sql}', 'active')
ON CONFLICT (id) DO UPDATE
  SET tenant_id = EXCLUDED.tenant_id,
      name = EXCLUDED.name,
      status = EXCLUDED.status;

INSERT INTO users (id, tenant_id, external_subject, display_name, status)
VALUES ('{user_id_sql}'::uuid, '{tenant_sql}', '{user_subject_sql}', '{user_display_name_sql}', 'active')
ON CONFLICT (id) DO UPDATE
  SET tenant_id = EXCLUDED.tenant_id,
      external_subject = EXCLUDED.external_subject,
      display_name = EXCLUDED.display_name,
      status = EXCLUDED.status;

SELECT json_build_object(
  'tenant_id', '{tenant_sql}',
  'agent_id', '{agent_id_sql}',
  'user_id', '{user_id_sql}'
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


def _query_run_status_postgres(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    tenant_id: str,
    agent_id: str,
    trigger_event_id: str,
) -> dict[str, object] | None:
    tenant_sql = _sql_literal(tenant_id)
    agent_sql = _sql_literal(agent_id)
    event_sql = _sql_literal(trigger_event_id)
    sql = f"""
SELECT
  r.id::text,
  r.status,
  to_char(r.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  to_char(r.started_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  to_char(r.finished_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
  (
    SELECT COUNT(*)
    FROM audit_events ae
    WHERE ae.run_id = r.id
      AND ae.event_type = 'action.executed'
      AND ae.payload_json->>'action_type' = 'message.send'
  ) AS message_send_executed_count
FROM runs r
WHERE r.tenant_id = '{tenant_sql}'
  AND r.agent_id = '{agent_sql}'::uuid
  AND r.input_json #>> '{{_trigger,event_id}}' = '{event_sql}'
ORDER BY r.created_at DESC
LIMIT 1;
""".strip()
    output = _exec_postgres_psql(
        repo_root=repo_root,
        compose_cmd=compose_cmd,
        sql=sql,
    ).strip()
    if not output:
        return None
    parts = output.split("\t")
    if len(parts) < 6:
        return None
    return {
        "run_id": parts[0],
        "status": parts[1],
        "created_at": parts[2] or None,
        "started_at": parts[3] or None,
        "finished_at": parts[4] or None,
        "message_send_executed_count": int(parts[5] or 0),
    }


def _generate_nostr_keypair(repo_root: pathlib.Path) -> dict[str, str]:
    raw = _run_capture(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "worker",
            "--bin",
            "secureagnt-nostr-keygen",
            "--",
            "--json",
        ],
        cwd=repo_root,
    )
    payload = _parse_json_loose(raw, label="nostr keygen")
    npub = payload.get("npub")
    nsec = payload.get("nsec")
    if not isinstance(npub, str) or not isinstance(nsec, str):
        raise RuntimeError("nostr keygen missing npub/nsec")
    return {"npub": npub, "nsec": nsec}


def _resolve_operator_keys(args: argparse.Namespace, repo_root: pathlib.Path) -> tuple[dict[str, str], str]:
    explicit_nsec = (args.operator_nsec or "").strip()
    explicit_npub = (args.operator_npub or "").strip()
    explicit_nsec_file = (args.operator_nsec_file or "").strip()

    if explicit_nsec and explicit_nsec_file:
        raise RuntimeError("use only one of --operator-nsec or --operator-nsec-file")

    if explicit_nsec_file:
        explicit_nsec = pathlib.Path(explicit_nsec_file).read_text(encoding="utf-8").strip()

    has_explicit = bool(explicit_nsec or explicit_npub)
    if has_explicit:
        if not explicit_nsec or not explicit_npub:
            raise RuntimeError(
                "when using explicit operator identity, provide both --operator-npub and "
                "one of --operator-nsec/--operator-nsec-file"
            )
        return {"npub": explicit_npub, "nsec": explicit_nsec}, "provided"

    return _generate_nostr_keypair(repo_root), "generated"


def _extract_trigger_event_id(bridge_payload: dict[str, object]) -> str:
    results = bridge_payload.get("results")
    if not isinstance(results, list):
        raise RuntimeError("bridge output missing results")
    for entry in results:
        if not isinstance(entry, dict):
            continue
        trigger_event = entry.get("trigger_event_response")
        if not isinstance(trigger_event, dict):
            continue
        event_id = trigger_event.get("event_id")
        if isinstance(event_id, str) and event_id.strip():
            return event_id
    raise RuntimeError("bridge output missing trigger event id")


def _http_ops_ready(*, base_url: str, tenant_id: str, auth_proxy_token: str | None) -> bool:
    headers = {
        "x-tenant-id": tenant_id,
        "x-user-role": "owner",
    }
    token = (auth_proxy_token or "").strip()
    if token:
        headers["x-auth-proxy-token"] = token
    req = urllib.request.Request(
        url=f"{base_url.rstrip('/')}/v1/ops/summary?window_secs=3600",
        method="GET",
        headers=headers,
    )
    try:
        with urllib.request.urlopen(req, timeout=3.0) as resp:
            return resp.status == 200
    except urllib.error.URLError:
        return False
    except urllib.error.HTTPError as err:
        return err.code == 200


def _wait_for_api(
    *,
    base_url: str,
    tenant_id: str,
    auth_proxy_token: str | None,
    timeout_secs: float,
) -> None:
    started = time.monotonic()
    while time.monotonic() - started < timeout_secs:
        if _http_ops_ready(
            base_url=base_url,
            tenant_id=tenant_id,
            auth_proxy_token=auth_proxy_token,
        ):
            return
        time.sleep(1.0)
    raise RuntimeError(f"enterprise API did not become ready at {base_url}")


def _is_worker_exec_ready(*, repo_root: pathlib.Path, compose_cmd: list[str]) -> bool:
    compose_file = repo_root / "infra" / "containers" / "compose.yml"
    base_cmd = compose_cmd + [
        "-f",
        str(compose_file),
        "--profile",
        "stack",
        "exec",
        "-T",
        "worker",
        "true",
    ]
    try:
        runner._run(base_cmd, cwd=repo_root, capture_output=True)
        return True
    except subprocess.CalledProcessError as err:
        if "-T" in base_cmd and ("unknown flag" in err.stderr or "unknown shorthand flag" in err.stderr):
            fallback = [part for part in base_cmd if part != "-T"]
            try:
                runner._run(fallback, cwd=repo_root, capture_output=True)
                return True
            except subprocess.CalledProcessError:
                return False
        return False


def _build_stack_env() -> dict[str, str]:
    profile_path = runner._repo_root() / "infra" / "config" / "profile.enterprise.env"
    stack_env = _load_env_file(profile_path)
    stack_env.update(os.environ)
    stack_env.setdefault("NOSTR_SIGNER_MODE", "local_key")
    stack_env.setdefault("NOSTR_SECRET_KEY", "")
    stack_env.setdefault("NOSTR_SECRET_KEY_FILE", "")
    stack_env.setdefault("NOSTR_NIP46_BUNKER_URI", "")
    stack_env.setdefault("NOSTR_NIP46_PUBLIC_KEY", "")
    stack_env.setdefault("NOSTR_NIP46_CLIENT_SECRET_KEY", "")
    stack_env.setdefault("NOSTR_RELAYS", "")
    stack_env.setdefault("NOSTR_PUBLISH_TIMEOUT_MS", "4000")
    trusted_proxy_enabled = stack_env.get("API_TRUSTED_PROXY_AUTH_ENABLED", "0").strip().lower() in {
        "1",
        "true",
        "yes",
        "on",
    }
    has_proxy_secret = bool(
        (stack_env.get("API_TRUSTED_PROXY_SHARED_SECRET", "") or "").strip()
        or (stack_env.get("API_TRUSTED_PROXY_SHARED_SECRET_REF", "") or "").strip()
    )
    if trusted_proxy_enabled and not has_proxy_secret:
        stack_env["API_TRUSTED_PROXY_AUTH_ENABLED"] = "0"
    return stack_env


def _wire_worker_signer_env(
    *,
    repo_root: pathlib.Path,
    key_root: pathlib.Path,
    key_info: dict[str, str],
    signer_mode: str,
    nostr_relays: str,
    nostr_publish_timeout_ms: int,
    nip46_bunker_uri: str | None,
    nip46_public_key: str | None,
    nip46_client_secret_key: str | None,
) -> tuple[dict[str, str], str | None]:
    stack_env = _build_stack_env()
    stack_env["NOSTR_SIGNER_MODE"] = signer_mode
    stack_env["NOSTR_RELAYS"] = nostr_relays
    stack_env["NOSTR_PUBLISH_TIMEOUT_MS"] = str(max(1, nostr_publish_timeout_ms))

    configured_secret_path = None
    if signer_mode == "local_key":
        nsec_file = key_info.get("nsec_file")
        if not isinstance(nsec_file, str):
            raise RuntimeError("agent key info missing nsec_file for local signer wiring")
        nsec_value = pathlib.Path(nsec_file).read_text(encoding="utf-8").strip()
        if not nsec_value:
            raise RuntimeError(f"agent key file is empty: {nsec_file}")
        container_path = runner._container_path_for_agent_key(
            repo_root=repo_root,
            key_root=key_root,
            host_key_path=pathlib.Path(nsec_file),
        )
        if container_path is None:
            raise RuntimeError(
                "cannot wire local signer from custom --agent-key-root; use default "
                f"{runner.DEFAULT_AGENT_KEY_ROOT} so compose mount maps into worker"
            )
        configured_secret_path = container_path
        stack_env["NOSTR_SECRET_KEY_FILE"] = container_path
        stack_env["NOSTR_SECRET_KEY"] = nsec_value
        stack_env["NOSTR_NIP46_BUNKER_URI"] = ""
        stack_env["NOSTR_NIP46_PUBLIC_KEY"] = ""
        stack_env["NOSTR_NIP46_CLIENT_SECRET_KEY"] = ""
    elif signer_mode == "nip46_signer":
        bunker_uri = (nip46_bunker_uri or "").strip()
        if not bunker_uri:
            raise RuntimeError(
                "--nostr-nip46-bunker-uri is required when --nostr-signer-mode nip46_signer"
            )
        stack_env["NOSTR_SECRET_KEY_FILE"] = ""
        stack_env["NOSTR_SECRET_KEY"] = ""
        stack_env["NOSTR_NIP46_BUNKER_URI"] = bunker_uri
        stack_env["NOSTR_NIP46_PUBLIC_KEY"] = (nip46_public_key or "").strip()
        stack_env["NOSTR_NIP46_CLIENT_SECRET_KEY"] = (nip46_client_secret_key or "").strip()
    else:
        raise RuntimeError(f"unsupported --nostr-signer-mode: {signer_mode}")

    return stack_env, configured_secret_path


def main() -> int:
    repo_root = runner._repo_root()
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:8080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--agent-id", default=str(uuid.uuid4()))
    parser.add_argument("--agent-name", default=None)
    parser.add_argument("--user-id", default=str(uuid.uuid4()))
    parser.add_argument("--user-subject", default=None)
    parser.add_argument("--user-display-name", default="White Noise Enterprise Smoke User")
    parser.add_argument("--recipe-id", default="operator_chat_v1")
    parser.add_argument(
        "--message-text",
        default="enterprise operator smoke: queue depth stable and no critical alerts fired.",
    )
    parser.add_argument("--nostr-relay", default="wss://relay.damus.io")
    parser.add_argument(
        "--spawn-mock-relay",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Start a local mock Nostr relay (CI-safe, no public relay dependency).",
    )
    parser.add_argument(
        "--mock-relay-bind",
        default="127.0.0.1:19191",
        help="Local host:port bind for --spawn-mock-relay.",
    )
    parser.add_argument(
        "--operator-npub",
        default=None,
        help="Optional explicit operator npub (recommended for real identity testing).",
    )
    parser.add_argument(
        "--operator-nsec",
        default=None,
        help="Optional explicit operator nsec/hex secret (use with --operator-npub).",
    )
    parser.add_argument(
        "--operator-nsec-file",
        default=None,
        help="Optional file containing operator nsec/hex secret (use with --operator-npub).",
    )
    parser.add_argument("--start-stack", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--agent-key-root", default=runner.DEFAULT_AGENT_KEY_ROOT)
    parser.add_argument("--regen-agent-keys", action="store_true")
    parser.add_argument(
        "--nostr-signer-mode",
        default="local_key",
        choices=["local_key", "nip46_signer"],
    )
    parser.add_argument("--nostr-nip46-bunker-uri", default=None)
    parser.add_argument("--nostr-nip46-public-key", default=None)
    parser.add_argument("--nostr-nip46-client-secret-key", default=None)
    parser.add_argument(
        "--auth-proxy-token",
        default=None,
        help="Optional x-auth-proxy-token for trusted-proxy enterprise mode.",
    )
    parser.add_argument("--ready-timeout-secs", type=float, default=120.0)
    parser.add_argument("--bridge-listen-timeout-secs", type=float, default=90.0)
    parser.add_argument("--bridge-subscribe-delay-secs", type=float, default=2.0)
    parser.add_argument("--run-timeout-secs", type=float, default=120.0)
    parser.add_argument("--poll-interval-secs", type=float, default=1.0)
    args = parser.parse_args()
    if not (args.agent_name or "").strip():
        args.agent_name = f"whitenoise-enterprise-roundtrip-agent-{args.agent_id[:8]}"
    if not (args.user_subject or "").strip():
        args.user_subject = f"whitenoise-enterprise-roundtrip-user-{args.user_id[:8]}"

    compose_cmd = runner._detect_compose_cmd()
    relay_url = args.nostr_relay
    mock_relay_proc: subprocess.Popen[str] | None = None
    if args.spawn_mock_relay:
        host, sep, raw_port = args.mock_relay_bind.rpartition(":")
        if not sep or not host:
            raise RuntimeError(f"invalid --mock-relay-bind `{args.mock_relay_bind}` (expected host:port)")
        port = int(raw_port)
        relay_url = f"ws://{args.mock_relay_bind}"
        mock_relay_proc = _spawn_mock_relay(repo_root=repo_root, bind_addr=args.mock_relay_bind)
        try:
            _wait_for_tcp(host=host, port=port, timeout_secs=20.0)
        except Exception:
            stderr = ""
            if mock_relay_proc.stderr is not None:
                stderr = mock_relay_proc.stderr.read()
            raise RuntimeError(
                f"mock relay failed to start on {args.mock_relay_bind}; stderr:\n{stderr}"
            ) from None

    try:
        key_info = runner._ensure_agent_nostr_keypair(
            repo_root=repo_root,
            key_root=(repo_root / args.agent_key_root),
            tenant_id=args.tenant_id,
            agent_id=args.agent_id,
            regenerate=args.regen_agent_keys,
        )
        operator_keys, operator_key_source = _resolve_operator_keys(args, repo_root)

        stack_env, worker_signer_secret_path = _wire_worker_signer_env(
            repo_root=repo_root,
            key_root=(repo_root / args.agent_key_root),
            key_info=key_info,
            signer_mode=args.nostr_signer_mode,
            nostr_relays=relay_url,
            nostr_publish_timeout_ms=4000,
            nip46_bunker_uri=args.nostr_nip46_bunker_uri,
            nip46_public_key=args.nostr_nip46_public_key,
            nip46_client_secret_key=args.nostr_nip46_client_secret_key,
        )
        stack_env["WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST"] = operator_keys["npub"]

        if args.start_stack:
            api_ready = _http_ops_ready(
                base_url=args.base_url,
                tenant_id=args.tenant_id,
                auth_proxy_token=args.auth_proxy_token,
            )
            worker_ready = _is_worker_exec_ready(repo_root=repo_root, compose_cmd=compose_cmd)
            if api_ready and worker_ready:
                print("note: enterprise API/worker reachable; reconciling signer env", flush=True)
            make_target = "stack-up-build" if args.build else "stack-up"
            runner._run(["make", make_target], cwd=repo_root, env=stack_env)

        _wait_for_api(
            base_url=args.base_url,
            tenant_id=args.tenant_id,
            auth_proxy_token=args.auth_proxy_token,
            timeout_secs=args.ready_timeout_secs,
        )
        if not _is_worker_exec_ready(repo_root=repo_root, compose_cmd=compose_cmd):
            raise RuntimeError(
                "enterprise worker is not exec-ready after stack startup; "
                "check `make stack-logs` for worker startup errors"
            )

        seeded_agent_id, seeded_user_id = _seed_agent_user_postgres_via_compose(
            repo_root=repo_root,
            compose_cmd=compose_cmd,
            tenant_id=args.tenant_id,
            agent_id=args.agent_id,
            agent_name=args.agent_name,
            user_id=args.user_id,
            user_subject=args.user_subject,
            user_display_name=args.user_display_name,
        )

        if seeded_agent_id != args.agent_id:
            key_info = runner._ensure_agent_nostr_keypair(
                repo_root=repo_root,
                key_root=(repo_root / args.agent_key_root),
                tenant_id=args.tenant_id,
                agent_id=seeded_agent_id,
                regenerate=args.regen_agent_keys,
            )
            stack_env, worker_signer_secret_path = _wire_worker_signer_env(
                repo_root=repo_root,
                key_root=(repo_root / args.agent_key_root),
                key_info=key_info,
                signer_mode=args.nostr_signer_mode,
                nostr_relays=relay_url,
                nostr_publish_timeout_ms=4000,
                nip46_bunker_uri=args.nostr_nip46_bunker_uri,
                nip46_public_key=args.nostr_nip46_public_key,
                nip46_client_secret_key=args.nostr_nip46_client_secret_key,
            )
            stack_env["WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST"] = operator_keys["npub"]
            if args.start_stack:
                runner._run(["make", "stack-up"], cwd=repo_root, env=stack_env)

        bridge_cmd = [
            "cargo",
            "run",
            "-q",
            "-p",
            "worker",
            "--bin",
            "secureagnt-whitenoise-bridge",
            "--",
            "--base-url",
            args.base_url,
            "--tenant-id",
            args.tenant_id,
            "--relay",
            relay_url,
            "--agent-id",
            seeded_agent_id,
            "--agent-pubkey",
            str(key_info["npub"]),
            "--operator-pubkey",
            operator_keys["npub"],
            "--triggered-by-user-id",
            seeded_user_id,
            "--recipe-id",
            args.recipe_id,
            "--max-events",
            "1",
            "--listen-timeout-secs",
            str(max(1, int(args.bridge_listen_timeout_secs))),
        ]
        if (args.auth_proxy_token or "").strip():
            bridge_cmd.extend(["--auth-proxy-token", str(args.auth_proxy_token).strip()])

        send_cmd = [
            "cargo",
            "run",
            "-q",
            "-p",
            "worker",
            "--bin",
            "secureagnt-whitenoise-send",
            "--",
            "--relay",
            relay_url,
            "--to",
            str(key_info["npub"]),
            "--text",
            args.message_text,
            "--secret-key",
            operator_keys["nsec"],
        ]

        bridge_proc = subprocess.Popen(
            bridge_cmd,
            cwd=repo_root,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        try:
            time.sleep(max(0.0, args.bridge_subscribe_delay_secs))
            send_raw = _run_capture(send_cmd, cwd=repo_root)
            send_payload = _parse_json_loose(send_raw, label="whitenoise send")

            try:
                bridge_stdout, bridge_stderr = bridge_proc.communicate(
                    timeout=max(1.0, args.bridge_listen_timeout_secs + 15.0)
                )
            except subprocess.TimeoutExpired:
                bridge_proc.kill()
                bridge_stdout, bridge_stderr = bridge_proc.communicate()
                raise RuntimeError(
                    "bridge listener timed out waiting for event\n"
                    f"stdout:\n{bridge_stdout}\n\nstderr:\n{bridge_stderr}"
                )
            if bridge_proc.returncode != 0:
                raise RuntimeError(
                    "bridge listener failed\n"
                    f"stdout:\n{bridge_stdout}\n\nstderr:\n{bridge_stderr}"
                )
            bridge_payload = _parse_json_loose(bridge_stdout, label="whitenoise bridge")
        finally:
            if bridge_proc.poll() is None:
                bridge_proc.kill()

        accepted = bridge_payload.get("accepted_events")
        if not isinstance(accepted, int) or accepted < 1:
            raise RuntimeError(
                f"bridge did not accept an event: {json.dumps(bridge_payload, indent=2)}"
            )

        trigger_event_id = _extract_trigger_event_id(bridge_payload)

        started = time.monotonic()
        run_info = None
        while time.monotonic() - started < args.run_timeout_secs:
            run_info = _query_run_status_postgres(
                repo_root=repo_root,
                compose_cmd=compose_cmd,
                tenant_id=args.tenant_id,
                agent_id=seeded_agent_id,
                trigger_event_id=trigger_event_id,
            )
            if isinstance(run_info, dict):
                status = str(run_info.get("status", ""))
                sent_count = int(run_info.get("message_send_executed_count", 0))
                if status in TERMINAL_RUN_STATUSES and sent_count > 0:
                    break
                if status in TERMINAL_RUN_STATUSES and sent_count == 0:
                    raise RuntimeError(
                        "run reached terminal state without message.send execution: "
                        f"{json.dumps(run_info, indent=2)}"
                    )
            time.sleep(args.poll_interval_secs)
        else:
            raise RuntimeError(
                "timed out waiting for trigger-created run with message.send execution; "
                f"last_run_info={json.dumps(run_info, indent=2) if run_info else 'null'}"
            )

        summary = {
            "base_url": args.base_url,
            "tenant_id": args.tenant_id,
            "agent_id": seeded_agent_id,
            "agent_npub": key_info.get("npub"),
            "agent_nsec_file": key_info.get("nsec_file"),
            "worker_nostr_signer_mode": args.nostr_signer_mode,
            "worker_nostr_secret_key_file": worker_signer_secret_path,
            "worker_message_whitenoise_dest_allowlist": stack_env.get(
                "WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST", ""
            ),
            "operator_npub": operator_keys["npub"],
            "operator_key_source": operator_key_source,
            "relay": relay_url,
            "trigger_id": bridge_payload.get("trigger_id"),
            "trigger_event_id": trigger_event_id,
            "send_publish_result": send_payload.get("publish_result"),
            "run": run_info,
        }
        print("whitenoise enterprise smoke complete")
        print(json.dumps(summary, indent=2, sort_keys=True))
        print(f"export TENANT_ID={args.tenant_id}")
        print(f"export AGENT_ID={seeded_agent_id}")
        print(f"export AGENT_NPUB={key_info.get('npub')}")
        if isinstance(run_info, dict) and isinstance(run_info.get("run_id"), str):
            print(f"export RUN_ID={run_info['run_id']}")
        return 0
    finally:
        if mock_relay_proc is not None:
            if mock_relay_proc.poll() is None:
                mock_relay_proc.terminate()
                try:
                    mock_relay_proc.wait(timeout=5.0)
                except subprocess.TimeoutExpired:
                    mock_relay_proc.kill()


if __name__ == "__main__":
    raise SystemExit(main())
