#!/usr/bin/env python3
"""Run a minimal solo-lite SQLite smoke check for the run lifecycle."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import sqlite3
import subprocess
import sys
import tempfile


def _resolve_repo_root() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parents[2]


def _run_init(repo_root: pathlib.Path, db_path: pathlib.Path) -> None:
    init_script = repo_root / "scripts" / "ops" / "solo_lite_init.py"
    subprocess.run(
        [sys.executable, str(init_script), "--db-path", str(db_path)],
        check=True,
    )


def _seed_and_run_lifecycle(conn: sqlite3.Connection) -> dict[str, object]:
    tenant_id = "solo"
    agent_id = "11111111-1111-1111-1111-111111111111"
    user_id = "22222222-2222-2222-2222-222222222222"
    run_id = "33333333-3333-3333-3333-333333333333"
    step_id = "44444444-4444-4444-4444-444444444444"
    audit_id = "55555555-5555-5555-5555-555555555555"

    conn.execute(
        """
        INSERT INTO agents (id, tenant_id, name, status)
        VALUES (?, ?, ?, 'active')
        ON CONFLICT(id) DO NOTHING
        """,
        (agent_id, tenant_id, "solo-lite-agent"),
    )
    conn.execute(
        """
        INSERT INTO users (id, tenant_id, external_subject, display_name, status)
        VALUES (?, ?, ?, ?, 'active')
        ON CONFLICT(id) DO NOTHING
        """,
        (user_id, tenant_id, "solo-lite-user", "Solo Lite User"),
    )
    conn.execute(
        """
        INSERT INTO runs (
          id, tenant_id, agent_id, triggered_by_user_id, recipe_id, status,
          input_json, requested_capabilities, granted_capabilities, started_at
        )
        VALUES (?, ?, ?, ?, ?, 'running', ?, ?, ?, CURRENT_TIMESTAMP)
        """,
        (
            run_id,
            tenant_id,
            agent_id,
            user_id,
            "show_notes_v1",
            '{"text":"hello"}',
            "[]",
            "[]",
        ),
    )
    conn.execute(
        """
        INSERT INTO steps (id, run_id, tenant_id, agent_id, user_id, name, status, input_json, started_at)
        VALUES (?, ?, ?, ?, ?, 'skill.invoke', 'running', '{"text":"hello"}', CURRENT_TIMESTAMP)
        """,
        (step_id, run_id, tenant_id, agent_id, user_id),
    )
    conn.execute(
        """
        UPDATE steps
           SET status = 'succeeded',
               output_json = '{"markdown":"# Summary"}',
               finished_at = CURRENT_TIMESTAMP
         WHERE id = ?
        """,
        (step_id,),
    )
    conn.execute(
        """
        UPDATE runs
           SET status = 'succeeded',
               finished_at = CURRENT_TIMESTAMP
         WHERE id = ?
        """,
        (run_id,),
    )
    conn.execute(
        """
        INSERT INTO audit_events (id, run_id, step_id, tenant_id, agent_id, user_id, actor, event_type, payload_json)
        VALUES (?, ?, ?, ?, ?, ?, 'worker', 'run.succeeded', '{"ok":true}')
        """,
        (audit_id, run_id, step_id, tenant_id, agent_id, user_id),
    )
    conn.commit()

    summary_row = conn.execute(
        """
        SELECT
          SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END) AS queued_runs,
          SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) AS running_runs,
          SUM(CASE WHEN status = 'succeeded' THEN 1 ELSE 0 END) AS succeeded_runs_window,
          SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed_runs_window,
          AVG(
            CASE
              WHEN started_at IS NOT NULL AND finished_at IS NOT NULL
                THEN (julianday(finished_at) - julianday(started_at)) * 86400000.0
              ELSE NULL
            END
          ) AS avg_run_duration_ms
        FROM runs
        WHERE tenant_id = ?
        """,
        (tenant_id,),
    ).fetchone()

    audit_count = conn.execute(
        "SELECT COUNT(*) FROM audit_events WHERE tenant_id = ? AND run_id = ?",
        (tenant_id, run_id),
    ).fetchone()[0]
    run_state = conn.execute("SELECT status FROM runs WHERE id = ?", (run_id,)).fetchone()[0]

    result = {
        "queued_runs": int(summary_row[0] or 0),
        "running_runs": int(summary_row[1] or 0),
        "succeeded_runs_window": int(summary_row[2] or 0),
        "failed_runs_window": int(summary_row[3] or 0),
        "avg_run_duration_ms": float(summary_row[4] or 0.0),
        "audit_count": int(audit_count),
        "run_state": run_state,
    }

    if result["run_state"] != "succeeded":
        raise RuntimeError("expected run_state=succeeded")
    if result["succeeded_runs_window"] != 1:
        raise RuntimeError("expected succeeded_runs_window=1")
    if result["audit_count"] < 1:
        raise RuntimeError("expected at least one audit event")

    return result


def main() -> int:
    repo_root = _resolve_repo_root()
    env_db_path = os.environ.get("SOLO_LITE_DB_PATH")
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--db-path",
        type=pathlib.Path,
        default=pathlib.Path(env_db_path) if env_db_path else None,
        help="SQLite DB path. Defaults to a temporary file.",
    )
    parser.add_argument(
        "--keep-db",
        action="store_true",
        help="Do not delete the temporary DB on success.",
    )
    args = parser.parse_args()

    temp_db = None
    if args.db_path is None:
        fd, raw_path = tempfile.mkstemp(prefix="secureagnt-solo-lite-", suffix=".sqlite3")
        os.close(fd)
        temp_db = pathlib.Path(raw_path)
        db_path = temp_db
    else:
        db_path = args.db_path
        db_path.parent.mkdir(parents=True, exist_ok=True)

    _run_init(repo_root, db_path)

    conn = sqlite3.connect(db_path)
    try:
        conn.execute("PRAGMA foreign_keys = ON")
        result = _seed_and_run_lifecycle(conn)
    finally:
        conn.close()

    print("solo-lite smoke passed")
    print(json.dumps({"db_path": str(db_path), **result}, indent=2, sort_keys=True))

    if temp_db is not None and not args.keep_db:
        temp_db.unlink(missing_ok=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
