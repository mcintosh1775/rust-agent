#!/usr/bin/env python3
"""Unit tests for scripts/ops/release_llm_smoke.py."""

from __future__ import annotations

import importlib.util
import json
import sqlite3
import tempfile
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve().parent / "release_llm_smoke.py"
SPEC = importlib.util.spec_from_file_location("release_llm_smoke", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"unable to load release_llm_smoke from {SCRIPT_PATH}")
RELEASE_LLM_SMOKE_MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE_LLM_SMOKE_MOD)  # type: ignore[union-attr]
release_llm_smoke = RELEASE_LLM_SMOKE_MOD


def _build_db(
    path: Path,
    *,
    run_status: str = "succeeded",
    result_status: str = "executed",
    action_status: str = "executed",
    gateway_route: str = "remote",
    model: str = "gpt-4o-mini",
    remote_host: str = "api.openai.com",
    prompt: str = "ping",
    prefer: str = "remote",
    created_at: int = 10_000,
    run_id: str = "run-1",
    step_id: str = "step-1",
    action_id: str = "action-1",
) -> None:
    conn = sqlite3.connect(str(path))
    try:
        conn.executescript(
            """
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                recipe_id TEXT NOT NULL,
                status TEXT NOT NULL,
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
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS action_results (
                id TEXT PRIMARY KEY,
                action_request_id TEXT NOT NULL,
                status TEXT NOT NULL,
                result_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            """
        )
        conn.execute(
            "INSERT OR REPLACE INTO runs (id, tenant_id, recipe_id, status, created_at) "
            "VALUES (?, 'single', 'llm_remote_v1', ?, ?)",
            (run_id, run_status, created_at),
        )
        conn.execute(
            "INSERT OR REPLACE INTO steps (id, run_id, created_at) VALUES (?, ?, ?)",
            (step_id, run_id, created_at),
        )
        conn.execute(
            """
            INSERT OR REPLACE INTO action_requests
              (id, step_id, action_type, args_json, status, created_at)
            VALUES (?, ?, 'llm.infer', ?, ?, ?)
            """,
            (
                action_id,
                step_id,
                json.dumps({"prompt": prompt, "prefer": prefer}),
                action_status,
                created_at,
            ),
        )
        conn.execute(
            """
            INSERT OR REPLACE INTO action_results
              (id, action_request_id, status, result_json, created_at)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                f"result-{action_id}",
                action_id,
                result_status,
                json.dumps(
                    {
                        "gateway": {
                            "route": gateway_route,
                            "model": model,
                            "remote_host": remote_host,
                        }
                    }
                ),
                created_at,
            ),
        )
        conn.commit()
    finally:
        conn.close()


def test_release_llm_smoke_passes_for_remote_route():
    with tempfile.TemporaryDirectory() as workspace:
        db_path = Path(workspace) / "secureagnt.sqlite3"
        _build_db(db_path)
        run_id, step_id, action_request_id, details = release_llm_smoke._assert_llm_smoke(
            db_path=db_path,
            tenant_id="single",
            recipe_id="llm_remote_v1",
            expected_route="remote",
            expected_model="gpt-4o-mini",
            expected_host="api.openai.com",
        )
        assert run_id == "run-1"
        assert step_id == "step-1"
        assert action_request_id == "action-1"
        assert details["result_route"] == "remote"
        assert details["result_model"] == "gpt-4o-mini"
        assert details["result_host"] == "api.openai.com"


def test_release_llm_smoke_fails_without_llm_rows():
    with tempfile.TemporaryDirectory() as workspace:
        db_path = Path(workspace) / "secureagnt.sqlite3"
        conn = sqlite3.connect(str(db_path))
        try:
            conn.executescript(
                """
                CREATE TABLE runs (id TEXT PRIMARY KEY, tenant_id TEXT, recipe_id TEXT, status TEXT, created_at INTEGER);
                CREATE TABLE steps (id TEXT PRIMARY KEY, run_id TEXT, created_at INTEGER);
                CREATE TABLE action_requests (id TEXT PRIMARY KEY, step_id TEXT, action_type TEXT, args_json TEXT, status TEXT, created_at INTEGER);
                CREATE TABLE action_results (id TEXT PRIMARY KEY, action_request_id TEXT, status TEXT, result_json TEXT, created_at INTEGER);
                """
            )
            conn.commit()
        finally:
            conn.close()

        try:
            release_llm_smoke._assert_llm_smoke(
                db_path=db_path,
                tenant_id="single",
                recipe_id="llm_remote_v1",
                expected_route="remote",
                expected_model=None,
                expected_host=None,
            )
        except RuntimeError as exc:
            assert "no llm.infer rows found" in str(exc)
        else:
            raise AssertionError("expected RuntimeError")


def test_release_llm_smoke_fails_when_local_route():
    with tempfile.TemporaryDirectory() as workspace:
        db_path = Path(workspace) / "secureagnt.sqlite3"
        _build_db(db_path, gateway_route="local", model="qwen2.5:7b-instruct", remote_host="")
        try:
            release_llm_smoke._assert_llm_smoke(
                db_path=db_path,
                tenant_id="single",
                recipe_id="llm_remote_v1",
                expected_route="remote",
                expected_model=None,
                expected_host=None,
            )
        except RuntimeError as exc:
            assert "route='remote'" in str(exc)
        else:
            raise AssertionError("expected RuntimeError")


def test_release_llm_smoke_fails_when_model_mismatch():
    with tempfile.TemporaryDirectory() as workspace:
        db_path = Path(workspace) / "secureagnt.sqlite3"
        _build_db(db_path, model="gpt-3.5-turbo")
        try:
            release_llm_smoke._assert_llm_smoke(
                db_path=db_path,
                tenant_id="single",
                recipe_id="llm_remote_v1",
                expected_route="remote",
                expected_model="gpt-4o-mini",
                expected_host="api.openai.com",
            )
        except RuntimeError as exc:
            assert "expected='gpt-4o-mini'" in str(exc)
        else:
            raise AssertionError("expected RuntimeError")
