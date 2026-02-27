#!/usr/bin/env python3
"""Run a deterministic command-based solo-lite smoke check.

The script sends a single notify-style run with a fixed expected reply by default.
Use ``--inbound-smoke`` to exercise the inbound webhook path; when enabled the
script defaults to ``operator_chat_v1`` so the run can include the LLM + reply
loop. Use ``--expect-llm-inference`` when you want to validate that inbound
chat path and message return both include ``llm.infer`` and ``message.send``.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
import time
import uuid
import sqlite3

import solo_lite_agent_run as runner


def _is_readonly_db_error(err: sqlite3.OperationalError) -> bool:
    msg = str(err).lower()
    return "readonly" in msg or "read-only" in msg


def _resolve_default_agent_key_root() -> pathlib.Path:
    preferred = pathlib.Path("/opt/secureagnt/agent_keys")
    fallback_repo = pathlib.Path(runner.DEFAULT_AGENT_KEY_ROOT)
    fallback_tmp = pathlib.Path("/tmp/secureagnt-agent-keys")

    for candidate in (preferred, fallback_repo, fallback_tmp):
        if _is_agent_key_root_writable(candidate):
            return candidate
    # Worst-case fallback; caller will handle permission errors before keygen.
    return fallback_tmp


def _is_agent_key_root_writable(path: pathlib.Path) -> bool:
    try:
        path.mkdir(parents=True, exist_ok=True)
    except PermissionError:
        return False
    except OSError:
        return False
    return True


def _seed_agent_user_sqlite_local(
    *,
    sqlite_path: str,
    tenant_id: str,
    agent_id: str,
    agent_name: str,
    user_id: str,
    user_subject: str,
    user_display_name: str,
) -> tuple[str, str]:
    fallback_existing = _lookup_existing_entities_sqlite(
        sqlite_path=sqlite_path,
        tenant_id=tenant_id,
        agent_name=agent_name,
        user_subject=user_subject,
    )
    if fallback_existing is not None:
        return fallback_existing

    try:
        conn = sqlite3.connect(sqlite_path)
        try:
            conn.execute("BEGIN IMMEDIATE")
            agent_row = conn.execute(
                """
                INSERT INTO agents (id, tenant_id, name, status)
                VALUES (?, ?, ?, 'active')
                ON CONFLICT(tenant_id, name) DO UPDATE
                  SET status = excluded.status
                RETURNING id
                """,
                (agent_id, tenant_id, agent_name),
            ).fetchone()
            user_row = conn.execute(
                """
                INSERT INTO users (id, tenant_id, external_subject, display_name, status)
                VALUES (?, ?, ?, ?, 'active')
                ON CONFLICT(tenant_id, external_subject) DO UPDATE
                  SET display_name = excluded.display_name,
                      status = excluded.status
                RETURNING id
                """,
                (user_id, tenant_id, user_subject, user_display_name),
            ).fetchone()
            conn.commit()
        finally:
            conn.close()
    except sqlite3.OperationalError as err:
        if not _is_readonly_db_error(err):
            raise RuntimeError(
                f"failed seeding sqlite directly at {sqlite_path}: {err}"
            ) from err

        fallback = _lookup_existing_entities_sqlite(
            sqlite_path=sqlite_path,
            tenant_id=tenant_id,
            agent_name=agent_name,
            user_subject=user_subject,
        )
        if fallback is None:
            raise RuntimeError(
                f"sqlite database is read-only and no existing smoke entities found in {sqlite_path}: {err}"
            ) from err
        return fallback

def _lookup_existing_entities_sqlite(
    *,
    sqlite_path: str,
    tenant_id: str,
    agent_name: str,
    user_subject: str,
) -> tuple[str, str] | None:
    try:
        conn = sqlite3.connect(sqlite_path)
        try:
            agent_row = conn.execute(
                """
                SELECT id
                FROM agents
                WHERE tenant_id = ? AND name = ?
                LIMIT 1
                """,
                (tenant_id, agent_name),
            ).fetchone()
            if agent_row is None:
                agent_row = conn.execute(
                    """
                    SELECT id
                    FROM agents
                    WHERE tenant_id = ?
                    ORDER BY created_at DESC
                    LIMIT 1
                    """,
                    (tenant_id,),
                ).fetchone()
            user_row = conn.execute(
                """
                SELECT id
                FROM users
                WHERE tenant_id = ? AND external_subject = ?
                LIMIT 1
                """,
                (tenant_id, user_subject),
            ).fetchone()
            if user_row is None:
                user_row = conn.execute(
                    """
                    SELECT id
                    FROM users
                    WHERE tenant_id = ?
                    ORDER BY created_at DESC
                    LIMIT 1
                    """,
                    (tenant_id,),
                ).fetchone()
        finally:
            conn.close()
    except sqlite3.OperationalError as err:
        raise RuntimeError(f"failed reading sqlite fallback ids from {sqlite_path}: {err}") from err

    if not agent_row or not user_row:
        return None
    return str(agent_row[0]), str(user_row[0])



def _query_action_rows_sqlite_local(
    *,
    sqlite_path: str,
    run_id: str,
    tenant_id: str,
    action_type: str | None = None,
) -> list[dict[str, object]]:
    try:
        conn = sqlite3.connect(sqlite_path)
        try:
            query = """
                SELECT
                  ar.id,
                  ar.action_type,
                  ar.status,
                  ar.args_json,
                  ar2.status AS result_status,
                  ar2.result_json
                FROM runs r
                JOIN steps s ON s.run_id = r.id
                JOIN action_requests ar ON ar.step_id = s.id
                LEFT JOIN action_results ar2 ON ar2.action_request_id = ar.id
                WHERE r.id = ?
                  AND r.tenant_id = ?
            """
            params: list[object] = [run_id, tenant_id]
            if action_type:
                query += " AND ar.action_type = ?"
                params.append(action_type)
            query += "\n                ORDER BY s.created_at DESC, ar.created_at DESC\n"
            rows = conn.execute(
                query,
                tuple(params),
            ).fetchall()
        finally:
            conn.close()
    except sqlite3.OperationalError as err:
        raise RuntimeError(
            f"worker-lite sqlite query failed for {sqlite_path}: {err}"
        ) from err

    action_rows: list[dict[str, object]] = []
    for action_id, a_type, request_status, args_json, result_status, result_json in rows:
        action_rows.append(
            {
                "action_request_id": action_id,
                "action_type": a_type,
                "request_status": request_status,
                "result_status": result_status,
                "result_json": result_json,
                "args_json": args_json,
            }
        )
    return action_rows


def _query_message_rows_sqlite_local(
    *,
    sqlite_path: str,
    run_id: str,
    tenant_id: str,
) -> list[dict[str, object]]:
    return _query_action_rows_sqlite_local(
        sqlite_path=sqlite_path,
        run_id=run_id,
        tenant_id=tenant_id,
        action_type="message.send",
    )


def _create_webhook_trigger(
    *,
    base_url: str,
    tenant_id: str,
    agent_id: str,
    user_id: str,
    recipe_id: str,
    payload: dict[str, object],
    timeout_secs: float,
) -> str:
    status, response, raw_body = runner._http_json(
        base_url=base_url,
        method="POST",
        path="/v1/triggers/webhook",
        tenant_id=tenant_id,
        user_role="owner",
        timeout_secs=timeout_secs,
        json_body={
            "agent_id": agent_id,
            "triggered_by_user_id": user_id,
            "recipe_id": recipe_id,
            "input": payload,
            "requested_capabilities": [],
            "max_attempts": 3,
            "max_inflight_runs": 1,
            "jitter_seconds": 0,
        },
    )
    if status != 201 or not isinstance(response, dict):
        raise RuntimeError(f"webhook trigger creation failed status={status}: {raw_body}")
    trigger_id = response.get("id")
    if not isinstance(trigger_id, str):
        raise RuntimeError("webhook trigger creation response missing id")
    return trigger_id


def _post_trigger_event(
    *,
    base_url: str,
    tenant_id: str,
    trigger_id: str,
    event_id: str,
    payload: dict[str, object],
    timeout_secs: float,
) -> dict[str, object]:
    status, response, raw_body = runner._http_json(
        base_url=base_url,
        method="POST",
        path=f"/v1/triggers/{trigger_id}/events",
        tenant_id=tenant_id,
        user_role="owner",
        timeout_secs=timeout_secs,
        json_body={"event_id": event_id, "payload": payload},
    )
    if status not in {200, 202} or not isinstance(response, dict):
        raise RuntimeError(f"trigger event ingest failed status={status}: {raw_body}")
    return response


def _post_trigger_fire(
    *,
    base_url: str,
    tenant_id: str,
    trigger_id: str,
    idempotency_key: str,
    payload: dict[str, object],
    timeout_secs: float,
) -> str:
    status, response, raw_body = runner._http_json(
        base_url=base_url,
        method="POST",
        path=f"/v1/triggers/{trigger_id}/fire",
        tenant_id=tenant_id,
        user_role="owner",
        timeout_secs=timeout_secs,
        json_body={"idempotency_key": idempotency_key, "payload": payload},
    )
    if status not in {200, 202} or not isinstance(response, dict):
        raise RuntimeError(f"trigger fire failed status={status}: {raw_body}")
    run_id = response.get("run_id")
    if not isinstance(run_id, str) or not run_id.strip():
        raise RuntimeError(
            f"trigger fire response missing run_id for idempotency_key={idempotency_key!r}: {response}"
        )
    return run_id.strip()


def _find_run_id_for_trigger_event(
    *,
    sqlite_path: str,
    tenant_id: str,
    trigger_id: str,
    event_id: str,
) -> str | None:
    return _find_run_id_for_event(
        sqlite_path=sqlite_path,
        tenant_id=tenant_id,
        event_id=event_id,
        trigger_id=trigger_id,
    )


def _find_run_id_for_event(
    *,
    sqlite_path: str,
    tenant_id: str,
    event_id: str,
    trigger_id: str | None,
) -> str | None:
    try:
        conn = sqlite3.connect(sqlite_path)
        try:
            query = """
                SELECT tr.run_id
                FROM trigger_runs tr
                JOIN runs r ON r.id = tr.run_id
                WHERE tr.dedupe_key = ?
                  AND r.tenant_id = ?
            """
            params: list[object] = [event_id, tenant_id]
            if trigger_id:
                query += "\n  AND tr.trigger_id = ?"
                params.append(trigger_id)
            query += "\nORDER BY tr.created_at DESC\nLIMIT 1"
            row = conn.execute(query, tuple(params)).fetchone()
        finally:
            conn.close()
    except sqlite3.OperationalError as err:
        raise RuntimeError(
            f"failed to query trigger event->run mapping at {sqlite_path}: {err}"
        ) from err

    if not row:
        return None
    run_id = row[0]
    if not isinstance(run_id, str) or not run_id.strip():
        return None
    return run_id.strip()


def _poll_run_id_from_event(
    *,
    sqlite_path: str,
    tenant_id: str,
    trigger_id: str | None,
    event_id: str,
    timeout_secs: float,
    poll_interval_secs: float,
) -> str | None:
    deadline = time.monotonic() + timeout_secs
    while time.monotonic() < deadline:
        run_id = _find_run_id_for_event(
            sqlite_path=sqlite_path,
            tenant_id=tenant_id,
            trigger_id=trigger_id,
            event_id=event_id,
        )
        if run_id:
            return run_id
        time.sleep(poll_interval_secs)
    return None


def _build_run_input(
    *,
    command: str,
    destination: str,
    expected_reply: str,
    request_write: bool,
    message_approved: bool,
    summary_style: str,
    message_text: str | None = None,
) -> dict[str, object]:
    payload: dict[str, object] = {
        "text": command,
        "summary_style": summary_style,
        "request_write": request_write,
        "request_message": True,
        "message_approved": message_approved,
    }
    if message_text is not None:
        payload["message_text"] = message_text
    if destination:
        payload["destination"] = destination
    return payload


def _action_executed(row: dict[str, object]) -> bool:
    req_status = str(row.get("request_status", ""))
    result_status = str(row.get("result_status", ""))
    return req_status == "executed" or result_status == "executed"


def _build_event_payload(*, command: str, destination: str) -> dict[str, object]:
    payload: dict[str, object] = {"text": command}
    if destination:
        payload["channel"] = destination.split(":", 1)[0] if ":" in destination else destination
    payload["source"] = "slack_smoke"
    payload["command"] = command
    return payload


def _build_slack_event_payload(*, command: str, destination: str) -> dict[str, object]:
    slack_channel = ""
    if destination.startswith("slack:"):
        slack_channel = destination.split(":", 1)[1].strip()
    elif destination:
        slack_channel = destination.strip()

    return {
        "channel": "slack",
        "source": "slack_smoke",
        "text": command,
        "command": command,
        "channel_id": slack_channel,
        "event": {
            "type": "message",
            "text": command,
            "channel": slack_channel,
            "user": "",
            "ts": "",
        },
    }


def _load_event_payload(
    *,
    inline_json: str,
    json_file: str,
) -> dict[str, object] | None:
    source = inline_json.strip() or ""
    if source and json_file:
        raise RuntimeError("provide only one of --inbound-event-json or --inbound-event-json-file")
    if not source and not json_file:
        return None

    if source:
        raw_payload = source
    else:
        try:
            raw_payload = pathlib.Path(json_file).read_text()
        except OSError as err:
            raise RuntimeError(f"failed reading inbound event json file {json_file!r}: {err}") from err

    try:
        payload = json.loads(raw_payload)
    except json.JSONDecodeError as err:
        raise RuntimeError(f"invalid inbound event json payload: {err}") from err

    if not isinstance(payload, dict):
        raise RuntimeError("inbound event json payload must be a JSON object")
    return payload


def _extract_requested_text(row: dict[str, object]) -> str:
    args_raw = row.get("args_json")
    if not isinstance(args_raw, str):
        return ""
    try:
        args = json.loads(args_raw)
    except json.JSONDecodeError:
        return ""
    if not isinstance(args, dict):
        return ""
    text = args.get("text")
    if isinstance(text, str):
        return text
    return ""


def _check_expected_row(
    *,
    rows: list[dict[str, object]],
    expected: str,
    exact: bool,
    require_executed: bool,
) -> dict[str, object]:
    best: dict[str, object] | None = None
    for row in rows:
        requested_text = _extract_requested_text(row)
        if exact and requested_text != expected:
            continue
        if not exact and expected not in requested_text:
            continue
        best = row
        best["requested_text"] = requested_text
        break

    if best is None:
        observed = [
            {
                "action_request_id": row.get("action_request_id"),
                "requested_text": _extract_requested_text(row),
            }
            for row in rows
        ]
        raise RuntimeError(
            "command smoke did not find expected preconfigured reply\n"
            f"expected={expected!r}\nobserved={json.dumps(observed, indent=2)}"
        )

    if require_executed:
        req_status = str(best.get("request_status", ""))
        result_status = str(best.get("result_status", ""))
        if req_status != "executed" and result_status != "executed":
            raise RuntimeError(
                f"expected executed message action but got request_status={req_status!r}, "
                f"result_status={result_status!r}"
            )

    return best


def _create_command_run(
    *,
    base_url: str,
    tenant_id: str,
    agent_id: str,
    user_id: str,
    recipe_id: str,
    payload: dict[str, object],
    timeout_secs: float,
) -> str:
    status, response, raw_body = runner._http_json(
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
            "input": payload,
            "requested_capabilities": [],
        },
    )
    if status != 201 or not isinstance(response, dict):
        raise RuntimeError(f"run creation failed status={status}: {raw_body}")
    run_id = response.get("id")
    if not isinstance(run_id, str):
        raise RuntimeError("run creation response missing id")
    return run_id


def main() -> int:
    repo_root = runner._repo_root()
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--agent-id", default=str(uuid.uuid4()))
    parser.add_argument("--agent-name", default="solo-lite-smoke-agent")
    parser.add_argument("--user-id", default=str(uuid.uuid4()))
    parser.add_argument("--user-subject", default="solo-lite-smoke-user")
    parser.add_argument("--user-display-name", default="Solo Lite Smoke User")
    parser.add_argument("--recipe-id", default="notify_v1")
    parser.add_argument("--command", default="run smoke check command")
    parser.add_argument("--expected-reply", required=True)
    parser.add_argument(
        "--destination",
        default="",
        help="Optional message destination override, e.g. slack:C0AGRN3B895",
    )
    parser.add_argument(
        "--exact-match",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Require exact expected-reply match (default: true).",
    )
    parser.add_argument(
        "--expect-executed",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Require message.send request/result status executed.",
    )
    parser.add_argument(
        "--expect-llm-inference",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Require an executed llm.infer action in the run and skip strict message text comparison.",
    )
    parser.add_argument(
        "--omit-message-text",
        action=argparse.BooleanOptionalAction,
        default=False,
        help=(
            "Do not provide static message_text; inbound LLM loops can render the "
            "reply from {{llm_response}}."
        ),
    )
    parser.add_argument(
        "--request-write",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Include input.request_write in run payload.",
    )
    parser.add_argument(
        "--message-approved",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Set input.message_approved flag.",
    )
    parser.add_argument(
        "--summary-style",
        default="summary",
        choices=["summary", "ops_digest"],
        help="Summarization style for notify job text (not used for preconfigured reply).",
    )
    parser.add_argument(
        "--inbound-smoke",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="Use webhook-trigger ingest path to emulate inbound channel message before validating reply.",
    )
    parser.add_argument(
        "--inbound-live",
        action=argparse.BooleanOptionalAction,
        default=False,
        help=(
            "Wait for an existing inbound event run (useful after posting a real Slack event "
            "to /v1/triggers/<id>/events)."
        ),
    )
    parser.add_argument(
        "--inbound-provider",
        default="generic",
        choices=["generic", "slack"],
        help="Inbound event payload provider shape when using --inbound-smoke and no explicit payload JSON.",
    )
    parser.add_argument(
        "--inbound-event-json",
        default="",
        help="Raw JSON object to use as the inbound event_payload when --inbound-smoke.",
    )
    parser.add_argument(
        "--inbound-event-json-file",
        default="",
        help="Path to JSON file containing event_payload for --inbound-smoke.",
    )
    parser.add_argument("--ready-timeout-secs", type=float, default=120.0)
    parser.add_argument("--run-timeout-secs", type=float, default=90.0)
    parser.add_argument("--poll-interval-secs", type=float, default=1.0)
    parser.add_argument(
        "--agent-key-root",
        default=str(_resolve_default_agent_key_root()),
        help="Nostr key root on configured host installs.",
    )
    parser.add_argument(
        "--regen-agent-keys",
        action="store_true",
        help="Regenerate Nostr keypair even when one already exists.",
    )
    parser.add_argument(
        "--skip-agent-keypair",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Skip agent nostr keypair provisioning for command-only smoke checks.",
    )
    parser.add_argument(
        "--sqlite-path",
        default="/opt/secureagnt/secureagnt.sqlite3",
        help="SQLite DB path used for seed + query on host installs.",
    )
    parser.add_argument(
        "--inbound-event-id",
        default="",
        help="Optional event_id for inbound trigger-events path.",
    )
    parser.add_argument(
        "--inbound-trigger-id",
        default="",
        help="Optional trigger_id for live/observability path; when provided with --inbound-live it narrows polling.",
    )
    parser.add_argument(
        "--inbound-event-idem-key",
        default="",
        help=(
            "Optional idempotency key for inbound manual-fire fallback when trigger-scheduler "
            "delivery is unavailable or delayed."
        ),
    )
    args = parser.parse_args()

    if args.command == "-":
        args.command = sys.stdin.read()
    if not str(args.command).strip():
        raise RuntimeError("command is empty")

    if args.inbound_smoke and args.recipe_id == "notify_v1":
        args.recipe_id = "operator_chat_v1"

    if args.inbound_live and not args.inbound_smoke:
        raise RuntimeError("--inbound-live requires --inbound-smoke")

    if args.inbound_live and not args.inbound_event_id.strip():
        raise RuntimeError("--inbound-live requires --inbound-event-id")

    runner._wait_for_api(args.base_url, args.tenant_id, args.ready_timeout_secs)

    seeded_agent_id, seeded_user_id = _seed_agent_user_sqlite_local(
        sqlite_path=args.sqlite_path,
        tenant_id=args.tenant_id,
        agent_id=args.agent_id,
        agent_name=args.agent_name,
        user_id=args.user_id,
        user_subject=args.user_subject,
        user_display_name=args.user_display_name,
    )

    if not args.skip_agent_keypair:
        try:
            runner._ensure_agent_nostr_keypair(
                repo_root=repo_root,
                key_root=pathlib.Path(args.agent_key_root),
                tenant_id=args.tenant_id,
                agent_id=seeded_agent_id,
                regenerate=args.regen_agent_keys,
            )
        except (PermissionError, OSError) as err:
            print(
                f"[warning] skipping nostr keypair provisioning: {err}",
                file=sys.stderr,
            )

    if args.omit_message_text and not args.expect_llm_inference:
        raise RuntimeError("--omit-message-text is valid only with --expect-llm-inference.")

    message_text = args.expected_reply
    if args.inbound_smoke and args.omit_message_text:
        message_text = None

    run_payload = _build_run_input(
        command=args.command,
        destination=args.destination,
        expected_reply=args.expected_reply,
        request_write=args.request_write,
        message_approved=args.message_approved,
        summary_style=args.summary_style,
        message_text=message_text,
    )

    if not args.inbound_smoke:
        run_id = _create_command_run(
            base_url=args.base_url,
            tenant_id=args.tenant_id,
            agent_id=seeded_agent_id,
            user_id=seeded_user_id,
            recipe_id=args.recipe_id,
            payload=run_payload,
            timeout_secs=10.0,
        )
    else:
        event_id = args.inbound_event_id.strip() or str(uuid.uuid4())
        trigger_id: str | None = args.inbound_trigger_id.strip() or None
        if args.inbound_live:
            if not trigger_id:
                print(
                    "[info] --inbound-live will wait by event_id only; "
                    "if multiple triggers use --inbound-trigger-id to scope lookup.",
                    file=sys.stderr,
                )
            run_id = _poll_run_id_from_event(
                sqlite_path=args.sqlite_path,
                tenant_id=args.tenant_id,
                trigger_id=trigger_id,
                event_id=event_id,
                timeout_secs=args.run_timeout_secs,
                poll_interval_secs=args.poll_interval_secs,
            )
            if run_id is None:
                scope_desc = f" event_id={event_id!r}"
                if trigger_id:
                    scope_desc += f" trigger_id={trigger_id!r}"
                raise RuntimeError(
                    f"did not observe a run for live inbound event with {scope_desc} within {args.run_timeout_secs}s"
                )
        else:
            inbound_event_payload = _load_event_payload(
                inline_json=args.inbound_event_json,
                json_file=args.inbound_event_json_file,
            )
            if inbound_event_payload is None:
                if args.inbound_provider == "slack":
                    inbound_event_payload = _build_slack_event_payload(
                        command=args.command,
                        destination=args.destination,
                    )
                else:
                    inbound_event_payload = _build_event_payload(
                        command=args.command,
                        destination=args.destination,
                    )

            trigger_id = _create_webhook_trigger(
                base_url=args.base_url,
                tenant_id=args.tenant_id,
                agent_id=seeded_agent_id,
                user_id=seeded_user_id,
                recipe_id=args.recipe_id,
                payload=run_payload,
                timeout_secs=10.0,
            )
            event_resp = _post_trigger_event(
                base_url=args.base_url,
                tenant_id=args.tenant_id,
                trigger_id=trigger_id,
                event_id=event_id,
                payload=inbound_event_payload,
                timeout_secs=10.0,
            )
            if str(event_resp.get("status", "")).strip() not in {"queued", "duplicate"}:
                raise RuntimeError(
                    f"unexpected inbound event enqueue status {event_resp.get('status')!r} for event {event_id}"
                )
            run_id = _poll_run_id_from_event(
                sqlite_path=args.sqlite_path,
                tenant_id=args.tenant_id,
                trigger_id=trigger_id,
                event_id=event_id,
                timeout_secs=args.run_timeout_secs,
                poll_interval_secs=args.poll_interval_secs,
            )
            if run_id is None and args.inbound_event_idem_key.strip():
                run_id = _post_trigger_fire(
                    base_url=args.base_url,
                    tenant_id=args.tenant_id,
                    trigger_id=trigger_id,
                    idempotency_key=args.inbound_event_idem_key.strip(),
                    payload=inbound_event_payload,
                    timeout_secs=10.0,
                )
            elif run_id is None:
                raise RuntimeError(
                    f"did not observe a run for inbound event {event_id} on trigger {trigger_id} within {args.run_timeout_secs}s"
                )

    run_result = runner._poll_run(
        base_url=args.base_url,
        tenant_id=args.tenant_id,
        run_id=run_id,
        timeout_secs=args.run_timeout_secs,
        poll_interval_secs=args.poll_interval_secs,
    )
    if str(run_result.get("status")) not in runner.TERMINAL_RUN_STATUSES:
        raise RuntimeError(f"run did not finish terminally: {run_result.get('status')}")

    message_rows = _query_message_rows_sqlite_local(
        sqlite_path=args.sqlite_path,
        run_id=run_id,
        tenant_id=args.tenant_id,
    )
    if not message_rows:
        raise RuntimeError(f"no message.send action_request found for run_id={run_id}")

    llm_summary = None
    if args.expect_llm_inference and not args.inbound_smoke:
        raise RuntimeError("--expect-llm-inference is only supported with --inbound-smoke")

    if args.expect_llm_inference:
        executed_message_rows = [row for row in message_rows if _action_executed(row)]
        if not executed_message_rows:
            observed = [
                {
                    "action_request_id": row.get("action_request_id"),
                    "request_status": str(row.get("request_status")),
                    "result_status": str(row.get("result_status")),
                }
                for row in message_rows
            ]
            raise RuntimeError(
                "expected executed message.send action but none were executed: "
                f"{json.dumps(observed, indent=2)}"
            )
        matched = executed_message_rows[0]
        matched["requested_text"] = _extract_requested_text(matched)
        message_request_matched = True

        llm_rows = _query_action_rows_sqlite_local(
            sqlite_path=args.sqlite_path,
            run_id=run_id,
            tenant_id=args.tenant_id,
            action_type="llm.infer",
        )
        if not llm_rows:
            raise RuntimeError(f"no llm.infer action_request found for run_id={run_id}")
        executed_llm_rows = [row for row in llm_rows if _action_executed(row)]
        if not executed_llm_rows:
            observed_llm = [
                {
                    "action_request_id": row.get("action_request_id"),
                    "request_status": str(row.get("request_status")),
                    "result_status": str(row.get("result_status")),
                }
                for row in llm_rows
            ]
            raise RuntimeError(
                "expected executed llm.infer action but none were executed: "
                f"{json.dumps(observed_llm, indent=2)}"
            )
        llm_summary = executed_llm_rows[0]
    else:
        matched = _check_expected_row(
            rows=message_rows,
            expected=args.expected_reply,
            exact=args.exact_match,
            require_executed=args.expect_executed,
        )
        message_request_matched = True

    print(
        json.dumps(
            {
                "run_id": run_id,
                "run_status": run_result.get("status"),
                "agent_id": seeded_agent_id,
                "tenant_id": args.tenant_id,
                "message_request": matched,
                "llm_request": llm_summary,
                "message_expected": args.expected_reply,
                "message_request_matched": message_request_matched,
                "llm_inference_required": args.expect_llm_inference,
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
