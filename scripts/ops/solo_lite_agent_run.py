#!/usr/bin/env python3
"""Start and drive a solo-lite agent run end-to-end."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from collections import Counter


TERMINAL_RUN_STATUSES = {"succeeded", "failed", "cancelled"}
DEFAULT_SQLITE_PATH = "/var/lib/secureagnt/solo-lite/secureagnt.sqlite3"


def _repo_root() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parents[2]


def _detect_compose_cmd() -> list[str]:
    if shutil.which("podman") is not None:
        return ["podman", "compose"]
    if shutil.which("podman-compose") is not None:
        return ["podman-compose"]
    if shutil.which("docker") is not None:
        return ["docker", "compose"]
    raise RuntimeError("no compose runtime found (podman/podman-compose/docker)")


def _run(
    cmd: list[str],
    *,
    cwd: pathlib.Path,
    env: dict[str, str] | None = None,
    capture_output: bool = False,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        check=True,
        text=True,
        capture_output=capture_output,
    )


def _http_json(
    *,
    base_url: str,
    method: str,
    path: str,
    tenant_id: str,
    user_role: str | None,
    timeout_secs: float,
    json_body: dict[str, object] | None = None,
) -> tuple[int, dict[str, object] | list[object] | None, str]:
    payload = None
    headers = {"x-tenant-id": tenant_id}
    if user_role is not None:
        headers["x-user-role"] = user_role
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


def _wait_for_api(base_url: str, tenant_id: str, timeout_secs: float) -> None:
    started_at = time.monotonic()
    while time.monotonic() - started_at < timeout_secs:
        status, _, _ = _http_json(
            base_url=base_url,
            method="GET",
            path="/v1/ops/summary?window_secs=3600",
            tenant_id=tenant_id,
            user_role="owner",
            timeout_secs=5.0,
        )
        if status == 200:
            return
        time.sleep(1.0)
    raise RuntimeError(f"solo-lite API did not become ready at {base_url}")


def _is_api_ready(base_url: str, tenant_id: str) -> bool:
    status, _, _ = _http_json(
        base_url=base_url,
        method="GET",
        path="/v1/ops/summary?window_secs=3600",
        tenant_id=tenant_id,
        user_role="owner",
        timeout_secs=2.0,
    )
    return status == 200


def _seed_agent_user_sqlite_via_worker(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    tenant_id: str,
    agent_id: str,
    agent_name: str,
    user_id: str,
    user_subject: str,
    user_display_name: str,
    sqlite_path: str,
) -> tuple[str, str]:
    seed_script = r"""
import json
import sqlite3
import sys

db_path, tenant_id, agent_id, agent_name, user_id, user_subject, user_display_name = sys.argv[1:]
conn = sqlite3.connect(db_path)
agent_row = conn.execute(
    '''
    INSERT INTO agents (id, tenant_id, name, status)
    VALUES (?, ?, ?, 'active')
    ON CONFLICT(tenant_id, name) DO UPDATE
      SET status = excluded.status
    RETURNING id
    ''',
    (agent_id, tenant_id, agent_name),
).fetchone()
user_row = conn.execute(
    '''
    INSERT INTO users (id, tenant_id, external_subject, display_name, status)
    VALUES (?, ?, ?, ?, 'active')
    ON CONFLICT(tenant_id, external_subject) DO UPDATE
      SET display_name = excluded.display_name,
          status = excluded.status
    RETURNING id
    ''',
    (user_id, tenant_id, user_subject, user_display_name),
).fetchone()
conn.commit()
conn.close()
print(json.dumps({
    "tenant_id": tenant_id,
    "agent_id": agent_row[0],
    "user_id": user_row[0],
    "db_path": db_path,
}))
"""
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
        seed_script,
        sqlite_path,
        tenant_id,
        agent_id,
        agent_name,
        user_id,
        user_subject,
        user_display_name,
    ]
    try:
        completed = _run(base_cmd, cwd=repo_root, capture_output=True)
    except subprocess.CalledProcessError as err:
        # Some compose variants do not support -T.
        if "-T" in base_cmd and ("unknown flag" in err.stderr or "unknown shorthand flag" in err.stderr):
            fallback = [part for part in base_cmd if part != "-T"]
            completed = _run(fallback, cwd=repo_root, capture_output=True)
        else:
            raise RuntimeError(
                f"failed seeding sqlite via worker-lite exec:\nstdout:\n{err.stdout}\nstderr:\n{err.stderr}"
            ) from err
    output_text = completed.stdout.strip()
    if output_text:
        print(output_text)
    payload = None
    for raw_line in reversed(output_text.splitlines()):
        line = raw_line.strip()
        if not line.startswith("{"):
            continue
        payload = json.loads(line)
        break
    if not isinstance(payload, dict):
        raise RuntimeError("seed step did not return JSON payload from worker-lite exec")
    agent_id_value = payload.get("agent_id")
    user_id_value = payload.get("user_id")
    if not isinstance(agent_id_value, str) or not isinstance(user_id_value, str):
        raise RuntimeError("seed payload missing agent_id/user_id")
    return agent_id_value, user_id_value


def _init_agent_context(
    *,
    repo_root: pathlib.Path,
    context_root: pathlib.Path,
    tenant_id: str,
    agent_id: str,
    agent_name: str,
    force: bool,
) -> None:
    cmd = [
        "bash",
        "scripts/ops/init_agent_context.sh",
        "--root",
        str(context_root),
        "--tenant",
        tenant_id,
        "--agent-id",
        agent_id,
        "--agent-name",
        agent_name,
    ]
    if force:
        cmd.append("--force")
    _run(cmd, cwd=repo_root)


def _create_run(
    *,
    base_url: str,
    tenant_id: str,
    agent_id: str,
    user_id: str,
    recipe_id: str,
    text: str,
    summary_style: str,
    request_write: bool,
    timeout_secs: float,
) -> str:
    status, payload, raw_body = _http_json(
        base_url=base_url,
        method="POST",
        path="/v1/runs",
        tenant_id=tenant_id,
        user_role="owner",
        timeout_secs=timeout_secs,
        json_body={
            "agent_id": agent_id,
            "triggered_by_user_id": user_id,
            "recipe_id": recipe_id,
            "input": {
                "text": text,
                "summary_style": summary_style,
                "request_write": request_write,
            },
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
    run_id: str,
    timeout_secs: float,
) -> list[dict[str, object]]:
    status, payload, raw_body = _http_json(
        base_url=base_url,
        method="GET",
        path=f"/v1/runs/{run_id}/audit?limit=200",
        tenant_id=tenant_id,
        user_role=None,
        timeout_secs=timeout_secs,
    )
    if status != 200 or not isinstance(payload, list):
        raise RuntimeError(f"audit fetch failed status={status}: {raw_body}")
    return [event for event in payload if isinstance(event, dict)]


def _summarize_audit(audit_events: list[dict[str, object]]) -> dict[str, object]:
    event_counts = Counter()
    object_writes: list[dict[str, object]] = []
    for event in audit_events:
        event_type = event.get("event_type")
        if isinstance(event_type, str):
            event_counts[event_type] += 1
        payload = event.get("payload_json")
        if not isinstance(payload, dict):
            continue
        if payload.get("action_type") != "object.write":
            continue
        result = payload.get("result")
        if not isinstance(result, dict):
            continue
        object_writes.append(
            {
                "path": result.get("path"),
                "size_bytes": result.get("size_bytes"),
                "storage_ref": result.get("storage_ref"),
                "artifact_id": result.get("artifact_id"),
            }
        )
    return {
        "event_counts": dict(event_counts),
        "object_writes": object_writes,
        "latest_object_write": object_writes[-1] if object_writes else None,
    }


def main() -> int:
    repo_root = _repo_root()
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:18080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--agent-id", default=str(uuid.uuid4()))
    parser.add_argument("--agent-name", default="solo-lite-agent")
    parser.add_argument("--user-id", default=str(uuid.uuid4()))
    parser.add_argument("--user-subject", default="solo-lite-user")
    parser.add_argument("--user-display-name", default="Solo Lite User")
    parser.add_argument("--recipe-id", default="show_notes_v1")
    parser.add_argument(
        "--summary-style",
        default="summary",
        choices=["summary", "ops_digest"],
        help="Non-LLM output style for summarize skill.",
    )
    parser.add_argument(
        "--text",
        default="Summarize this update: solo-lite agent path is up, seeded, and processing runs.",
    )
    parser.add_argument(
        "--request-write",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Include input.request_write in run payload (default: true).",
    )
    parser.add_argument(
        "--start-stack",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Start solo-lite containers before seeding/running (default: true).",
    )
    parser.add_argument(
        "--build",
        action="store_true",
        help="Use stack-lite-up-build instead of stack-lite-up when starting containers.",
    )
    parser.add_argument(
        "--enable-context",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="When starting stack, enable worker agent-context loading (default: true).",
    )
    parser.add_argument(
        "--init-context",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Scaffold agent_context markdown files (default: true).",
    )
    parser.add_argument(
        "--context-root",
        default="agent_context",
        help="Path used for agent context scaffolding.",
    )
    parser.add_argument(
        "--force-context",
        action="store_true",
        help="Overwrite existing context files when used with --init-context.",
    )
    parser.add_argument(
        "--sqlite-path",
        default=DEFAULT_SQLITE_PATH,
        help="SQLite path inside worker-lite container for seed inserts.",
    )
    parser.add_argument("--ready-timeout-secs", type=float, default=120.0)
    parser.add_argument("--run-timeout-secs", type=float, default=90.0)
    parser.add_argument("--poll-interval-secs", type=float, default=1.0)
    args = parser.parse_args()

    if args.text == "-":
        args.text = sys.stdin.read()
    if not args.text.strip():
        raise RuntimeError("input text is empty")

    compose_cmd = _detect_compose_cmd()

    if args.start_stack:
        if _is_api_ready(args.base_url, args.tenant_id):
            print("note: solo-lite API already reachable; skipping stack start", file=sys.stderr)
        else:
            make_target = "stack-lite-up-build" if args.build else "stack-lite-up"
            stack_env = dict(os.environ)
            if args.enable_context:
                stack_env["WORKER_AGENT_CONTEXT_ENABLED"] = "1"
                stack_env.setdefault("WORKER_AGENT_CONTEXT_REQUIRED", "0")
                stack_env.setdefault(
                    "WORKER_AGENT_CONTEXT_ROOT", "/var/lib/secureagnt/agent-context"
                )
            _run(["make", make_target], cwd=repo_root, env=stack_env)
    elif args.enable_context:
        print(
            "note: --no-start-stack set; context loading depends on your currently running worker-lite env",
            file=sys.stderr,
        )

    _wait_for_api(args.base_url, args.tenant_id, args.ready_timeout_secs)

    seeded_agent_id, seeded_user_id = _seed_agent_user_sqlite_via_worker(
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
    if seeded_agent_id != args.agent_id:
        print(
            f"note: reusing existing agent id for tenant/name collision: {seeded_agent_id}",
            file=sys.stderr,
        )
    if seeded_user_id != args.user_id:
        print(
            f"note: reusing existing user id for tenant/subject collision: {seeded_user_id}",
            file=sys.stderr,
        )

    if args.init_context:
        _init_agent_context(
            repo_root=repo_root,
            context_root=(repo_root / args.context_root),
            tenant_id=args.tenant_id,
            agent_id=seeded_agent_id,
            agent_name=args.agent_name,
            force=args.force_context,
        )

    run_id = _create_run(
        base_url=args.base_url,
        tenant_id=args.tenant_id,
        agent_id=seeded_agent_id,
        user_id=seeded_user_id,
        recipe_id=args.recipe_id,
        text=args.text,
        summary_style=args.summary_style,
        request_write=args.request_write,
        timeout_secs=10.0,
    )
    run_payload = _poll_run(
        base_url=args.base_url,
        tenant_id=args.tenant_id,
        run_id=run_id,
        timeout_secs=args.run_timeout_secs,
        poll_interval_secs=args.poll_interval_secs,
    )
    audit_events = _fetch_audit(
        base_url=args.base_url,
        tenant_id=args.tenant_id,
        run_id=run_id,
        timeout_secs=10.0,
    )
    audit_summary = _summarize_audit(audit_events)

    print("solo-lite agent run complete")
    print(
        json.dumps(
            {
                "base_url": args.base_url,
                "tenant_id": args.tenant_id,
                "agent_id": seeded_agent_id,
                "user_id": seeded_user_id,
                "run_id": run_id,
                "run_status": run_payload.get("status"),
                "summary_style": args.summary_style,
                "started_at": run_payload.get("started_at"),
                "finished_at": run_payload.get("finished_at"),
                "audit_summary": audit_summary,
            },
            indent=2,
            sort_keys=True,
        )
    )
    print(f"export TENANT_ID={args.tenant_id}")
    print(f"export AGENT_ID={seeded_agent_id}")
    print(f"export USER_ID={seeded_user_id}")
    print(f"export RUN_ID={run_id}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
