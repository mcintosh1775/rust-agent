#!/usr/bin/env python3
"""Smoke-check startup upgrade messages in SQLite for full release tokens."""

from __future__ import annotations

import argparse
import re
import sqlite3
import sys
from pathlib import Path


VERSION_TOKEN_LOOSE_RE = re.compile(r"\bv\d+(?:\s*\.\s*\d+)+\b")
VERSION_TOKEN_RE = re.compile(r"\bv\d+(?:\.\d+)+\b")


def _extract_message_text(conn: sqlite3.Connection, tenant_id: str) -> tuple[str, str, str]:
    row = conn.execute(
        """
        SELECT
            r.id,
            s.id,
            COALESCE(json_extract(ar.args_json, '$.text'), '') AS requested_text
        FROM runs r
        JOIN steps s ON s.run_id = r.id
        JOIN action_requests ar ON ar.step_id = s.id
        WHERE r.tenant_id = ?
          AND r.recipe_id = 'notify_v1'
          AND ar.action_type LIKE 'message.send%'
        ORDER BY r.created_at DESC, s.created_at DESC, ar.created_at DESC
        LIMIT 1
        """,
        (tenant_id,),
    ).fetchone()

    if row is None:
        raise RuntimeError(f"no notify_v1 startup message rows found for tenant '{tenant_id}'")

    run_id, step_id, requested_text = row
    if requested_text is None:
        requested_text = ""
    return str(run_id), str(step_id), str(requested_text)


def _assert_full_version_tokens(message: str, expected_tag: str | None) -> list[str]:
    failures = []

    matches = list(VERSION_TOKEN_LOOSE_RE.finditer(message))
    if not matches:
        failures.append("no semantic-like version token present")
        if expected_tag and expected_tag not in message:
            failures.append(f"expected release tag '{expected_tag}' not present in message")
        return failures

    for match in matches:
        token = match.group(0)
        if re.search(r"\.\s|\s\.", token):
            failures.append(f"found spaced version token pattern such as '{token}'")
            break

    if failures:
        return failures

    if not VERSION_TOKEN_RE.search(message):
        failures.append("no unbroken semantic version token present")

    if expected_tag and expected_tag not in message:
        failures.append(f"expected release tag '{expected_tag}' not present in message")

    return failures


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db-path", required=True, help="Path to secureagnt.sqlite3")
    parser.add_argument("--tenant-id", default="single", help="Tenant ID to inspect")
    parser.add_argument(
        "--expect-tag",
        dest="expect_tag",
        help="Expected release tag, for example v0.2.29",
    )
    return parser.parse_args()


def main() -> int:
    args = _parse_args()

    db_path = Path(args.db_path)
    if not db_path.exists():
        print(f"sqlite database not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    try:
        run_id, step_id, message = _extract_message_text(conn, args.tenant_id)
    finally:
        conn.close()

    failures = _assert_full_version_tokens(message, args.expect_tag)
    if failures:
        print(
            f"startup-message-smoke failed for run={run_id} step={step_id}: "
            + "; ".join(failures),
            file=sys.stderr,
        )
        print(f"message preview: {message[:240]}", file=sys.stderr)
        return 1

    print(
        f"startup-message-smoke passed for tenant='{args.tenant_id}' "
        f"run={run_id} step={step_id}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
