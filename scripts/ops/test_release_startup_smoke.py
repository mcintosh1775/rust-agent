#!/usr/bin/env python3
"""Unit tests for scripts/ops/release_startup_smoke.py."""

from __future__ import annotations

import importlib.util
import json
import sqlite3
import tempfile
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parent / "release_startup_smoke.py"
SPEC = importlib.util.spec_from_file_location("release_startup_smoke", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:  # pragma: no cover - defensive import failure path
    raise RuntimeError(f"unable to load release_startup_smoke from {SCRIPT_PATH}")
RELEASE_SMOKE_MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE_SMOKE_MOD)  # type: ignore[union-attr]
release_startup_smoke = RELEASE_SMOKE_MOD


def _write_notify_message(
    db_path: Path,
    message: str,
    tenant_id: str = "single",
    created_at: int = 10_000,
    run_id: str = "run-1",
    step_id: str = "step-1",
    action_id: str = "ar-1",
) -> None:
    conn = sqlite3.connect(str(db_path))
    try:
        conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                recipe_id TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS steps (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS action_requests (
                id TEXT PRIMARY KEY,
                step_id TEXT NOT NULL,
                action_type TEXT NOT NULL,
                args_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            """
        )
        conn.execute(
            "INSERT INTO runs (id, tenant_id, recipe_id, created_at) VALUES (?, ?, ?, ?)",
            (run_id, tenant_id, "notify_v1", created_at),
        )
        conn.execute(
            "INSERT INTO steps (id, run_id, created_at) VALUES (?, ?, ?)",
            (step_id, run_id, created_at),
        )
        conn.execute(
            "INSERT INTO action_requests (id, step_id, action_type, args_json, created_at) "
            "VALUES (?, ?, ?, ?, ?)",
            (
                action_id,
                step_id,
                "message.send",
                json.dumps({"text": message}),
                created_at,
            ),
        )
        conn.commit()
    finally:
        conn.close()


def test_assert_full_version_tokens_accepts_unbroken_version():
    failures = release_startup_smoke._assert_full_version_tokens(
        "Agent 'x' is now upgraded to SecureAgnt v0.2.28 (destinations: slack:C0AG...)", "v0.2.28"
    )
    assert not failures


def test_assert_full_version_tokens_flags_spaced_token():
    failures = release_startup_smoke._assert_full_version_tokens(
        "Agent 'x' is now upgraded to SecureAgnt v0. 2. 28"
    )
    assert failures == ["found spaced version token pattern such as 'v0. 2. 28'"]


def test_extract_message_text_reads_latest_message():
    with tempfile.TemporaryDirectory() as workspace:
        db_path = Path(workspace) / "secureagnt.sqlite3"
        _write_notify_message(db_path, "first message", created_at=100)
        _write_notify_message(db_path, "second message", created_at=200, run_id="run-2", step_id="step-2", action_id="ar-2")
        conn = sqlite3.connect(str(db_path))
        try:
            run_id, step_id, message = release_startup_smoke._extract_message_text(conn, "single")
            assert run_id == "run-2"
            assert step_id == "step-2"
            assert message == "second message"
        finally:
            conn.close()


def test_main_fails_when_expected_tag_missing():
    failures = release_startup_smoke._assert_full_version_tokens(
        "Agent 'x' is now upgraded to SecureAgnt v0.2.28",
        expected_tag="v0.2.29",
    )
    assert failures == ["expected release tag 'v0.2.29' not present in message"]


def test_main_fails_when_no_notify_v1_rows():
    with tempfile.TemporaryDirectory() as workspace:
        db_path = Path(workspace) / "secureagnt.sqlite3"
        conn = sqlite3.connect(str(db_path))
        try:
            conn.executescript(
                """
                CREATE TABLE runs (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL,
                    recipe_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE steps (
                    id TEXT PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE action_requests (
                    id TEXT PRIMARY KEY,
                    step_id TEXT NOT NULL,
                    action_type TEXT NOT NULL,
                    args_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                """
            )
            conn.commit()
            try:
                release_startup_smoke._extract_message_text(conn, "single")
            except RuntimeError as exc:
                assert "no notify_v1 startup message rows found" in str(exc)
            else:
                raise AssertionError("expected RuntimeError")
        finally:
            conn.close()
