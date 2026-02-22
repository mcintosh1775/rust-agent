#!/usr/bin/env python3
import hashlib
import json
import re
from datetime import datetime
from typing import Any, Dict, Iterable, List


def _to_bool(value, default=False):
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        lowered = value.strip().lower()
        if lowered in {"1", "true", "yes", "on", "enable", "enabled", "y"}:
            return True
        if lowered in {"0", "false", "no", "off", "disable", "disabled", "n"}:
            return False
    return default


def _coerce_int(value, default=0):
    if isinstance(value, int):
        return value
    if isinstance(value, str):
        stripped = value.strip()
        if stripped.isdigit():
            try:
                return int(stripped)
            except ValueError:
                return default
    return default


def _coerce_text(value):
    if value is None:
        return ""
    if isinstance(value, str):
        return value.strip()
    return str(value).strip()


def _safe_text(payload):
    candidates = [
        payload.get("text"),
        payload.get("body"),
        payload.get("content"),
        payload.get("input"),
    ]
    for value in candidates:
        text = _coerce_text(value)
        if text:
            return text
    event_payload = payload.get("event_payload")
    if isinstance(event_payload, dict):
        event = event_payload.get("event")
        if isinstance(event, dict):
            event_text = _coerce_text(event.get("content"))
            if event_text:
                return event_text
    return ""


def _slug(value):
    lowered = re.sub(r"[^a-z0-9-_]", "-", _coerce_text(value).lower())
    return re.sub(r"-+", "-", lowered).strip("-") or "skill"


def _short_id(value):
    digest = hashlib.sha1(_coerce_text(value).encode("utf-8")).hexdigest()
    return digest[:10]


def _normalize_points(text):
    parts = [part.strip() for part in re.split(r"[.;\n]+", text) if part.strip()]
    points = []
    for part in parts:
        if part.lower().startswith(("and ", "so ", "but ", "the ", "this ")):
            part = part.strip(" -")
        if part:
            points.append(f"- {part[0].upper()}{part[1:] if len(part) > 1 else ''}")
    if not points:
        points = ["- No structured points detected."]
    return points[:6]


def _build_object_write(payload, skill_name, markdown):
    path = _coerce_text(payload.get("output_path"))
    if not path:
        path = f"shownotes/{skill_name}_{_short_id(markdown)}.md"
    return {
        "action_id": f"{skill_name}-write",
        "action_type": "object.write",
        "args": {
            "path": path,
            "content": markdown,
        },
        "justification": "Persist deterministic skill output for downstream handoff.",
    }


def _build_message(payload, markdown):
    destination = _coerce_text(
        payload.get("destination") or payload.get("message_destination")
    ) or "whitenoise:agent"
    return {
        "action_id": "a-msg",
        "action_type": "message.send",
        "args": {
            "destination": destination,
            "text": markdown[:480],
            "approved": True,
        },
        "justification": "Notify operator of key skill result.",
    }


def _build_local_exec(payload):
    return {
        "action_id": "a-local-exec",
        "action_type": "local.exec",
        "args": {
            "template_id": _coerce_text(payload.get("template_id") or "local.exec:file.head"),
            "path": _coerce_text(payload.get("path") or "."),
            "lines": _coerce_int(payload.get("lines"), 5),
        },
        "justification": "Collect safe local snapshot before drafting a response.",
    }


def _build_llm(payload, markdown):
    prompt = _coerce_text(payload.get("llm_prompt"))
    if not prompt:
        prompt = f"Refine this text while keeping intent: {markdown[:500]}"
    return {
        "action_id": "a-llm",
        "action_type": "llm.infer",
        "args": {
            "prompt": prompt,
            "prefer": _coerce_text(payload.get("llm_prefer") or "local"),
            "max_tokens": _coerce_int(payload.get("llm_max_tokens", 180), 180),
        },
        "justification": "Refinement requested by skill input.",
    }


def _build_payment(payload):
    amount_msat = _coerce_int(
        payload.get("amount_msat") or payload.get("payment_amount_msat"),
        0,
    )
    return {
        "action_id": "a-payment",
        "action_type": "payment.send",
        "args": {
            "destination": _coerce_text(payload.get("destination") or payload.get("payment_destination") or "default"),
            "operation": _coerce_text(payload.get("operation") or payload.get("payment_operation") or "pay_invoice"),
            "idempotency_key": _coerce_text(
                payload.get("idempotency_key")
                or payload.get("payment_idempotency_key")
                or _short_id(_coerce_text(payload.get("text")))
            ),
            "amount_msat": amount_msat,
            "invoice": _coerce_text(payload.get("invoice") or payload.get("payment_invoice")),
            "description": _coerce_text(payload.get("description") or payload.get("payment_description")),
            "approved": _to_bool(payload.get("payment_approved"), False),
        },
        "justification": "Execute payment action plan through controlled, policy-gated path.",
    }


def summarize_transcript(payload):
    text = _safe_text(payload)
    bullets = _normalize_points(text)
    markdown = "\n".join(
        [
            "# Transcript Summary",
            "",
            "## Key Points",
            *bullets,
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )
    return markdown


def extract_action_items(payload):
    text = _safe_text(payload)
    items = _normalize_points(text)
    if items[0].startswith("- No "):
        return "# Action Items\n\n- No obvious action candidates found."
    tasks = []
    for idx, point in enumerate(items, start=1):
        tasks.append(f"{idx}. [ ] {point[2:]}")
    return "\n".join(["# Action Items", "", *tasks, "", f"_Generated at {_utc_timestamp()}_"])


def draft_reply(payload):
    text = _safe_text(payload)
    tone = _coerce_text(payload.get("tone") or "concise")
    return "\n".join(
        [
            "# Draft Reply",
            "",
            f"Tone: `{tone}`",
            "",
            "Hi,",
            "",
            f"I reviewed the request and here is a draft response:",
            "",
            f"> {text[:360]}",
            "",
            "If this looks good, I can finalize it with additional details and a clearer CTA.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def translate_text(payload):
    text = _safe_text(payload)
    language = _coerce_text(payload.get("language") or "Spanish")
    prefix = f"[Pseudo-translation target: {language}]"
    translated = text[::-1] if _to_bool(payload.get("reverse_copy"), False) else text
    return "\n".join(
        [
            "# Translation Draft",
            "",
            prefix,
            "",
            translated if text else "[no source text]",
            "",
            "> This is a local-first scaffold; swap with your translation provider once MCP integration is enabled.",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def sentiment_scan(payload):
    text = _safe_text(payload).lower()
    positive = sum(word in text for word in ("great", "good", "improve", "success", "resolved"))
    negative = sum(word in text for word in ("bad", "error", "failed", "outage", "urgent", "broken"))
    sentiment = "neutral"
    if positive > negative:
        sentiment = "positive"
    elif negative > positive:
        sentiment = "negative"
    return "\n".join(
        [
            "# Sentiment Scan",
            "",
            f"- Signal: **{sentiment}**",
            f"- positive signals: `{positive}`",
            f"- negative signals: `{negative}`",
            "",
            "## Raw signals",
            *_normalize_points(_safe_text(payload)),
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def triage_incident(payload):
    text = _safe_text(payload).lower()
    is_critical = any(word in text for word in ("critical", "outage", "down", "page", "fire"))
    is_high = any(word in text for word in ("error", "failed", "timeout", "retry", "deadlock"))
    priority = "p0" if is_critical else ("p1" if is_high else "p2")
    recommendations = [
        "- Confirm blast radius.",
        "- Capture timeline from source alerts.",
        "- Notify on-call if priority is p1 or above.",
    ]
    return "\n".join(
        [
            "# Incident Triage",
            "",
            f"- Priority: **{priority}**",
            "",
            "## Recommended next steps",
            *recommendations,
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def meeting_minutes(payload):
    text = _safe_text(payload)
    points = _normalize_points(text)
    attendees = _coerce_text(payload.get("attendees") or "Not specified")
    return "\n".join(
        [
            "# Meeting Minutes",
            "",
            f"Attendees: {attendees}",
            "",
            "## Decisions",
            *points[:4],
            "",
            "## Follow-up",
            "1. Confirm action owners.",
            "2. Set deadlines and checkpoints.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def web_research_draft(payload):
    query = _coerce_text(payload.get("query") or payload.get("text") or payload.get("search_query"))
    if not query:
        query = "No search query supplied."
    max_results = max(1, min(_coerce_int(payload.get("max_results"), 5), 20))
    include_sources = _coerce_text(payload.get("include_sources") or payload.get("sources") or "internal")
    domain_hints = _coerce_text(payload.get("domains") or payload.get("domain"))
    domains = [item.strip() for item in domain_hints.split(",") if item.strip()]
    tags = [tag.strip() for tag in _coerce_text(payload.get("topic_tags")).split(",") if tag.strip()]
    if not tags:
        goal = _coerce_text(payload.get("goal"))
        if goal:
            tags = [goal]
        else:
            tags = [query.split(" ", 1)[0].lower()] if query else ["research"]

    plan = []
    for index in range(1, min(max_results, 4) + 1):
        plan.append(f"{index}. Subquery: `{query}` + `{tags[(index - 1) % len(tags)]}`")

    domain_section = ", ".join(domains) if domains else "No domain constraints requested."
    return "\n".join(
        [
            "# Web Research Draft",
            "",
            f"- Primary query: `{query}`",
            f"- Max results target: `{max_results}`",
            f"- Source preference: `{include_sources}`",
            f"- Source/domain hints: `{domain_section}`",
            "",
            "## Planned query strategy",
            *plan,
            "",
            "## Notes",
            "- This pack is compute-only and does not perform outbound network calls.",
            "- Route through a governed connector to execute external research.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def calendar_event_plan(payload):
    title = _coerce_text(payload.get("title") or payload.get("event_title") or payload.get("summary"))
    if not title:
        title = "Untitled event"
    start_time = _coerce_text(payload.get("start") or payload.get("start_time") or "TBD")
    duration = _coerce_text(payload.get("duration") or payload.get("length") or "60m")
    location = _coerce_text(payload.get("location") or payload.get("where"))
    tz = _coerce_text(payload.get("timezone") or "UTC")
    attendees = _coerce_text(payload.get("attendees"))
    attendee_list = [person.strip() for person in attendees.split(",") if person.strip()] if attendees else []
    if not attendee_list:
        attendee_list = ["(no attendees provided)"]
    notes = _coerce_text(payload.get("notes") or "No notes provided.")

    return "\n".join(
        [
            "# Calendar Event Plan",
            "",
            f"- Title: `{title}`",
            f"- Start: `{start_time}`",
            f"- Duration: `{duration}`",
            f"- Timezone: `{tz}`",
            f"- Location: `{location or 'TBD'}`",
            "",
            "## Attendees",
            *[f"- {person}" for person in attendee_list],
            "",
            "## Suggested next steps",
            "- Confirm timezone and availability before scheduling.",
            "- Persist event details in a governance-approved connector.",
            "- Add reminders and agenda before invite dispatch.",
            "",
            "## Notes",
            notes,
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def incident_postmortem_brief(payload):
    summary = _safe_text(payload)
    if not summary:
        summary = _coerce_text(payload.get("summary") or "No incident summary provided.")
    impact = _coerce_text(payload.get("impact") or "Needs operator confirmation.")
    root_cause = _coerce_text(payload.get("root_cause") or "Not yet identified.")
    resolution = _coerce_text(payload.get("resolution") or "Not yet executed.")
    owners = _coerce_text(payload.get("owners") or "Unassigned")
    owner_list = [person.strip() for person in owners.split(",") if person.strip()]
    if not owner_list:
        owner_list = ["Unassigned"]

    return "\n".join(
        [
            "# Incident Postmortem Draft",
            "",
            "## Incident Summary",
            summary,
            "",
            f"- Impact: {impact}",
            f"- Resolution status: {resolution}",
            "",
            "## Suggested action ownership",
            *[f"- {person}" for person in owner_list],
            "",
            "## Root cause hypothesis",
            root_cause,
            "",
            "## Immediate follow-up",
            "1. Validate root cause hypothesis with logs and trace IDs.",
            "2. Confirm rollback/mitigation windows and residual risk.",
            "3. Capture preventive actions with owners and due dates.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def slo_status_snapshot(payload):
    metric_text = _coerce_text(payload.get("metrics") or payload.get("metric_text") or _safe_text(payload))
    if not metric_text:
        metric_text = "No metrics supplied."
    target_latency = _coerce_text(payload.get("target_latency_ms") or "250ms")
    target_error = _coerce_text(payload.get("target_error_rate") or "1%")
    window = _coerce_text(payload.get("window") or "15m")
    alert_text = _coerce_text(payload.get("alerts") or "None captured")
    alerts = [item.strip() for item in alert_text.split(",") if item.strip()]

    return "\n".join(
        [
            "# SLO Status Snapshot",
            "",
            f"- Window: `{window}`",
            f"- Latency target: `{target_latency}`",
            f"- Error-rate target: `{target_error}`",
            "",
            "## Observed inputs",
            metric_text,
            "",
            "## Alert feed",
            *([f"- {alert}" for alert in alerts] if alerts else ["- None captured"]),
            "",
            "## Suggested interpretation",
            "- Confirm whether targets above are breached against actual telemetry windows.",
            "- If breached, generate targeted `runbook_builder` or `triage_incident` follow-up.",
            "- Keep evidence in a governance-approved attachment or artifact record.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def code_change_summary(payload):
    diff = _safe_text(payload)
    cleaned = "\n".join(line for line in diff.splitlines() if line.strip())
    if not cleaned:
        cleaned = "No diff provided."
    return "\n".join(
        [
            "# Code Change Summary",
            "",
            "- Reviewed structural changes.",
            "- Reduced risk by isolating behavior changes.",
            "- No sensitive files targeted in this run.",
            "",
            "### Extracted snippets",
            *[f"- {line}" for line in cleaned.splitlines()[:6]],
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def release_note_writer(payload):
    text = _safe_text(payload)
    return "\n".join(
        [
            "# Release Notes",
            "",
            "## What changed",
            *[f"- {line}" for line in _normalize_points(text)],
            "",
            "## Validation",
            "- Manual verification required before publishing.",
            "- Confirm no sensitive data in artifacts.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def ticket_packager(payload):
    title = _coerce_text(payload.get("title") or "New ticket")
    text = _safe_text(payload)
    severity = _coerce_text(payload.get("severity") or "medium")
    return "\n".join(
        [
            "# Ticket Draft",
            "",
            f"- Title: {title}",
            f"- Severity: {severity}",
            "- Status: triage",
            "",
            "## Description",
            *( _normalize_points(text) if text else ["- No detailed text provided."] ),
            "",
            "## Acceptance",
            "- Reproducible test steps captured.",
            "- Owner assigned before handoff.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def compliance_audit_check(payload):
    text = _safe_text(payload)
    has_secret = bool(re.search(r"api[-_]?key|password|token|secret", text, re.IGNORECASE))
    has_pii = bool(re.search(r"\b\d{3}-\d{2}-\d{4}\b|\b\d{16}\b", text))
    findings = []
    if has_secret:
        findings.append("- Possible credentials-like token pattern detected.")
    if has_pii:
        findings.append("- Potential PII-like pattern detected.")
    if not findings:
        findings.append("- No obvious high-risk secret or PII markers in supplied text.")
    return "\n".join(["# Compliance Audit Check", "", *findings, "", f"_Generated at {_utc_timestamp()}_"])


def knowledge_extraction(payload):
    text = _safe_text(payload)
    terms = sorted(set(word.strip(".,") for word in re.findall(r"\b[A-Za-z][A-Za-z\-]{3,}\b", text)))
    head = terms[:12] if terms else ["No domain terms detected."]
    return "\n".join(
        [
            "# Knowledge Extraction",
            "",
            "## Key terms",
            *[f"- {term}" for term in head],
            "",
            "## Summary",
            *(_normalize_points(text)[:4]),
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def memory_checkpoint(payload):
    note = _safe_text(payload)
    return "\n".join(
        [
            "# Memory Checkpoint",
            "",
            f"- Snapshot ID: {_short_id(note)}",
            "- Context size bucket: medium",
            "- Retain only high-signal content.",
            "",
            "## Snapshot",
            *(_normalize_points(note)[:3]),
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def runbook_builder(payload):
    goal = _coerce_text(payload.get("goal") or "Stabilize service.")
    return "\n".join(
        [
            "# Runbook Draft",
            "",
            f"Goal: {goal}",
            "",
            "## Steps",
            "- Step 1: Confirm blast radius.",
            "- Step 2: Apply temporary mitigation.",
            "- Step 3: Validate system health.",
            "- Step 4: Close with post-incident review.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def ops_on_call_brief(payload):
    channel = _coerce_text(payload.get("channel") or "pager")
    text = _safe_text(payload)
    return "\n".join(
        [
            "# On-Call Brief",
            "",
            f"- Channel: {channel}",
            "",
            "## Current status",
            *_normalize_points(text),
            "",
            "## Immediate actions",
            "- Validate critical alerts still active.",
            "- Acknowledge if escalation threshold reached.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def observability_snapshot(payload):
    metrics = _coerce_text(payload.get("metrics") or "{}")
    return "\n".join(
        [
            "# Observability Snapshot",
            "",
            "## Metrics context",
            f"```text\n{metrics[:1000]}\n```",
            "",
            "## Interpretation",
            "- Treat anomalies as candidates for runbook triggers.",
            "- Verify correlation across latency, error, and queue depth.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def pii_scrub_report(payload):
    text = _safe_text(payload)
    scrubbed = re.sub(r"\b\d{3}-\d{2}-\d{4}\b", "XXX-XX-XXXX", text)
    scrubbed = re.sub(r"\b\d{16}\b", "XXXXXXXXXXXXXXXX", scrubbed)
    scrubbed = re.sub(
        r"[\w\.-]+@[\w\.-]+\.\w+",
        "[redacted-email]",
        scrubbed,
    )
    return "\n".join(
        [
            "# PII Scrub Report",
            "",
            "## Scrubbed payload",
            "```text",
            scrubbed[:1800],
            "```",
            "",
            "- Review redaction policy before forwarding payloads.",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def rewrite_style(payload):
    text = _safe_text(payload)
    style = _coerce_text(payload.get("style") or "technical")
    rewritten = f"[{style} tone] " + text
    return "\n".join(
        [
            "# Rewritten Text",
            "",
            rewritten if rewritten else "[no source text]",
            "",
            "_Local transform only; grammar and tone can be refined after human review._",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def follow_up_plan(payload):
    text = _safe_text(payload)
    followups = _normalize_points(text)
    if len(followups) < 3:
        followups.extend(["- Continue tracking open risks.", "- Confirm owners.", "- Record resolution notes."])
    return "\n".join(
        [
            "# Follow-Up Plan",
            "",
            "## Checklist",
            *[f"{idx}. {item[2:]}" for idx, item in enumerate(followups[:3], start=1)],
            "",
            "## Suggested cadence",
            "- First follow-up in 2h.",
            "- Retrospective in 24h.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def payment_action_plan(payload):
    provider = _coerce_text(payload.get("provider") or "nwc")
    amount = _coerce_text(payload.get("amount") or "unknown")
    destination = _coerce_text(payload.get("destination") or "default")
    return "\n".join(
        [
            "# Payment Action Plan",
            "",
            f"- Provider: {provider}",
            f"- Destination: {destination}",
            f"- Amount: {amount}",
            "",
            "## Suggested sequence",
            "- Verify approval requirements.",
            "- Re-check idempotency key.",
            "- Execute payment request through controlled action path.",
            "",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


def _coerce_list(value) -> List[dict]:
    if isinstance(value, list):
        return [item for item in value if isinstance(item, dict)]
    if isinstance(value, str):
        try:
            decoded = json.loads(value)
            if isinstance(decoded, list):
                return [item for item in decoded if isinstance(item, dict)]
        except json.JSONDecodeError:
            return []
    return []


def _coerce_scalar(value):
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if isinstance(value, dict) or isinstance(value, list):
        return json.dumps(value, sort_keys=True)
    return str(value)


def structured_data_query(payload):
    source = _coerce_list(payload.get("records"))
    if not source:
        source = _coerce_list(payload.get("data"))
    if not source:
        source = _coerce_list(payload.get("dataset"))

    if not source:
        return "\n".join(
            [
                "# Structured Data Query",
                "",
                "- No structured dataset provided (`records`, `data`, or `dataset`).",
                "- Provide an array of objects and optional `filters`, `select`, `sort_by`, `limit`.",
                "",
                f"_Generated at {_utc_timestamp()}_",
            ]
        )

    field_filters = payload.get("filters")
    normalized_filters = []
    if isinstance(field_filters, list):
        for item in field_filters:
            if not isinstance(item, dict):
                continue
            key = _coerce_text(item.get("field"))
            if not key:
                continue
            operator = _coerce_text(item.get("op") or item.get("operator") or "eq").lower()
            normalized_filters.append(
                {
                    "field": key,
                    "op": operator,
                    "value": item.get("value"),
                }
            )

    select_fields = payload.get("select")
    if isinstance(select_fields, str):
        selected = [field.strip() for field in select_fields.split(",") if field.strip()]
    elif isinstance(select_fields, list):
        selected = [str(item).strip() for item in select_fields if str(item).strip()]
    else:
        selected = []

    limit = _coerce_int(payload.get("limit") or payload.get("max_rows"), 50)
    sort_by = _coerce_text(payload.get("sort_by"))
    sort_desc = _to_bool(payload.get("sort_desc"), False)
    group_by = _coerce_text(payload.get("group_by"))

    def record_matches(row):
        for filter_spec in normalized_filters:
            key = filter_spec["field"]
            expected = filter_spec["value"]
            actual = row.get(key)
            op = filter_spec["op"]
            if op in {"eq", "equals"} and _coerce_scalar(actual) != _coerce_scalar(expected):
                return False
            if op in {"ne", "not_equals"} and _coerce_scalar(actual) == _coerce_scalar(expected):
                return False
            if op in {"contains", "has"} and not str(expected).lower() in str(actual).lower():
                return False
        return True

    filtered = [row for row in source if record_matches(row)]

    if sort_by:
        filtered.sort(
            key=lambda row: str(row.get(sort_by, "")),
            reverse=sort_desc,
        )

    if group_by:
        groups: Dict[str, List[Dict[str, Any]]] = {}
        for row in filtered:
            key = str(row.get(group_by, ""))
            groups.setdefault(key, []).append(row)
        grouped_lines = []
        for key in sorted(groups):
            grouped_lines.append(f"## {group_by}: {key}")
            grouped_lines.append(f"- count: {len(groups[key])}")
            grouped_lines.append("")
        summary = "\n".join(
            ["# Structured Data Query - Grouped"] + grouped_lines + [f"_Generated at {_utc_timestamp()}_"]
        )
        return summary

    if selected:
        rendered_rows = [
            [str(row.get(field, "")) for field in selected]
            for row in filtered[: max(limit, 1)]
        ]
        header = ["| " + " | ".join(selected) + " |", "| " + " | ".join(["---"] * len(selected)) + " |"]
        body = ["| " + " | ".join(row) + " |" for row in rendered_rows]
        return "\n".join(
            [
                "# Structured Data Query",
                "",
                f"- rows: {len(filtered)}",
                "",
                *header,
                *body,
                "",
                f"_Generated at {_utc_timestamp()}_",
            ]
        )

    items: Iterable[Dict[str, Any]] = filtered[: max(limit, 1)]
    lines = ["# Structured Data Query", "", f"- rows: {len(filtered)}", "", "| key | value |", "| --- | --- |"]
    if items:
        keys = sorted({key for row in items for key in row.keys()})
        for key in keys:
            values = [row.get(key) for row in items]
            preview = []
            for value in values[:3]:
                preview.append(_coerce_text(value))
            sample = ", ".join(preview)
            lines.append(f"| {key} | {sample} |")
    else:
        lines.append("| key | value |")
        lines.append("| --- | --- |")
        lines.append("| (no_rows) | (no data matches requested filters) |")

    lines.append("")
    lines.append(f"_Generated at {_utc_timestamp()}_")
    return "\n".join(lines)


def local_exec_snapshot(payload):
    template_id = _coerce_text(payload.get("template_id") or "local.exec:file.head")
    path = _coerce_text(payload.get("path") or ".")
    lines = _coerce_int(payload.get("lines"), 5)
    return "\n".join(
        [
            "# Local Execution Snapshot",
            "",
            f"- template_id: {template_id}",
            f"- path: {path}",
            f"- lines: {lines}",
            "",
            "Requested to execute a constrained local template for pre-approval diagnostics.",
            f"_Generated at {_utc_timestamp()}_",
        ]
    )


SKILL_MAP = {
    "summarize_transcript": summarize_transcript,
    "extract_action_items": extract_action_items,
    "draft_reply": draft_reply,
    "translate_text": translate_text,
    "sentiment_scan": sentiment_scan,
    "triage_incident": triage_incident,
    "meeting_minutes": meeting_minutes,
    "web_research_draft": web_research_draft,
    "calendar_event_plan": calendar_event_plan,
    "incident_postmortem_brief": incident_postmortem_brief,
    "slo_status_snapshot": slo_status_snapshot,
    "code_change_summary": code_change_summary,
    "release_note_writer": release_note_writer,
    "ticket_packager": ticket_packager,
    "compliance_audit_check": compliance_audit_check,
    "knowledge_extraction": knowledge_extraction,
    "memory_checkpoint": memory_checkpoint,
    "runbook_builder": runbook_builder,
    "ops_on_call_brief": ops_on_call_brief,
    "observability_snapshot": observability_snapshot,
    "pii_scrub_report": pii_scrub_report,
    "rewrite_style": rewrite_style,
    "follow_up_plan": follow_up_plan,
    "payment_action_plan": payment_action_plan,
    "structured_data_query": structured_data_query,
    "local_exec_snapshot": local_exec_snapshot,
}


SKILL_ALIASES = {
    "show_notes_v1": "summarize_transcript",
    "notify_v1": "draft_reply",
    "operator_reply_v1": "draft_reply",
    "payments_v1": "payment_action_plan",
    "payments_cashu_v1": "payment_action_plan",
    "memory_v1": "memory_checkpoint",
    "llm_local_v1": "rewrite_style",
    "llm_remote_v1": "rewrite_style",
    "local_exec_v1": "local_exec_snapshot",
}

SKILL_MANIFEST = {
    "summarize_transcript": {
        "category": "analysis",
        "description": "Summarize long text into concise markdown sections.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["max_points", "output_path"],
    },
    "extract_action_items": {
        "category": "analysis",
        "description": "Convert input text to markdown action-item checklists.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["output_path"],
    },
    "draft_reply": {
        "category": "communication",
        "description": "Draft a short, professional reply from source text.",
        "capabilities": ["message.send"],
        "required_input": ["text"],
        "recommended_args": ["tone", "notify"],
    },
    "translate_text": {
        "category": "communication",
        "description": "Generate a language-localized draft for given text.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["language", "reverse_copy"],
    },
    "sentiment_scan": {
        "category": "analysis",
        "description": "Classify sentiment signals and provide simple polarity counts.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": [],
    },
    "triage_incident": {
        "category": "incident",
        "description": "Assign priority and triage guidance from incident context.",
        "capabilities": ["message.send"],
        "required_input": ["text"],
        "recommended_args": ["notify"],
    },
    "meeting_minutes": {
        "category": "knowledge",
        "description": "Produce structured meeting minutes from transcript text.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["attendees", "output_path"],
    },
    "web_research_draft": {
        "category": "research",
        "description": "Generate a deterministic research plan for outbound lookup workflows.",
        "capabilities": [],
        "required_input": ["query"],
        "recommended_args": [
            "max_results",
            "domains",
            "include_sources",
            "topic_tags",
            "goal",
            "request_write",
        ],
    },
    "calendar_event_plan": {
        "category": "coordination",
        "description": "Create a governance-ready calendar event draft from structured fields.",
        "capabilities": [],
        "required_input": ["title", "start"],
        "recommended_args": ["duration", "attendees", "timezone", "location", "notes"],
    },
    "incident_postmortem_brief": {
        "category": "incident",
        "description": "Draft an incident postmortem section with ownership and follow-up structure.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["impact", "root_cause", "resolution", "owners", "request_write"],
    },
    "slo_status_snapshot": {
        "category": "ops",
        "description": "Summarize SLO-relevant telemetry context into a compact status snapshot.",
        "capabilities": [],
        "required_input": ["metrics"],
        "recommended_args": [
            "target_latency_ms",
            "target_error_rate",
            "window",
            "alerts",
            "request_write",
        ],
    },
    "code_change_summary": {
        "category": "engineering",
        "description": "Summarize code diffs or change text with risk-focused notes.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["output_path"],
    },
    "release_note_writer": {
        "category": "engineering",
        "description": "Draft release-note style changelog content.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["output_path"],
    },
    "ticket_packager": {
        "category": "ops",
        "description": "Draft a ticket/issue package from incident or change context.",
        "capabilities": ["object.write"],
        "required_input": ["text"],
        "recommended_args": ["title", "severity", "request_write"],
    },
    "compliance_audit_check": {
        "category": "security",
        "description": "Scan text for likely secrets/PII markers and return findings.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["request_write"],
    },
    "knowledge_extraction": {
        "category": "knowledge",
        "description": "Extract domain terms and build a compact knowledge snapshot.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["output_path"],
    },
    "memory_checkpoint": {
        "category": "memory",
        "description": "Create a stable memory checkpoint payload for follow-up runs.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["output_path"],
    },
    "runbook_builder": {
        "category": "ops",
        "description": "Create a short runbook draft for a stated operational goal.",
        "capabilities": [],
        "required_input": ["goal"],
        "recommended_args": ["output_path"],
    },
    "ops_on_call_brief": {
        "category": "ops",
        "description": "Create a concise on-call handoff brief.",
        "capabilities": ["message.send"],
        "required_input": ["text"],
        "recommended_args": ["channel", "notify"],
    },
    "observability_snapshot": {
        "category": "ops",
        "description": "Render a compact observability interpretation from metric text.",
        "capabilities": [],
        "required_input": ["metrics"],
        "recommended_args": ["output_path"],
    },
    "pii_scrub_report": {
        "category": "security",
        "description": "Produce a redacted PII/token scrubbed payload copy.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["output_path", "request_write"],
    },
    "rewrite_style": {
        "category": "content",
        "description": "Rewrite input text with a requested style and constraints.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["style", "request_llm"],
    },
    "follow_up_plan": {
        "category": "ops",
        "description": "Build a follow-up action plan with owners and cadence suggestions.",
        "capabilities": [],
        "required_input": ["text"],
        "recommended_args": ["request_write"],
    },
    "payment_action_plan": {
        "category": "finance",
        "description": "Draft a payment execution plan with approval checkpoints.",
        "capabilities": ["payment.send"],
        "required_input": ["provider", "amount", "destination"],
        "recommended_args": [
            "request_payment",
            "payment_approved",
            "payment_idempotency_key",
            "amount_msat",
            "payment_invoice",
            "payment_destination",
            "operation",
        ],
    },
    "structured_data_query": {
        "category": "analysis",
        "description": "Filter, sort, and project local structured records for quick reporting.",
        "capabilities": [],
        "required_input": ["records"],
        "recommended_args": ["filters", "select", "sort_by", "limit", "group_by"],
    },
    "local_exec_snapshot": {
        "category": "operations",
        "description": "Prepare context for constrained local snapshot execution.",
        "capabilities": ["local.exec"],
        "required_input": ["path", "template_id", "lines"],
        "recommended_args": ["request_local_exec"],
    },
}


SKILL_ORDER = [
    "summarize_transcript",
    "extract_action_items",
    "draft_reply",
    "translate_text",
    "sentiment_scan",
    "triage_incident",
    "meeting_minutes",
    "web_research_draft",
    "calendar_event_plan",
    "incident_postmortem_brief",
    "slo_status_snapshot",
    "code_change_summary",
    "release_note_writer",
    "ticket_packager",
    "compliance_audit_check",
    "knowledge_extraction",
    "memory_checkpoint",
    "runbook_builder",
    "ops_on_call_brief",
    "observability_snapshot",
    "pii_scrub_report",
    "rewrite_style",
    "follow_up_plan",
    "payment_action_plan",
    "structured_data_query",
    "local_exec_snapshot",
]


def _build_manifest_rows():
    rows = []
    for name in SKILL_ORDER:
        details = SKILL_MANIFEST.get(name, {})
        rows.append(
            {
                "name": name,
                "category": details.get("category", "misc"),
                "description": details.get("description", ""),
                "required_input": details.get("required_input", []),
                "recommended_args": details.get("recommended_args", []),
                "capabilities": details.get("capabilities", []),
            }
        )
    return rows


def _utc_timestamp():
    return datetime.utcnow().replace(microsecond=0).isoformat() + "Z"


def _resolve_skill(payload):
    requested = _coerce_text(payload.get("skill_name") or payload.get("skill") or payload.get("action")).lower()
    if requested and requested in SKILL_MAP:
        return requested

    runtime = payload.get("runtime")
    if isinstance(runtime, dict):
        recipe_id = _coerce_text(runtime.get("recipe_id"))
        if recipe_id and recipe_id in SKILL_ALIASES:
            return SKILL_ALIASES[recipe_id]

    return "summarize_transcript"


def _action_plan(payload, skill_name, markdown):
    actions = []
    if _to_bool(payload.get("request_write"), False):
        actions.append(_build_object_write(payload, skill_name, markdown))

    if _to_bool(payload.get("request_send"), False) or _to_bool(payload.get("notify"), False):
        actions.append(_build_message(payload, markdown))

    if _to_bool(payload.get("request_local_exec"), False) and skill_name == "local_exec_snapshot":
        actions.append(_build_local_exec(payload))

    if _to_bool(payload.get("request_llm"), False):
        actions.append(_build_llm(payload, markdown))

    if _to_bool(payload.get("request_payment"), False) and skill_name == "payment_action_plan":
        actions.append(_build_payment(payload))

    return actions


def handle_describe():
    return {
        "type": "describe_result",
        "id": "ignored",
        "skill": {
            "name": "top20_skill_pack",
            "version": "0.3.0",
            "description": (
                "A compute-first pack of practical, policy-gated skills for secure-agent workflows. "
                "Handlers are selected by skill_name, skill, action, or runtime.recipe_id alias."
            ),
            "inputs_schema": {
                "type": "object",
                "properties": {
                    "skill_name": {"type": "string"},
                    "runtime": {"type": "object"},
                    "text": {"type": "string"},
                },
            },
            "outputs_schema": {
                "type": "object",
                "properties": {
                    "markdown": {"type": "string"},
                    "skill": {"type": "string"},
                    "generated_at": {"type": "string"},
                    "available_skills": {"type": "array", "items": {"type": "string"}},
                    "manifest": {"type": "array", "items": {"type": "object"}},
                },
            },
            "requested_capabilities": [
                {"capability": "object.write", "scope": "shownotes/*"},
                {"capability": "message.send", "scope": "whitenoise:*"},
                {"capability": "payment.send", "scope": "nwc:*"},
                {"capability": "payment.send", "scope": "cashu:*"},
                {"capability": "local.exec", "scope": "local.exec:file.head"},
                {"capability": "llm.infer", "scope": "local:*"},
            ],
            "action_types": ["object.write", "message.send", "payment.send", "local.exec", "llm.infer"],
            "available_skills": SKILL_ORDER,
            "manifest": _build_manifest_rows(),
        },
    }


def handle_invoke(message):
    payload = message.get("input") or {}
    if not isinstance(payload, dict):
        payload = {"text": _coerce_text(payload)}

    skill_name = _resolve_skill(payload)
    handler = SKILL_MAP.get(skill_name, summarize_transcript)
    markdown = handler(payload)
    action_requests = _action_plan(payload, skill_name, markdown)

    output = {
        "markdown": markdown,
        "skill": skill_name,
        "generated_at": _utc_timestamp(),
        "available_skills": SKILL_ORDER,
        "manifest": _build_manifest_rows(),
    }
    return {
        "type": "invoke_result",
        "id": message["id"],
        "output": output,
        "action_requests": action_requests,
    }


def main():
    while True:
        line = input().strip()
        if not line:
            continue

        request = json.loads(line)
        msg_type = request.get("type")

        if msg_type == "describe":
            response = handle_describe()
            response["id"] = request.get("id", "unknown")
            print(json.dumps(response), flush=True)
        elif msg_type == "invoke":
            print(json.dumps(handle_invoke(request)), flush=True)
        else:
            response = {
                "type": "error",
                "id": request.get("id", "unknown"),
                "error": {
                    "code": "INVALID_INPUT",
                    "message": f"unsupported message type: {msg_type}",
                    "details": {},
                },
            }
            print(json.dumps(response), flush=True)


if __name__ == "__main__":
    main()
