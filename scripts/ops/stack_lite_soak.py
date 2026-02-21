#!/usr/bin/env python3
"""Run repeated stack-lite smoke checks and report a soak summary."""

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path


def run_once(
    *,
    smoke_script: Path,
    base_url: str,
    tenant_id: str,
    user_role: str,
    timeout_secs: float,
) -> tuple[bool, float, str]:
    started = time.monotonic()
    completed = subprocess.run(
        [
            sys.executable,
            str(smoke_script),
            "--base-url",
            base_url,
            "--tenant-id",
            tenant_id,
            "--user-role",
            user_role,
            "--timeout-secs",
            str(timeout_secs),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    elapsed_secs = time.monotonic() - started
    output = (completed.stdout or "") + (completed.stderr or "")
    return completed.returncode == 0, elapsed_secs, output.strip()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:18080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--user-role", default="owner")
    parser.add_argument("--timeout-secs", type=float, default=10.0)
    parser.add_argument("--iterations", type=int, default=10)
    parser.add_argument("--interval-secs", type=float, default=2.0)
    parser.add_argument("--fail-fast", action="store_true")
    args = parser.parse_args()

    if args.iterations <= 0:
        raise SystemExit("--iterations must be greater than zero")
    if args.interval_secs < 0:
        raise SystemExit("--interval-secs must be >= 0")
    if args.timeout_secs <= 0:
        raise SystemExit("--timeout-secs must be > 0")

    smoke_script = Path(__file__).with_name("stack_lite_smoke.py")
    if not smoke_script.exists():
        raise SystemExit(f"missing smoke script: {smoke_script}")

    durations: list[float] = []
    failures: list[dict[str, object]] = []

    for attempt in range(1, args.iterations + 1):
        ok, elapsed_secs, output = run_once(
            smoke_script=smoke_script,
            base_url=args.base_url,
            tenant_id=args.tenant_id,
            user_role=args.user_role,
            timeout_secs=args.timeout_secs,
        )
        durations.append(elapsed_secs)
        status = "ok" if ok else "failed"
        print(
            f"[{attempt}/{args.iterations}] {status} elapsed={elapsed_secs:.3f}s",
            flush=True,
        )
        if not ok:
            failures.append(
                {
                    "attempt": attempt,
                    "elapsed_secs": round(elapsed_secs, 6),
                    "output": output,
                }
            )
            if args.fail_fast:
                break
        if attempt < args.iterations and args.interval_secs > 0:
            time.sleep(args.interval_secs)

    successful = len(durations) - len(failures)
    summary = {
        "base_url": args.base_url,
        "tenant_id": args.tenant_id,
        "user_role": args.user_role,
        "iterations_requested": args.iterations,
        "iterations_completed": len(durations),
        "success_count": successful,
        "failure_count": len(failures),
        "duration_secs": {
            "min": round(min(durations), 6),
            "avg": round(statistics.fmean(durations), 6),
            "max": round(max(durations), 6),
        },
    }
    print(json.dumps(summary, indent=2, sort_keys=True))

    if failures:
        first_failure = failures[0]
        print(
            f"stack-lite soak failed on attempt {first_failure['attempt']}:",
            file=sys.stderr,
        )
        if isinstance(first_failure.get("output"), str):
            print(first_failure["output"], file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
