#!/usr/bin/env python3
"""Smoke-check LLM inference execution in SQLite for release validation."""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from pathlib import Path


def _safe_json(value: str | None) -> dict[str, object]:
    if not value:
        return {}
    if isinstance(value, dict):  # pragma: no cover - defensive for future test adapters
        return value
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError:
        return {}
    if not isinstance(parsed, dict):
        return {}
    return parsed


def _extract_latest_llm_run(
    conn: sqlite3.Connection,
    tenant_id: str,
    recipe_id: str,
) -> tuple[str, str, str, str, str | None, str | None, str, str | None]:
    row = conn.execute(
        """
        SELECT
          r.id,
          r.status,
          s.id,
          ar.id,
          ar.status,
          ar_r.status,
          COALESCE(ar.args_json, ''),
          COALESCE(ar_r.result_json, '')
        FROM runs r
        JOIN steps s ON s.run_id = r.id
        JOIN action_requests ar ON ar.step_id = s.id
        LEFT JOIN action_results ar_r ON ar_r.action_request_id = ar.id
        WHERE r.tenant_id = ?
          AND r.recipe_id = ?
          AND ar.action_type = 'llm.infer'
        ORDER BY r.created_at DESC, s.created_at DESC, ar.created_at DESC
        LIMIT 1
        """,
        (tenant_id, recipe_id),
    ).fetchone()
    if row is None:
        raise RuntimeError(
            f"no llm.infer rows found for tenant='{tenant_id}' recipe_id='{recipe_id}'"
        )
    return (
        str(row[0]),
        str(row[1]),
        str(row[2]),
        str(row[3]),
        row[4] if row[4] is not None else None,
        row[5] if row[5] is not None else None,
        str(row[6]),
        str(row[7]) if row[7] is not None else None,
    )


def _assert_llm_smoke(
    *,
    db_path: Path,
    tenant_id: str,
    recipe_id: str,
    expected_route: str,
    expected_model: str | None,
    expected_host: str | None,
) -> tuple[str, str, str, dict[str, object]]:
    conn = sqlite3.connect(str(db_path))
    try:
        run_id, run_status, step_id, action_request_id, action_status, result_status, args_json, result_json = _extract_latest_llm_run(
            conn=conn,
            tenant_id=tenant_id,
            recipe_id=recipe_id,
        )
        args = _safe_json(args_json)
        result = _safe_json(result_json)
    finally:
        conn.close()

    if run_status != "succeeded":
        raise RuntimeError(
            f"llm smoke found run={run_id} but status={run_status!r} (expected succeeded)"
        )

    if action_status is not None and action_status not in {"executed", "completed"}:
        raise RuntimeError(
            f"llm smoke found action request {action_request_id} with status={action_status!r}"
        )
    if result_status is not None and result_status != "executed":
        raise RuntimeError(
            f"llm smoke found action result for {action_request_id} with status={result_status!r}"
        )

    gateway = result.get("gateway")
    if not isinstance(gateway, dict):
        raise RuntimeError(
            f"llm smoke run={run_id} missing result.gateway (result keys={list(result.keys())})"
        )

    route = result.get("route", gateway.get("route", ""))
    route = str(route if route is not None else "").strip()
    if expected_route and route != expected_route:
        raise RuntimeError(
            f"llm smoke run={run_id} route={route!r} expected={expected_route!r}"
        )

    model = result.get("model", gateway.get("model", ""))
    if expected_model and str(model) != expected_model:
        raise RuntimeError(
            f"llm smoke run={run_id} model={model!r} expected={expected_model!r}"
        )

    remote_host = result.get("remote_host", gateway.get("remote_host", ""))
    if expected_host and str(remote_host) != expected_host:
        raise RuntimeError(
            f"llm smoke run={run_id} remote_host={remote_host!r} expected={expected_host!r}"
        )

    return run_id, step_id, action_request_id, {
        "action_request_id": action_request_id,
        "action_status": action_status,
        "result_status": result_status,
        "result_model": model,
        "result_route": route,
        "result_host": remote_host,
        "prompt": args.get("prompt", ""),
        "llm_prefer": args.get("prefer", ""),
    }


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db-path", required=True)
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--recipe-id", default="llm_remote_v1")
    parser.add_argument(
        "--expected-route",
        default="remote",
        help="Expected gateway route, for example 'remote'",
    )
    parser.add_argument("--expected-model", default="", help="Expected LLM model key")
    parser.add_argument("--expected-host", default="", help="Expected remote host")
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    db_path = Path(args.db_path)
    if not db_path.exists():
        print(f"sqlite database not found: {db_path}", file=sys.stderr)
        return 1

    try:
        run_id, step_id, action_request_id, details = _assert_llm_smoke(
            db_path=db_path,
            tenant_id=args.tenant_id,
            recipe_id=args.recipe_id,
            expected_route=args.expected_route.strip(),
            expected_model=args.expected_model.strip() or None,
            expected_host=args.expected_host.strip() or None,
        )
    except RuntimeError as exc:
        print(f"release-llm-smoke failed: {exc}", file=sys.stderr)
        return 1

    print(
        "release-llm-smoke passed for "
        f"tenant={args.tenant_id!r} recipe={args.recipe_id!r} "
        f"run={run_id} step={step_id} action_request={action_request_id} "
        f"route={details['result_route']!r} model={details['result_model']!r} "
        f"host={details['result_host']!r}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
