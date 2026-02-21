#!/usr/bin/env python3
"""Interactive chat-like loop for solo-lite runs."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys
import uuid

import solo_lite_agent_run as runner


def _print_help() -> None:
    print("commands:")
    print("  /help            show commands")
    print("  /exit or /quit   end chat session")
    print("  /ids             print tenant/agent/user ids")
    print("  /keys            print agent npub/nsec file path")
    print("  /last            print last run id")
    print("  /style <name>    set summary style: summary | ops_digest")


def _start_if_needed(
    *,
    repo_root: pathlib.Path,
    base_url: str,
    tenant_id: str,
    start_stack: bool,
    build: bool,
    enable_context: bool,
) -> None:
    if not start_stack:
        if enable_context:
            print(
                "note: --no-start-stack set; context loading depends on current worker-lite env",
                file=sys.stderr,
            )
        return

    if runner._is_api_ready(base_url, tenant_id):
        print("note: solo-lite API already reachable; skipping stack start", file=sys.stderr)
        return

    make_target = "stack-lite-up-build" if build else "stack-lite-up"
    stack_env = dict(os.environ)
    if enable_context:
        stack_env["WORKER_AGENT_CONTEXT_ENABLED"] = "1"
        stack_env.setdefault("WORKER_AGENT_CONTEXT_REQUIRED", "0")
        stack_env.setdefault("WORKER_AGENT_CONTEXT_ROOT", "/var/lib/secureagnt/agent-context")
    runner._run(["make", make_target], cwd=repo_root, env=stack_env)


def main() -> int:
    repo_root = runner._repo_root()
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:18080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--agent-id", default=str(uuid.uuid4()))
    parser.add_argument("--agent-name", default="solo-lite-chat-agent")
    parser.add_argument("--user-id", default=str(uuid.uuid4()))
    parser.add_argument("--user-subject", default="solo-lite-chat-user")
    parser.add_argument("--user-display-name", default="Solo Lite Chat User")
    parser.add_argument("--recipe-id", default="show_notes_v1")
    parser.add_argument(
        "--summary-style",
        default="ops_digest",
        choices=["summary", "ops_digest"],
    )
    parser.add_argument(
        "--request-write",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Include input.request_write in each run payload (default: true).",
    )
    parser.add_argument(
        "--start-stack",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Start solo-lite containers when needed (default: true).",
    )
    parser.add_argument("--build", action="store_true", help="Use stack-lite-up-build.")
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
    parser.add_argument("--context-root", default="agent_context")
    parser.add_argument("--force-context", action="store_true")
    parser.add_argument("--sqlite-path", default=runner.DEFAULT_SQLITE_PATH)
    parser.add_argument("--agent-key-root", default=runner.DEFAULT_AGENT_KEY_ROOT)
    parser.add_argument("--regen-agent-keys", action="store_true")
    parser.add_argument(
        "--print-agent-nsec",
        action="store_true",
        help="Print AGENT_NSEC in startup exports (disabled by default).",
    )
    parser.add_argument("--ready-timeout-secs", type=float, default=120.0)
    parser.add_argument("--run-timeout-secs", type=float, default=90.0)
    parser.add_argument("--poll-interval-secs", type=float, default=1.0)
    args = parser.parse_args()

    compose_cmd = runner._detect_compose_cmd()
    _start_if_needed(
        repo_root=repo_root,
        base_url=args.base_url,
        tenant_id=args.tenant_id,
        start_stack=args.start_stack,
        build=args.build,
        enable_context=args.enable_context,
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

    key_info = runner._ensure_agent_nostr_keypair(
        repo_root=repo_root,
        key_root=(repo_root / args.agent_key_root),
        tenant_id=args.tenant_id,
        agent_id=seeded_agent_id,
        regenerate=args.regen_agent_keys,
    )

    if args.init_context:
        runner._init_agent_context(
            repo_root=repo_root,
            context_root=(repo_root / args.context_root),
            tenant_id=args.tenant_id,
            agent_id=seeded_agent_id,
            agent_name=args.agent_name,
            nostr_pubkey=key_info.get("npub"),
            force=args.force_context,
        )

    print("solo-lite chat ready")
    print(
        json.dumps(
            {
                "base_url": args.base_url,
                "tenant_id": args.tenant_id,
                "agent_id": seeded_agent_id,
                "agent_npub": key_info.get("npub"),
                "agent_nostr_key_status": key_info.get("status"),
                "agent_nsec_file": key_info.get("nsec_file"),
                "user_id": seeded_user_id,
                "recipe_id": args.recipe_id,
                "summary_style": args.summary_style,
            },
            indent=2,
            sort_keys=True,
        )
    )
    _print_help()
    if isinstance(key_info.get("npub"), str):
        print(f"export AGENT_NPUB={key_info['npub']}")
    if isinstance(key_info.get("nsec_file"), str):
        print(f"export AGENT_NSEC_FILE={key_info['nsec_file']}")
    if args.print_agent_nsec and isinstance(key_info.get("nsec_file"), str):
        nsec_value = pathlib.Path(str(key_info["nsec_file"])).read_text(encoding="utf-8").strip()
        print(f"export AGENT_NSEC={nsec_value}")

    summary_style = args.summary_style
    last_run_id: str | None = None
    while True:
        try:
            raw = input("you> ").strip()
        except EOFError:
            print()
            break
        except KeyboardInterrupt:
            print("\ninterrupted")
            break

        if not raw:
            continue
        if raw in {"/exit", "/quit"}:
            break
        if raw == "/help":
            _print_help()
            continue
        if raw == "/ids":
            print(f"TENANT_ID={args.tenant_id}")
            print(f"AGENT_ID={seeded_agent_id}")
            print(f"USER_ID={seeded_user_id}")
            continue
        if raw == "/keys":
            print(f"AGENT_NPUB={key_info.get('npub')}")
            print(f"AGENT_NSEC_FILE={key_info.get('nsec_file')}")
            continue
        if raw == "/last":
            print(last_run_id or "<none>")
            continue
        if raw.startswith("/style "):
            _, new_style = raw.split(" ", 1)
            new_style = new_style.strip()
            if new_style not in {"summary", "ops_digest"}:
                print("invalid style; choose summary or ops_digest")
                continue
            summary_style = new_style
            print(f"summary_style={summary_style}")
            continue

        run_id = runner._create_run(
            base_url=args.base_url,
            tenant_id=args.tenant_id,
            agent_id=seeded_agent_id,
            user_id=seeded_user_id,
            recipe_id=args.recipe_id,
            text=raw,
            summary_style=summary_style,
            request_write=args.request_write,
            timeout_secs=10.0,
        )
        run_payload = runner._poll_run(
            base_url=args.base_url,
            tenant_id=args.tenant_id,
            run_id=run_id,
            timeout_secs=args.run_timeout_secs,
            poll_interval_secs=args.poll_interval_secs,
        )
        audit_events = runner._fetch_audit(
            base_url=args.base_url,
            tenant_id=args.tenant_id,
            run_id=run_id,
            timeout_secs=10.0,
        )
        audit_summary = runner._summarize_audit(audit_events)
        last_run_id = run_id

        print(
            json.dumps(
                {
                    "run_id": run_id,
                    "run_status": run_payload.get("status"),
                    "summary_style": summary_style,
                    "latest_object_write": audit_summary.get("latest_object_write"),
                    "event_counts": audit_summary.get("event_counts"),
                },
                indent=2,
                sort_keys=True,
            )
        )

    print("solo-lite chat ended")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
