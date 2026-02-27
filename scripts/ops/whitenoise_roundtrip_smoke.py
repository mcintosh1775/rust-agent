#!/usr/bin/env python3
"""One-command White Noise operator->agent->reply smoke test."""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
import time
import uuid

import solo_lite_agent_run as runner


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


def _exec_worker_python(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    script: str,
    args: list[str],
) -> str:
    compose_file = repo_root / "infra" / "containers" / "compose.yml"
    base_cmd = compose_cmd + [
        "-f",
        str(compose_file),
        "--profile",
        "solo-lite",
        "exec",
        "-T",
        "worker-lite",
        "python3",
        "-c",
        script,
        *args,
    ]
    try:
        completed = runner._run(base_cmd, cwd=repo_root, capture_output=True)
    except subprocess.CalledProcessError as err:
        if "-T" in base_cmd and ("unknown flag" in err.stderr or "unknown shorthand flag" in err.stderr):
            fallback = [part for part in base_cmd if part != "-T"]
            completed = runner._run(fallback, cwd=repo_root, capture_output=True)
        else:
            raise RuntimeError(
                f"worker-lite exec failed:\nstdout:\n{err.stdout}\nstderr:\n{err.stderr}"
            ) from err
    return completed.stdout


def _query_run_status_via_worker_sqlite(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    sqlite_path: str,
    tenant_id: str,
    agent_id: str,
    trigger_event_id: str,
) -> dict[str, object] | None:
    query_script = r"""
import json
import sqlite3
import sys

db_path, tenant_id, agent_id, trigger_event_id = sys.argv[1:]
conn = sqlite3.connect(db_path)
row = conn.execute(
    '''
    SELECT
      r.id,
      r.status,
      r.created_at,
      r.started_at,
      r.finished_at,
      (
        SELECT COUNT(*)
        FROM audit_events ae
        WHERE ae.run_id = r.id
          AND ae.event_type = 'action.executed'
          AND json_extract(ae.payload_json, '$.action_type') = 'message.send'
      ) AS message_send_executed_count
    FROM runs r
    WHERE r.tenant_id = ?
      AND r.agent_id = ?
      AND json_extract(r.input_json, '$._trigger.event_id') = ?
    ORDER BY r.created_at DESC
    LIMIT 1
    ''',
    (tenant_id, agent_id, trigger_event_id),
).fetchone()
if row is None:
    print("null")
else:
    print(json.dumps({
        "run_id": row[0],
        "status": row[1],
        "created_at": row[2],
        "started_at": row[3],
        "finished_at": row[4],
        "message_send_executed_count": int(row[5] or 0),
    }))
conn.close()
"""
    output = _exec_worker_python(
        repo_root=repo_root,
        compose_cmd=compose_cmd,
        script=query_script,
        args=[sqlite_path, tenant_id, agent_id, trigger_event_id],
    ).strip()
    if not output:
        return None
    if output == "null":
        return None
    return _parse_json_loose(output, label="sqlite run status query")


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


def main() -> int:
    repo_root = runner._repo_root()
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:18080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--agent-id", default=str(uuid.uuid4()))
    parser.add_argument("--agent-name", default="whitenoise-roundtrip-agent")
    parser.add_argument("--user-id", default=str(uuid.uuid4()))
    parser.add_argument("--user-subject", default="whitenoise-roundtrip-user")
    parser.add_argument("--user-display-name", default="White Noise Roundtrip User")
    parser.add_argument("--recipe-id", default="operator_chat_v1")
    parser.add_argument(
        "--message-text",
        default="operator smoke: queue depth stable and no critical alerts fired.",
    )
    parser.add_argument("--nostr-relay", default="wss://relay.damus.io")
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
    parser.add_argument("--enable-context", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--init-context", action=argparse.BooleanOptionalAction, default=False)
    parser.add_argument("--context-root", default="agent_context")
    parser.add_argument("--force-context", action="store_true")
    parser.add_argument("--sqlite-path", default=runner.DEFAULT_SQLITE_PATH)
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
    parser.add_argument("--ready-timeout-secs", type=float, default=120.0)
    parser.add_argument("--bridge-listen-timeout-secs", type=float, default=90.0)
    parser.add_argument("--bridge-subscribe-delay-secs", type=float, default=2.0)
    parser.add_argument("--run-timeout-secs", type=float, default=90.0)
    parser.add_argument("--poll-interval-secs", type=float, default=1.0)
    args = parser.parse_args()

    compose_cmd = runner._detect_compose_cmd()
    stack_env = runner._build_stack_env(enable_context=args.enable_context)
    stack_env["WORKER_TRIGGER_SCHEDULER_ENABLED"] = "1"

    if args.start_stack:
        api_ready = runner._is_api_ready(args.base_url, args.tenant_id)
        worker_ready = runner._is_worker_lite_exec_ready(repo_root=repo_root, compose_cmd=compose_cmd)
        if api_ready and worker_ready:
            print("note: solo-lite API/worker already reachable; skipping stack start", file=sys.stderr)
        else:
            if api_ready and not worker_ready:
                print(
                    "note: solo-lite API is reachable but worker-lite is not; reconciling stack",
                    file=sys.stderr,
                )
            make_target = "stack-lite-up-build" if args.build else "stack-lite-up"
            runner._run(["make", make_target], cwd=repo_root, env=stack_env)
    elif args.enable_context:
        print(
            "note: --no-start-stack set; context loading/signer wiring depend on current worker-lite env",
            file=sys.stderr,
        )

    runner._wait_for_api(args.base_url, args.tenant_id, args.ready_timeout_secs)

    seeded_agent_id, seeded_user_id = runner._seed_agent_user_sqlite_via_worker(
        repo_root=repo_root,
        compose_cmd=compose_cmd,
        tenant_id=args.tenant_id,
        agent_id=args.agent_id,
        agent_name=args.agent_name,
        user_id=args.user_id,
        user_subject=args.user_subject,
        user_display_name=args.user_display_name,
        sqlite_path=args.sqlite_path,
    )

    key_info = runner._ensure_agent_nostr_keypair(
        repo_root=repo_root,
        key_root=(repo_root / args.agent_key_root),
        tenant_id=args.tenant_id,
        agent_id=seeded_agent_id,
        regenerate=args.regen_agent_keys,
    )

    if args.start_stack:
        stack_env, worker_signer_secret_path = runner._wire_worker_nostr_signer(
            repo_root=repo_root,
            base_stack_env=stack_env,
            key_root=(repo_root / args.agent_key_root),
            key_info=key_info,
            signer_mode=args.nostr_signer_mode,
            nostr_relays=args.nostr_relay,
            nostr_publish_timeout_ms=4000,
            nip46_bunker_uri=args.nostr_nip46_bunker_uri,
            nip46_public_key=args.nostr_nip46_public_key,
            nip46_client_secret_key=args.nostr_nip46_client_secret_key,
        )
    else:
        worker_signer_secret_path = None

    if args.init_context:
        runner._init_agent_context(
            repo_root=repo_root,
            context_root=(repo_root / args.context_root),
            tenant_id=args.tenant_id,
            agent_id=seeded_agent_id,
            agent_name=args.agent_name,
            nostr_pubkey=str(key_info.get("npub", "")),
            force=args.force_context,
        )

    operator_keys, operator_key_source = _resolve_operator_keys(args, repo_root)

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
        args.nostr_relay,
        "--agent-id",
        seeded_agent_id,
        "--agent-pubkey",
        str(key_info["npub"]),
        "--operator-pubkey",
        operator_keys["npub"],
        "--recipe-id",
        args.recipe_id,
        "--max-events",
        "1",
        "--listen-timeout-secs",
        str(max(1, int(args.bridge_listen_timeout_secs))),
    ]

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
        args.nostr_relay,
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
        raise RuntimeError(f"bridge did not accept an event: {json.dumps(bridge_payload, indent=2)}")

    trigger_event_id = _extract_trigger_event_id(bridge_payload)

    started = time.monotonic()
    run_info = None
    while time.monotonic() - started < args.run_timeout_secs:
        run_info = _query_run_status_via_worker_sqlite(
            repo_root=repo_root,
            compose_cmd=compose_cmd,
            sqlite_path=args.sqlite_path,
            tenant_id=args.tenant_id,
            agent_id=seeded_agent_id,
            trigger_event_id=trigger_event_id,
        )
        if isinstance(run_info, dict):
            status = str(run_info.get("status", ""))
            sent_count = int(run_info.get("message_send_executed_count", 0))
            if status in runner.TERMINAL_RUN_STATUSES and sent_count > 0:
                break
            if status in runner.TERMINAL_RUN_STATUSES and sent_count == 0:
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
        "operator_npub": operator_keys["npub"],
        "operator_key_source": operator_key_source,
        "relay": args.nostr_relay,
        "trigger_id": bridge_payload.get("trigger_id"),
        "trigger_event_id": trigger_event_id,
        "send_publish_result": send_payload.get("publish_result"),
        "run": run_info,
    }
    print("whitenoise roundtrip smoke complete")
    print(json.dumps(summary, indent=2, sort_keys=True))
    print(f"export TENANT_ID={args.tenant_id}")
    print(f"export AGENT_ID={seeded_agent_id}")
    print(f"export AGENT_NPUB={key_info.get('npub')}")
    if isinstance(run_info, dict) and isinstance(run_info.get("run_id"), str):
        print(f"export RUN_ID={run_info['run_id']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
