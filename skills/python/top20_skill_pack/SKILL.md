# top20_skill_pack

SecureAgnt skill pack that bundles multiple in-house compute skills under a single NDJSON
executable (`skills/python/top20_skill_pack/main.py`).

This is intentionally **not** an outbound-network tool. It stays within SecureAgnt defaults:
- no direct HTTP access from the skill process
- actions are only requested via standard action requests
- platform policy decides execution for any side effect

## Runtime contract

- Transport: NDJSON over `stdin/stdout` (one JSON object per line).
- Supported message types: `describe`, `invoke`, `describe_result`, `invoke_result`.
- Output contract:
  - `markdown` (string): deterministic, operator-readable output
  - `skill` (string): resolved skill implementation used
  - `generated_at` (ISO8601 UTC)

## Invoking a specific skill

- Set `skill_name` in payload to one of:
  - `summarize_transcript`
  - `extract_action_items`
  - `draft_reply`
  - `translate_text`
  - `sentiment_scan`
  - `triage_incident`
  - `meeting_minutes`
  - `code_change_summary`
  - `release_note_writer`
  - `ticket_packager`
  - `compliance_audit_check`
  - `knowledge_extraction`
  - `memory_checkpoint`
  - `runbook_builder`
  - `ops_on_call_brief`
  - `observability_snapshot`
- `incident_postmortem_brief`
- `slo_status_snapshot`
- `pii_scrub_report`
- `rewrite_style`
- `follow_up_plan`
- `payment_action_plan`
- `structured_data_query`
- `local_exec_snapshot`
- `web_research_draft`
- `calendar_event_plan`
- `risk_register_draft`
- `deployment_readiness_checklist`
- `policy_decision_record`
- `customer_impact_assessment`
- `rollback_strategy`
- `dependency_health_check`
- `sla_breach_timeline`
- `audit_finding_summary`
- `incident_comm_plan`
- `vendor_dependency_risk`
- `runbook_validation_checklist`
- `cost_estimate_summary`

- If `skill_name` is omitted, the pack resolves a handler from `runtime.recipe_id` when
  possible via `SKILL_ALIASES` (`show_notes_v1`, `notify_v1`, `payments_v1`, etc.).
- If no match is found, it falls back to `summarize_transcript`.

## Action request flags

Action requests are only included when flags are set:

- `request_write: true` -> `object.write`
- `request_send: true` or `notify: true` -> `message.send`
- `request_local_exec: true` (with `local_exec_snapshot`) -> `local.exec`
- `request_llm: true` -> `llm.infer`
- `request_payment: true` (with `payment_action_plan`) -> `payment.send`
- `request_query: true` -> no action request; query-only result output.
- `request_write`, `request_send`, `request_local_exec`, `request_llm`, `request_payment` continue to request their corresponding action types when supported by the selected skill.

## Suggested input fields

- `text`, `body`, `content`, or `input`: generic source text.
- `tone`: tone style for `draft_reply`.
- `language`: target language for `translate_text`.
- `output_path`: override `object.write` destination.
- `destination`: override `message.send` destination.
- `template_id`, `path`, `lines`: local exec arguments when requested.
- `destination`, `operation`, `amount_msat`, `invoice`, `payment_idempotency_key`, `payment_approved`: payment flow inputs.
- `records`, `filters`, `select`, `sort_by`, `sort_desc`, `limit`, `group_by`: structured query inputs.
- `query`, `max_results`, `domains`, `include_sources`, `topic_tags`: web research planning inputs.
- `title`, `start`, `duration`, `attendees`, `timezone`, `location`, `notes`: calendar planning inputs.
- `impact`, `root_cause`, `resolution`, `owners`: incident postmortem inputs.
- `metrics`, `target_latency_ms`, `target_error_rate`, `window`, `alerts`: SLO snapshot inputs.
- `impact`, `owner`, `likelihood`, `mitigation`: risk register inputs.
- `service`, `environment`, `checks`: deployment readiness inputs.
- `decision`, `rationale`, `alternatives`, `conclusion`: policy decision record inputs.
- `services`, `severity`, `region`, `estimate`: customer impact assessment inputs.
- `service`, `reason`, `rollback_type`, `recovery_window`: rollback strategy inputs.
- `services`, `checks`: dependency health check inputs.
- `service`, `window`: SLA breach timeline inputs.
- `class`: audit finding summary inputs.
- `service`, `audience`, `channel`, `tone`, `impact`, `next_action`: incident communication plan inputs.
- `vendors`, `threshold`, `fallback`: vendor dependency risk inputs.
- `runbook`, `checks`: runbook validation checklist inputs.
- `scope`, `estimated_cost`, `unit`, `assumptions`: cost estimate summary inputs.
- `skill_name`: force one handler.

## Safety notes

- Capabilities are still enforced by the platform:
  - `object.write` requires `shownotes/*` scope
  - `message.send` requires destination scope
  - `payment.send` requires `nwc:*` or `cashu:*` scope
  - `local.exec` requires template scope
  - `llm.infer` requires model/policy grants
- Keep payloads small and deterministic when using this pack in production paths.
- Use `WORKER_SKILL_ENV_ALLOWLIST` minimally; this pack runs with cleared env by default.
