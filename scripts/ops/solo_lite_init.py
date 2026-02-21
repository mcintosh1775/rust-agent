#!/usr/bin/env python3
"""Initialize the SQLite schema for the M15 solo-lite profile."""

from __future__ import annotations

import argparse
import os
import pathlib
import sqlite3
import sys


def _resolve_repo_root() -> pathlib.Path:
    return pathlib.Path(__file__).resolve().parents[2]


def _default_db_path(repo_root: pathlib.Path) -> pathlib.Path:
    return repo_root / "var" / "solo-lite" / "secureagnt.sqlite3"


def _apply_pragmas(
    conn: sqlite3.Connection,
    *,
    journal_mode: str,
    synchronous: str,
    busy_timeout_ms: int,
) -> None:
    conn.execute("PRAGMA foreign_keys = ON")
    conn.execute(f"PRAGMA journal_mode = {journal_mode}")
    conn.execute(f"PRAGMA synchronous = {synchronous}")
    conn.execute(f"PRAGMA busy_timeout = {busy_timeout_ms}")


def _apply_migrations(conn: sqlite3.Connection, migrations_dir: pathlib.Path) -> int:
    applied = 0
    for migration in sorted(migrations_dir.glob("*.sql")):
        conn.executescript(migration.read_text(encoding="utf-8"))
        applied += 1
    return applied


def main() -> int:
    repo_root = _resolve_repo_root()
    env_db_path = os.environ.get("SOLO_LITE_DB_PATH")
    env_journal_mode = os.environ.get("SOLO_LITE_SQLITE_JOURNAL_MODE", "WAL")
    env_synchronous = os.environ.get("SOLO_LITE_SQLITE_SYNCHRONOUS", "NORMAL")
    env_busy_timeout_raw = os.environ.get("SOLO_LITE_SQLITE_BUSY_TIMEOUT_MS", "5000")
    try:
        env_busy_timeout = max(1, int(env_busy_timeout_raw))
    except ValueError:
        print(
            f"invalid SOLO_LITE_SQLITE_BUSY_TIMEOUT_MS `{env_busy_timeout_raw}`; expected integer",
            file=sys.stderr,
        )
        return 1
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--db-path",
        type=pathlib.Path,
        default=pathlib.Path(env_db_path) if env_db_path else _default_db_path(repo_root),
        help="SQLite database path for solo-lite profile.",
    )
    parser.add_argument(
        "--journal-mode",
        default=env_journal_mode,
        choices=["WAL", "DELETE", "TRUNCATE", "PERSIST", "MEMORY", "OFF"],
        help="SQLite PRAGMA journal_mode.",
    )
    parser.add_argument(
        "--synchronous",
        default=env_synchronous,
        choices=["OFF", "NORMAL", "FULL", "EXTRA"],
        help="SQLite PRAGMA synchronous.",
    )
    parser.add_argument(
        "--busy-timeout-ms",
        type=int,
        default=env_busy_timeout,
        help="SQLite PRAGMA busy_timeout in milliseconds.",
    )
    args = parser.parse_args()

    migrations_dir = repo_root / "migrations" / "sqlite"
    if not migrations_dir.is_dir():
        print(f"missing migrations directory: {migrations_dir}", file=sys.stderr)
        return 1

    db_path: pathlib.Path = args.db_path
    db_path.parent.mkdir(parents=True, exist_ok=True)

    conn = sqlite3.connect(db_path)
    try:
        _apply_pragmas(
            conn,
            journal_mode=args.journal_mode,
            synchronous=args.synchronous,
            busy_timeout_ms=max(1, args.busy_timeout_ms),
        )
        applied = _apply_migrations(conn, migrations_dir)
        conn.commit()
    finally:
        conn.close()

    print(
        "solo-lite sqlite initialized",
        f"db_path={db_path}",
        f"migrations_applied={applied}",
        f"journal_mode={args.journal_mode}",
        f"synchronous={args.synchronous}",
        f"busy_timeout_ms={max(1, args.busy_timeout_ms)}",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
