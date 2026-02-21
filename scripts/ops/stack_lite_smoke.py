#!/usr/bin/env python3
"""Smoke-check the running solo-lite API profile over HTTP."""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass


@dataclass
class HttpResult:
    status: int
    content_type: str
    body_text: str


def _http_request(
    *,
    base_url: str,
    method: str,
    path: str,
    tenant_id: str,
    user_role: str,
    timeout_secs: float,
    json_body: dict[str, object] | None = None,
) -> HttpResult:
    payload = None
    headers = {
        "x-tenant-id": tenant_id,
        "x-user-role": user_role,
    }
    if json_body is not None:
        payload = json.dumps(json_body).encode("utf-8")
        headers["content-type"] = "application/json"
    request = urllib.request.Request(
        url=f"{base_url.rstrip('/')}{path}",
        data=payload,
        headers=headers,
        method=method,
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout_secs) as response:
            body_text = response.read().decode("utf-8")
            return HttpResult(
                status=response.status,
                content_type=response.headers.get("content-type", ""),
                body_text=body_text,
            )
    except urllib.error.HTTPError as error:
        body_text = error.read().decode("utf-8")
        return HttpResult(
            status=error.code,
            content_type=error.headers.get("content-type", ""),
            body_text=body_text,
        )


def _require_status(label: str, result: HttpResult, expected_status: int) -> None:
    if result.status != expected_status:
        raise RuntimeError(
            f"{label} expected status {expected_status}, got {result.status}: {result.body_text}"
        )


def _require_json_object(label: str, result: HttpResult) -> dict[str, object]:
    try:
        payload = json.loads(result.body_text)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{label} returned invalid JSON: {error}") from error
    if not isinstance(payload, dict):
        raise RuntimeError(f"{label} expected JSON object, got {type(payload).__name__}")
    return payload


def _require_json_array(label: str, result: HttpResult) -> list[object]:
    try:
        payload = json.loads(result.body_text)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{label} returned invalid JSON: {error}") from error
    if not isinstance(payload, list):
        raise RuntimeError(f"{label} expected JSON array, got {type(payload).__name__}")
    return payload


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://localhost:18080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--user-role", default="owner")
    parser.add_argument("--timeout-secs", type=float, default=10.0)
    args = parser.parse_args()

    summary_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/ops/summary?window_secs=3600",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("ops summary", summary_result, 200)
    summary_payload = _require_json_object("ops summary", summary_result)

    histogram_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/ops/latency-histogram?window_secs=3600",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("ops latency histogram", histogram_result, 200)

    compliance_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/audit/compliance?limit=10",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("compliance list", compliance_result, 200)
    _require_json_array("compliance list", compliance_result)

    compliance_export_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/audit/compliance/export?limit=10",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("compliance export", compliance_export_result, 200)
    if not compliance_export_result.content_type.startswith("application/x-ndjson"):
        raise RuntimeError(
            "compliance export expected application/x-ndjson content-type"
        )

    siem_summary_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/audit/compliance/siem/deliveries/summary",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("compliance siem summary", siem_summary_result, 200)
    _require_json_object("compliance siem summary", siem_summary_result)

    siem_slo_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/audit/compliance/siem/deliveries/slo?window_secs=3600",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("compliance siem slo", siem_slo_result, 200)
    _require_json_object("compliance siem slo", siem_slo_result)

    policy_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/audit/compliance/policy",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("compliance policy", policy_result, 200)
    _require_json_object("compliance policy", policy_result)

    verify_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/audit/compliance/verify",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("compliance verify", verify_result, 200)
    _require_json_object("compliance verify", verify_result)

    replay_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/audit/compliance/replay-package?run_id=00000000-0000-0000-0000-000000000000&include_payments=false",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("compliance replay package (missing run)", replay_result, 404)
    replay_payload = _require_json_object(
        "compliance replay package (missing run)",
        replay_result,
    )
    replay_error_code = (
        replay_payload.get("error", {}).get("code")
        if isinstance(replay_payload.get("error"), dict)
        else None
    )
    if replay_error_code == "SQLITE_PROFILE_ENDPOINT_UNAVAILABLE":
        raise RuntimeError(
            "compliance replay package should be routed in sqlite profile"
        )

    context_result = _http_request(
        base_url=args.base_url,
        method="GET",
        path="/v1/agents/00000000-0000-0000-0000-000000000042/context",
        tenant_id=args.tenant_id,
        user_role=args.user_role,
        timeout_secs=args.timeout_secs,
    )
    _require_status("agent context (missing profile)", context_result, 404)
    context_payload = _require_json_object("agent context (missing profile)", context_result)
    context_error_code = (
        context_payload.get("error", {}).get("code")
        if isinstance(context_payload.get("error"), dict)
        else None
    )
    if context_error_code == "SQLITE_PROFILE_ENDPOINT_UNAVAILABLE":
        raise RuntimeError("agent context should be routed in sqlite profile")

    print("stack-lite smoke passed")
    print(
        json.dumps(
            {
                "base_url": args.base_url,
                "tenant_id": args.tenant_id,
                "user_role": args.user_role,
                "ops_summary_tenant_id": summary_payload.get("tenant_id"),
                "ops_summary_queued_runs": summary_payload.get("queued_runs"),
                "ops_summary_running_runs": summary_payload.get("running_runs"),
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
