#!/usr/bin/env python3
import json
import re
import sys
import time


INSTRUCTION_PREFIXES = (
    "summarize",
    "create",
    "write",
    "provide",
    "draft",
    "generate",
)

RISK_KEYWORDS = (
    "critical",
    "error",
    "failed",
    "failure",
    "degraded",
    "backlog",
    "delay",
    "incident",
    "outage",
    "retry",
)

ACTION_HINTS = (
    ("queue", "Watch queue depth and verify inflight throughput over the next window."),
    ("alert", "Confirm alert routes and acknowledge any stale notifications."),
    ("error", "Review top error categories and confirm recent fixes hold."),
    ("latency", "Check p95 latency trend and compare against baseline."),
    ("fail", "Inspect failed runs and identify recurring failure signatures."),
)


def _extract_whitenoise_event(payload: dict) -> dict[str, str]:
    event_payload = payload.get("event_payload")
    if not isinstance(event_payload, dict):
        return {}
    if str(event_payload.get("channel", "")).strip().lower() != "whitenoise":
        return {}

    event = event_payload.get("event")
    if not isinstance(event, dict):
        event = {}

    author_pubkey = str(
        event_payload.get("author_pubkey") or event.get("pubkey") or ""
    ).strip()
    content = str(event.get("content") or "").strip()
    event_id = str(event.get("id") or "").strip()
    relay = str(event_payload.get("relay") or "").strip()
    return {
        "author_pubkey": author_pubkey,
        "content": content,
        "event_id": event_id,
        "relay": relay,
    }


def _normalize_text(raw_text: str) -> str:
    compact = " ".join(line.strip() for line in raw_text.splitlines() if line.strip())
    return re.sub(r"\s+", " ", compact).strip()


def _strip_instruction_prefix(text: str) -> str:
    lower = text.lower()
    if not lower:
        return text

    if any(lower.startswith(prefix) for prefix in INSTRUCTION_PREFIXES):
        if ":" in text:
            _, candidate = text.split(":", 1)
            candidate = candidate.strip()
            if candidate:
                return candidate
    return text


def _sentence_case(text: str) -> str:
    stripped = text.strip(" -\t")
    if not stripped:
        return stripped
    normalized = re.sub(r"\s+", " ", stripped)
    cased = normalized[0].upper() + normalized[1:]
    if cased[-1] not in ".!?":
        cased += "."
    return cased


def _extract_points(text: str) -> list[str]:
    parts = re.split(r"[.;\n]+", text)
    points: list[str] = []
    seen: set[str] = set()

    for part in parts:
        chunk = part.strip()
        if not chunk:
            continue

        subparts = re.split(r"\band\b", chunk, flags=re.IGNORECASE)
        for subpart in subparts:
            candidate = re.sub(
                r"^(from this note|this note|note|update|that|which)\b[:,\-\s]*",
                "",
                subpart.strip(),
                flags=re.IGNORECASE,
            )
            candidate = re.sub(r"\s+", " ", candidate).strip(" -")
            if not candidate:
                continue
            key = candidate.lower()
            if key in seen:
                continue
            seen.add(key)
            points.append(_sentence_case(candidate))
            if len(points) >= 4:
                return points
    return points


def _keyword_is_negated(text: str, keyword: str) -> bool:
    return bool(
        re.search(
            rf"\b(no|not|without|none|zero)\b[\w\s,/-]{{0,24}}\b{re.escape(keyword)}\b",
            text,
            flags=re.IGNORECASE,
        )
    )


def _point_risk_keywords(point: str) -> list[str]:
    lower = point.lower()
    matches: list[str] = []
    for keyword in RISK_KEYWORDS:
        if keyword in lower and not _keyword_is_negated(lower, keyword):
            matches.append(keyword)
    return matches


def summarize_text(text: str) -> str:
    normalized = _normalize_text(text)
    if not normalized:
        return "# Summary\n\n_No content provided._"

    content = _strip_instruction_prefix(normalized)
    points = _extract_points(content)
    if not points:
        preview = content[:240].strip()
        return "# Summary\n\n" + _sentence_case(preview)

    bullet_lines = "\n".join(f"- {point}" for point in points)
    return f"# Summary\n\nKey points:\n{bullet_lines}"


def summarize_ops_digest(text: str) -> str:
    normalized = _normalize_text(text)
    if not normalized:
        return "# Operations Digest\n\n_No content provided._"

    content = _strip_instruction_prefix(normalized)
    points = _extract_points(content)
    if not points:
        points = [_sentence_case(content[:240])]

    lower_points = [point.lower() for point in points]
    risks = []
    point_risk_matches: list[list[str]] = []
    for point in points:
        matched_keywords = _point_risk_keywords(point)
        point_risk_matches.append(matched_keywords)
        if matched_keywords:
            risks.append(point)
    if not risks:
        risks = ["No critical risk terms detected in the provided note."]

    next_actions = []
    for keyword, action_text in ACTION_HINTS:
        if any(keyword in lower for lower in lower_points):
            next_actions.append(action_text)
    if not next_actions:
        next_actions = [
            "Re-check ops summary in 15 minutes to confirm status remains stable.",
            "Capture a brief handoff note for the next operator shift.",
        ]

    todo_items = [
        "Record this digest in session notes.",
        "Verify artifact outputs for the latest run.",
    ]
    if any("critical" in matches for matches in point_risk_matches):
        todo_items.append("Escalate and open an incident thread for critical signals.")

    situation_block = "\n".join(f"- {point}" for point in points[:4])
    risk_block = "\n".join(f"- {risk}" for risk in risks[:4])
    action_block = "\n".join(f"{idx}. {item}" for idx, item in enumerate(next_actions[:4], 1))
    todo_block = "\n".join(f"- {item}" for item in todo_items[:4])
    return (
        "# Operations Digest\n\n"
        "## Situation\n"
        f"{situation_block}\n\n"
        "## Risks\n"
        f"{risk_block}\n\n"
        "## Next Actions\n"
        f"{action_block}\n\n"
        "## TODO\n"
        f"{todo_block}"
    )


def summarize_one_liner(text: str) -> str:
    normalized = _normalize_text(text)
    if not normalized:
        return "No content provided."
    content = _strip_instruction_prefix(normalized)
    points = _extract_points(content)
    if points:
        return " ".join(points[:2])[:280]
    return _sentence_case(content[:280])


def handle_describe(message: dict) -> dict:
    return {
        "type": "describe_result",
        "id": message["id"],
        "skill": {
            "name": "summarize_transcript",
            "version": "0.1.0",
            "description": "Summarize transcript text into markdown.",
            "inputs_schema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "summary_style": {"type": "string"},
                    "mode": {"type": "string"},
                    "sleep_s": {"type": "number"},
                    "bytes": {"type": "integer"},
                    "request_write": {"type": "boolean"},
                    "request_message": {"type": "boolean"},
                    "destination": {"type": "string"},
                    "message_text": {"type": "string"},
                    "request_llm": {"type": "boolean"},
                    "llm_prompt": {"type": "string"},
                    "llm_prefer": {"type": "string"},
                    "llm_max_tokens": {"type": "integer"},
                    "request_local_exec": {"type": "boolean"},
                    "local_exec_template_id": {"type": "string"},
                    "local_exec_path": {"type": "string"},
                    "local_exec_lines": {"type": "integer"},
                    "request_payment": {"type": "boolean"},
                    "payment_destination": {"type": "string"},
                    "payment_operation": {"type": "string"},
                    "payment_idempotency_key": {"type": "string"},
                    "payment_amount_msat": {"type": "integer"},
                    "payment_approved": {"type": "boolean"},
                    "payment_invoice": {"type": "string"},
                    "payment_description": {"type": "string"},
                    "agent_context": {"type": "object"},
                },
            },
            "outputs_schema": {
                "type": "object",
                "properties": {"markdown": {"type": "string"}},
                "required": ["markdown"],
            },
            "requested_capabilities": [
                {"capability": "object.write", "scope": "shownotes/*"},
                {"capability": "message.send", "scope": "whitenoise:*"},
                {"capability": "payment.send", "scope": "nwc:*"},
                {"capability": "llm.infer", "scope": "local:*"},
                {"capability": "local.exec", "scope": "local.exec:file.head"},
            ],
            "action_types": [
                "object.write",
                "message.send",
                "payment.send",
                "llm.infer",
                "local.exec",
            ],
        },
    }


def handle_invoke(message: dict) -> dict:
    payload = message.get("input") or {}
    mode = payload.get("mode", "success")

    if mode == "timeout":
        time.sleep(float(payload.get("sleep_s", 10)))

    if mode == "crash":
        sys.exit(17)

    if mode == "oversize":
        size = int(payload.get("bytes", 100_000))
        markdown = "x" * size
    else:
        event_ctx = _extract_whitenoise_event(payload)
        configured_text = str(payload.get("text", ""))
        text = configured_text
        if not configured_text.strip() and event_ctx.get("content"):
            text = event_ctx["content"]
        summary_style = str(payload.get("summary_style", "summary")).strip().lower()
        if summary_style == "ops_digest":
            markdown = summarize_ops_digest(text)
        else:
            markdown = summarize_text(text)

    action_requests = []
    if payload.get("request_write"):
        action_requests.append(
            {
                "action_id": "a-1",
                "action_type": "object.write",
                "args": {
                    "path": "shownotes/ep245.md",
                    "content": markdown,
                },
                "justification": "Persist generated show notes",
            }
        )
    event_ctx = _extract_whitenoise_event(payload)
    request_message_raw = payload.get("request_message")
    if request_message_raw is None:
        request_message = bool(event_ctx.get("author_pubkey")) and bool(event_ctx.get("content"))
    else:
        request_message = bool(request_message_raw)
    if request_message:
        destination = payload.get("destination")
        if destination is None and event_ctx.get("author_pubkey"):
            destination = f"whitenoise:{event_ctx['author_pubkey']}"
        reply_text = payload.get("message_text")
        if reply_text is None:
            one_liner = summarize_one_liner(str(payload.get("text", "")) or event_ctx.get("content", ""))
            if event_ctx.get("event_id"):
                reply_text = f"Reply {event_ctx['event_id'][:8]}: {one_liner}"
            else:
                reply_text = one_liner
        action_requests.append(
            {
                "action_id": "a-2",
                "action_type": "message.send",
                "args": {
                    "destination": destination or "whitenoise:npub1example",
                    "text": str(reply_text)[:480],
                },
                "justification": "Send completion message",
            }
        )
    if payload.get("request_llm"):
        llm_args = {
            "prompt": payload.get("llm_prompt", markdown[:800]),
            "prefer": payload.get("llm_prefer", "local"),
        }
        if payload.get("llm_max_tokens") is not None:
            llm_args["max_tokens"] = int(payload.get("llm_max_tokens"))
        action_requests.append(
            {
                "action_id": "a-llm",
                "action_type": "llm.infer",
                "args": llm_args,
                "justification": "Generate helper completion with policy-gated model route",
            }
        )
    if payload.get("request_local_exec"):
        action_requests.append(
            {
                "action_id": "a-3",
                "action_type": "local.exec",
                "args": {
                    "template_id": payload.get("local_exec_template_id", "file.head"),
                    "path": payload.get("local_exec_path", ""),
                    "lines": payload.get("local_exec_lines", 5),
                },
                "justification": "Run scoped local command template",
            }
        )
    if payload.get("request_payment"):
        payment_args = {
            "destination": payload.get("payment_destination", "nwc:wallet-main"),
            "operation": payload.get("payment_operation", "pay_invoice"),
            "idempotency_key": payload.get("payment_idempotency_key", "payment-001"),
        }
        if payload.get("payment_amount_msat") is not None:
            payment_args["amount_msat"] = int(payload.get("payment_amount_msat"))
        if payload.get("payment_approved") is not None:
            payment_args["payment_approved"] = bool(payload.get("payment_approved"))
        if payload.get("payment_invoice") is not None:
            payment_args["invoice"] = str(payload.get("payment_invoice"))
        if payload.get("payment_description") is not None:
            payment_args["description"] = str(payload.get("payment_description"))
        action_requests.append(
            {
                "action_id": "a-payment",
                "action_type": "payment.send",
                "args": payment_args,
                "justification": "Settle or request invoice over NWC rail",
            }
        )

    return {
        "type": "invoke_result",
        "id": message["id"],
        "output": {"markdown": markdown},
        "action_requests": action_requests,
    }


def main() -> int:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        incoming = json.loads(line)
        msg_type = incoming.get("type")
        if msg_type == "describe":
            response = handle_describe(incoming)
        elif msg_type == "invoke":
            response = handle_invoke(incoming)
        else:
            response = {
                "type": "error",
                "id": incoming.get("id", "unknown"),
                "error": {
                    "code": "INVALID_INPUT",
                    "message": f"unsupported message type: {msg_type}",
                    "details": {},
                },
            }

        print(json.dumps(response), flush=True)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
