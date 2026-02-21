#!/usr/bin/env python3
"""Validate role/capability guardrails on a running stack-lite API."""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request


def http_request(
    *,
    base_url: str,
    method: str,
    path: str,
    tenant_id: str,
    user_role: str,
    timeout_secs: float,
    json_body: dict[str, object] | None = None,
    user_id: str | None = None,
) -> tuple[int, str]:
    payload = None
    headers = {
        "x-tenant-id": tenant_id,
        "x-user-role": user_role,
    }
    if user_id is not None:
        headers["x-user-id"] = user_id
    if json_body is not None:
        payload = json.dumps(json_body).encode("utf-8")
        headers["content-type"] = "application/json"

    req = urllib.request.Request(
        url=f"{base_url.rstrip('/')}{path}",
        data=payload,
        headers=headers,
        method=method,
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout_secs) as resp:
            return resp.status, resp.read().decode("utf-8")
    except urllib.error.HTTPError as err:
        return err.code, err.read().decode("utf-8")


def assert_status(
    *,
    label: str,
    status: int,
    expected: int,
    body_text: str,
) -> None:
    if status != expected:
        raise RuntimeError(
            f"{label} expected {expected}, got {status}: {body_text}"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:18080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--timeout-secs", type=float, default=10.0)
    parser.add_argument(
        "--owner-user-id",
        default="00000000-0000-0000-0000-000000000001",
        help="synthetic user id used for approval-attribution guardrail checks",
    )
    parser.add_argument(
        "--agent-id",
        default="00000000-0000-0000-0000-000000000042",
        help="synthetic agent id used for heartbeat materialization guardrail checks",
    )
    args = parser.parse_args()

    checks = [
        (
            "viewer ops summary forbidden",
            "GET",
            "/v1/ops/summary?window_secs=3600",
            "viewer",
            None,
            None,
            403,
        ),
        (
            "viewer compliance list forbidden",
            "GET",
            "/v1/audit/compliance?limit=10",
            "viewer",
            None,
            None,
            403,
        ),
        (
            "operator policy update forbidden",
            "PUT",
            "/v1/audit/compliance/policy",
            "operator",
            {
                "compliance_hot_retention_days": 30,
                "compliance_archive_retention_days": 365,
                "legal_hold": True,
                "legal_hold_reason": "guardrail-check",
            },
            None,
            403,
        ),
        (
            "operator purge forbidden",
            "POST",
            "/v1/audit/compliance/purge",
            "operator",
            None,
            None,
            403,
        ),
        (
            "owner materialize requires approval_confirmed",
            "POST",
            f"/v1/agents/{args.agent_id}/heartbeat/materialize",
            "owner",
            {"apply": True},
            args.owner_user_id,
            403,
        ),
    ]

    for (
        label,
        method,
        path,
        role,
        json_body,
        user_id,
        expected_status,
    ) in checks:
        status, body_text = http_request(
            base_url=args.base_url,
            method=method,
            path=path,
            tenant_id=args.tenant_id,
            user_role=role,
            timeout_secs=args.timeout_secs,
            json_body=json_body,
            user_id=user_id,
        )
        assert_status(
            label=label,
            status=status,
            expected=expected_status,
            body_text=body_text,
        )
        print(f"{label}: ok ({status})", flush=True)

    print("stack-lite guardrails passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
