# API (MVP)

All `/v1/*` endpoints require header `x-tenant-id`.
The console shell route (`GET /console`) does not require tenant headers, but its data requests do.

Optional run policy header:
- `x-user-role`: `owner` | `operator` | `viewer` (default: `owner`)

Trusted proxy auth hardening (optional):
- `API_TRUSTED_PROXY_AUTH_ENABLED=1` enforces trusted-proxy token validation on role-scoped API calls.
- Configure one of:
  - `API_TRUSTED_PROXY_SHARED_SECRET`
  - `API_TRUSTED_PROXY_SHARED_SECRET_REF`
- When enabled, requests that rely on role/user headers must include:
  - `x-auth-proxy-token`
- Missing/invalid token returns `401 UNAUTHORIZED`.

Optional API capacity guardrails:
- `API_TENANT_MAX_INFLIGHT_RUNS` (positive integer): if set, `POST /v1/runs` returns `429 TENANT_INFLIGHT_LIMITED` when tenant queued+running runs are at/above the limit.
- `API_TENANT_MAX_TRIGGERS` (positive integer): if set, trigger create endpoints return `429 TENANT_TRIGGER_LIMITED` when tenant trigger count is at/above the limit.
- `API_TENANT_MAX_MEMORY_RECORDS` (positive integer): if set, memory write endpoints return `429 TENANT_MEMORY_LIMITED` when active tenant memory rows are at/above the limit.

Agent-context API controls:
- `API_AGENT_CONTEXT_ROOT` (default `agent_context`)
- `API_AGENT_CONTEXT_REQUIRED_FILES` (CSV; default canonical file set)
- `API_AGENT_CONTEXT_MAX_FILE_BYTES` (default `65536`)
- `API_AGENT_CONTEXT_MAX_TOTAL_BYTES` (default `262144`)
- `API_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR` (default `8`)
- `API_AGENT_CONTEXT_MUTATION_ENABLED` (default `0`; must be `1` to enable context mutations)
- `API_AGENT_BOOTSTRAP_ENABLED` (default `1`; set `0` to disable bootstrap endpoints)

Trigger mutation note:
- `POST /v1/triggers`, `POST /v1/triggers/cron`, `POST /v1/triggers/webhook`,
  `PATCH /v1/triggers/{id}`, `POST /v1/triggers/{id}/enable`,
  `POST /v1/triggers/{id}/disable`, `POST /v1/triggers/{id}/fire`, and
  `POST /v1/triggers/{id}/events/{event_id}/replay` require role `owner` or `operator`.
- `viewer` receives `403 FORBIDDEN` for trigger mutation endpoints.
- When `x-user-role=operator` is used on trigger mutation endpoints, `x-user-id` is required:
  - create operations require operator `x-user-id` to match `triggered_by_user_id` (or set it implicitly)
  - update/enable/disable/fire operations allow only triggers owned by the same user id

Memory endpoint note:
- `POST /v1/memory/records` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on memory write endpoints.
- `GET /v1/memory/records` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on memory query endpoints.
- `POST /v1/memory/handoff-packets` is allowed for `owner` and `operator`.
- `GET /v1/memory/handoff-packets` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on handoff packet endpoints.
- `GET /v1/memory/retrieve` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on memory retrieval endpoints.
- `GET /v1/memory/compactions/stats` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on memory compaction stats endpoints.
- `POST /v1/memory/records/purge-expired` is allowed for `owner` only.
- `operator` and `viewer` receive `403 FORBIDDEN` on memory purge endpoints.
- `GET /v1/agents/{id}/context` is allowed for `owner` and `operator`.
- `GET /v1/agents/{id}/bootstrap` is allowed for `owner` and `operator`.
- `POST /v1/agents/{id}/heartbeat/compile` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on agent-context inspect/compile endpoints.
- `POST /v1/agents/{id}/bootstrap/complete` requires:
  - `API_AGENT_BOOTSTRAP_ENABLED=1`
  - `owner`
  - `x-user-id`
- `POST /v1/agents/{id}/context` (agent-context mutation) is disabled by default and requires:
  - `API_AGENT_CONTEXT_MUTATION_ENABLED=1`
  - `owner` for `USER.md`, `HEARTBEAT.md`, and `BOOTSTRAP.md`
  - `owner` or `operator` for `MEMORY.md`, `memory/*.md`, and `sessions/*.jsonl`
  - immutable files (`AGENTS.md`, `TOOLS.md`, `IDENTITY.md`, `SOUL.md`) are always denied

Usage query note:
- `GET /v1/usage/llm/tokens` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on usage query endpoints.
- `GET /v1/payments` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on payment ledger query endpoints.
- `GET /v1/payments/summary` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on payment summary query endpoints.
- `GET /v1/ops/summary` is allowed for `owner` and `operator`.
- `GET /v1/ops/llm-gateway` is allowed for `owner` and `operator`.
- `GET /v1/ops/action-latency` is allowed for `owner` and `operator`.
- `GET /v1/ops/action-latency-traces` is allowed for `owner` and `operator`.
- `GET /v1/ops/latency-histogram` is allowed for `owner` and `operator`.
- `GET /v1/ops/latency-traces` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on ops query endpoints.
- `GET /v1/audit/compliance` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on compliance audit query endpoints.
- `GET /v1/audit/compliance/policy` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on compliance policy query endpoints.
- `PUT /v1/audit/compliance/policy` is allowed for `owner` only.
- `operator` and `viewer` receive `403 FORBIDDEN` on compliance policy mutation endpoints.
- `GET /v1/audit/compliance/verify` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on compliance audit verification endpoints.
- `GET /v1/audit/compliance/export` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on compliance audit export endpoints.
- `GET /v1/audit/compliance/siem/export` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on SIEM export endpoints.
- `GET /v1/audit/compliance/siem/deliveries` is allowed for `owner` and `operator`.
- `GET /v1/audit/compliance/siem/deliveries/summary` is allowed for `owner` and `operator`.
- `GET /v1/audit/compliance/siem/deliveries/slo` is allowed for `owner` and `operator`.
- `GET /v1/audit/compliance/siem/deliveries/targets` is allowed for `owner` and `operator`.
- `GET /v1/audit/compliance/siem/deliveries/alerts` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on SIEM delivery query endpoints.
- `POST /v1/audit/compliance/siem/deliveries` is allowed for `owner` and `operator`.
- `POST /v1/audit/compliance/siem/deliveries/alerts/ack` is allowed for `owner` and `operator`.
  - requires `x-user-id` for accountability/audit binding
- `POST /v1/audit/compliance/siem/deliveries/{id}/replay` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on SIEM delivery mutation endpoints.
- `GET /v1/audit/compliance/replay-package` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on replay package endpoints.
- `POST /v1/audit/compliance/purge` is allowed for `owner` only.
- `operator` and `viewer` receive `403 FORBIDDEN` on compliance purge endpoints.

## GET /console
Serves the M11A web operations console shell from the API service.
- Console shell includes M11B RBAC panel-state handling:
  - `ROLE_FORBIDDEN` state when selected role cannot access a panel.
  - `FORBIDDEN` state when backing endpoint returns `403`.

Notes:
- Returns HTML (`text/html`).
- Read-only UI shell; data panels query existing `/v1/*` endpoints.
- Drill-down panels currently query:
  - `/v1/ops/llm-gateway`
  - `/v1/ops/latency-traces`
  - `/v1/ops/action-latency-traces`
  - `/v1/runs/:id`
  - `/v1/runs/:id/audit`
  - `/v1/payments`
  - `/v1/audit/compliance/siem/deliveries/alerts`
- Console computes threshold chips from fetched payloads (run failure/latency, token usage, payment failures, SIEM delivery rates).
- Console supports client-side JSON export actions:
  - full snapshot export
  - health summary export
- Console supports alert acknowledgment workflow:
  - `POST /v1/audit/compliance/siem/deliveries/alerts/ack`
  - sends `x-user-id` when provided in controls
- Console supports heartbeat materialization workflows:
  - `Preview Heartbeat Plan` and `Apply Heartbeat Plan`
  - `POST /v1/agents/{id}/heartbeat/materialize`
  - apply mode requires `x-user-id` and explicit approval confirmation
- Console control state persists client-side via local storage key `secureagnt_console_controls_v1`.
- Run behind your auth/TLS gateway in production.

## POST /v1/runs
Creates a queued run and appends `run.created` audit event.

Request:
```json
{
  "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "triggered_by_user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
  "recipe_id": "show_notes_v1",
  "input": { "transcript_path": "podcasts/ep245/transcript.txt" },
  "requested_capabilities": [
    { "capability": "object.read", "scope": "podcasts/*" }
  ]
}
```

Queue lane hint (optional):
- include `input.queue_class` (or `input.llm_queue_class`) as `interactive` or `batch`.
- worker claim order prioritizes `interactive`; aged `batch` runs are promoted to avoid starvation.

Response:
- `201 Created` when a new run is accepted.
- `200 OK` when the request collapses to an existing active queued/running run by semantic dedupe key and returns that run.
- Dedupe key is derived from the canonicalized values of:
  - `tenant_id`
  - `agent_id`
  - `triggered_by_user_id` (if provided)
  - `role_preset`
  - `recipe_id`
  - `input`
  - `requested_capabilities`

The dedupe payload is canonicalized before hashing by:
- sorting object keys recursively
- preserving array ordering
- excluding runtime-generated trace fields from the hash inputs

Response shape (`201 Created` / `200 OK`):
```json
{
  "id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "tenant_id": "single",
  "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "triggered_by_user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
  "recipe_id": "show_notes_v1",
  "status": "queued",
  "requested_capabilities": [
    { "capability": "object.read", "scope": "podcasts/*" }
  ],
  "granted_capabilities": [
    { "capability": "object.read", "scope": "podcasts/*" }
  ],
  "created_at": "2026-02-16T00:00:00Z",
  "started_at": null,
  "finished_at": null,
  "error_json": null,
  "attempts": 0,
  "lease_owner": null,
  "lease_expires_at": null
}
```

Capacity note:
- when `API_TENANT_MAX_INFLIGHT_RUNS` is configured, create-run requests may return `429` with error code `TENANT_INFLIGHT_LIMITED` if tenant queued/running run capacity is exhausted.

## GET /v1/runs/{run_id}
Returns run lifecycle status and lease metadata for the tenant.

Current behavior:
- `granted_capabilities` is policy-authoritative (not a mirror).
- API normalizes capability names and grants only allowlisted scope patterns.
- Disallowed capabilities/scopes are dropped from grants.
- For known recipes, API applies capability bundles and intersects requested capabilities with bundle scope.
  - If `requested_capabilities` is empty, the recipe bundle is granted by default.
  - If `requested_capabilities` is provided, only entries within the recipe bundle are granted.
- API applies a role preset to recipe bundles when `x-user-role` is set:
  - `owner`: full recipe bundle behavior
  - `operator`: strips `local.exec`
  - `viewer`: only `object.read` and local-route `llm.infer`
- MVP hard-denied from API grants: `http.request`, `db.query`.
- Payload limits are clamped to platform caps per capability.
- Payment capability support:
  - `payment.send` supports `nwc:*` and `cashu:*` scopes.
  - `payments_v1` grants `nwc:*`.
  - `payments_cashu_v1` grants `cashu:*`.
  - Cashu execution defaults to fail-closed; optional mock execution is available for local/dev validation (`PAYMENT_CASHU_MOCK_ENABLED=1`).
  - Recipe `payments_v1` grants `payment.send` by default.
- Memory capability support:
  - `memory.read` and `memory.write` are supported with `memory:*` scope.
  - Recipe `memory_v1` grants both memory capabilities by default.
- Messaging capability support:
  - `message.send` supports outbound provider+destination routing (`whitenoise:`, `slack:`)
  - `message.receive` supports inbound source capture under provider+source scope (`whitenoise:`, `slack:`)

## GET /v1/runs/{run_id}/audit
Returns ordered run audit events (`created_at`, then `id`), with optional query param:
- `limit` (default `200`, max `1000`)

## GET /v1/agents/{agent_id}/context
Returns the effective agent-context snapshot metadata for operator inspection without returning file contents.

Response includes:
- source directory resolution result
- per-file checksums/byte sizes
- mutability classification per file
- missing required files and loader warnings
- context aggregate checksum and canonical summary checksum
- precedence order used for deterministic conflict handling

## GET /v1/agents/{agent_id}/bootstrap
Returns bootstrap workflow state for `BOOTSTRAP.md`.

Behavior:
- requires `owner` or `operator` role.
- when `API_AGENT_BOOTSTRAP_ENABLED=0`, response is `enabled=false` and `status=disabled`.
- when enabled:
  - `status=pending` if `BOOTSTRAP.md` exists and no completion record is present.
  - `status=completed` if `sessions/bootstrap.status.jsonl` has a completed status event.
  - `status=not_configured` if no bootstrap file is present.

## POST /v1/agents/{agent_id}/bootstrap/complete
Records a bootstrap completion event and optionally writes initial profile files.

Request:
```json
{
  "identity_markdown": "# IDENTITY\nname: my-agent",
  "soul_markdown": "# SOUL\n...",
  "user_markdown": "# USER\n...",
  "heartbeat_markdown": "# HEARTBEAT\n...",
  "completion_note": "initial setup completed",
  "force": false
}
```

Behavior:
- requires `API_AGENT_BOOTSTRAP_ENABLED=1`.
- requires `owner` role and `x-user-id`.
- requires `BOOTSTRAP.md` to exist in the resolved agent context directory.
- writes any provided markdown payloads to:
  - `IDENTITY.md`
  - `SOUL.md`
  - `USER.md`
  - `HEARTBEAT.md`
- appends completion status event to:
  - `sessions/bootstrap.status.jsonl`
- if already completed, returns `409` unless `force=true`.

## POST /v1/agents/{agent_id}/heartbeat/compile
Compiles heartbeat intents into trigger candidates with no side effects.

Request (`heartbeat_markdown` optional):
```json
{
  "heartbeat_markdown": "- every 900 recipe=show_notes_v1 max_inflight=2 jitter=5"
}
```

Behavior:
- if `heartbeat_markdown` is omitted, API loads `HEARTBEAT.md` from agent context.
- validates cron expression/timezone and interval bounds.
- returns deterministic candidate and issue arrays.
- response includes context checksums when source is `HEARTBEAT.md`.

## POST /v1/agents/{agent_id}/heartbeat/materialize
Materializes heartbeat trigger candidates into governed trigger records (or returns a plan-only preview).

Request (`apply` defaults to `false`):
```json
{
  "apply": true,
  "approval_confirmed": true,
  "approval_note": "approved in change window",
  "cron_max_attempts": 3
}
```

Behavior:
- requires `owner` or `operator` role (`viewer` denied).
- when `apply=true`:
  - requires `approval_confirmed=true`
  - requires `x-user-id` for approval attribution
  - fails with `409` when heartbeat compile has unresolved issues
- compiles from inline markdown when provided; otherwise loads `HEARTBEAT.md`.
- creates interval/cron triggers using heartbeat candidate settings and recipe-managed capability grants.
- skips creating duplicates when a matching trigger already exists for the same tenant/agent schedule candidate.
- emits trigger audit provenance events (`trigger.materialized`) with source line and approval metadata.

## POST /v1/agents/{agent_id}/context
Mutates supported agent-context files when mutation endpoints are explicitly enabled.

Request:
```json
{
  "relative_path": "sessions/2026-02-20.jsonl",
  "content": "{\"event\":\"session.start\"}",
  "mode": "append"
}
```

Guardrails:
- disabled unless `API_AGENT_CONTEXT_MUTATION_ENABLED=1`.
- path must be relative and within supported context files.
- immutable files are always denied.
- `sessions/*.jsonl` supports `append` mode only.
- payload/write size is capped by `API_AGENT_CONTEXT_MAX_FILE_BYTES`.

## POST /v1/memory/records
Creates a tenant-scoped memory record.

Request:
```json
{
  "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "step_id": "56d5c5a8-8ebd-48b5-9823-a95a786f3f40",
  "memory_kind": "semantic",
  "scope": "memory:project/roadmap",
  "content_json": {"fact":"White Noise is first-class"},
  "summary_text": "messaging priority",
  "redaction_applied": true,
  "expires_at": "2026-03-01T00:00:00Z"
}
```

Validation:
- `memory_kind` must be one of `session|semantic|procedural|handoff`
- `scope` must be `memory:`-prefixed
- `run_id` and `step_id` are tenant-validated when present
- when `API_TENANT_MAX_MEMORY_RECORDS` is configured, writes may return `429 TENANT_MEMORY_LIMITED` when tenant active memory capacity is exhausted
- server applies redaction before persistence/indexing:
  - sensitive JSON keys are redacted
  - token-like secret strings are redacted in text fields
  - `redaction_applied` is set when redaction occurred (or caller explicitly set it)

## GET /v1/memory/records
Lists tenant-scoped memory records (latest first).

Query params:
- `limit` (optional, default `100`, min `1`, max `1000`)
- `agent_id` (optional UUID filter)
- `memory_kind` (optional exact filter)
- `scope_prefix` (optional prefix filter, must be memory-scoped)

## POST /v1/memory/handoff-packets
Creates a structured inter-agent handoff packet backed by a `memory_kind=handoff` memory record.

Request:
```json
{
  "to_agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "from_agent_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "step_id": "56d5c5a8-8ebd-48b5-9823-a95a786f3f40",
  "title": "handoff summary",
  "payload_json": {"next_action":"review and publish"},
  "expires_at": "2026-03-01T00:00:00Z"
}
```

Validation:
- `title` must not be empty
- `run_id` and `step_id` are tenant-validated when present
- when `API_TENANT_MAX_MEMORY_RECORDS` is configured, writes may return `429 TENANT_MEMORY_LIMITED` when tenant active memory capacity is exhausted
- packet payload/title are redacted before persistence/indexing when sensitive material is detected

## GET /v1/memory/handoff-packets
Lists tenant-scoped handoff packets (latest first).

Query params:
- `limit` (optional, default `100`, min `1`, max `1000`)
- `to_agent_id` (optional UUID filter)
- `from_agent_id` (optional UUID filter)

## GET /v1/memory/retrieve
Retrieves ranked memory context entries with citation metadata for deterministic prompt/context injection.

Query params:
- `limit` (optional, default `20`, min `1`, max `200`)
- `agent_id` (optional UUID filter)
- `memory_kind` (optional exact filter: `session|semantic|procedural|handoff`)
- `scope_prefix` (optional prefix filter, must be memory-scoped)
- `query_text` (optional free text used for token-overlap scoring)
- `min_score` (optional score floor, min `0.0`, max `2.0`)
- `source_prefix` (optional source prefix filter, for example `worker.`)
- `require_summary` (optional bool, default `false`; when true, excludes records without `summary_text`)

Ranking behavior:
- Each item includes a deterministic `score` in range `0.0..2.0`.
- Score combines recency bias, summary presence, and optional query token overlap.

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "limit": 20,
  "retrieved_count": 2,
  "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "memory_kind": "semantic",
  "scope_prefix": "memory:project",
  "query_text": "roadmap risk",
  "min_score": 0.5,
  "source_prefix": "worker.",
  "require_summary": true,
  "items": [
    {
      "rank": 1,
      "score": 1.24,
      "citation": {
        "memory_id": "6c81fcfd-c982-4e03-b40e-f13bc89cd412",
        "created_at": "2026-02-17T12:00:03Z",
        "source": "api",
        "memory_kind": "semantic",
        "scope": "memory:project/roadmap"
      },
      "content_json": {"note":"newer"},
      "summary_text": "newer"
    }
  ]
}
```

## GET /v1/memory/compactions/stats
Returns tenant-scoped memory compaction counters and pending backlog.

Query params:
- `window_secs` (optional, min `1`, max `31536000`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T11:00:00Z",
  "compacted_groups_window": 4,
  "compacted_source_records_window": 78,
  "pending_uncompacted_records": 12,
  "last_compacted_at": "2026-02-17T11:58:00Z"
}
```

## POST /v1/memory/records/purge-expired
Purges tenant memory rows with `expires_at <= as_of` (owner role only).
For runs impacted by purged rows, API appends `memory.purged` audit events to the run audit stream.

Request:
```json
{
  "as_of": "2026-02-17T12:00:00Z"
}
```

## GET /v1/audit/compliance
Returns tenant-scoped compliance-plane audit events (high-risk policy/funds/side-effect class routing).

Query params:
- `limit` (optional, default `200`, min `1`, max `1000`)
- `run_id` (optional UUID filter)
- `event_type` (optional exact event type filter)

Response (`200 OK`):
```json
[
  {
    "id": "6c81fcfd-c982-4e03-b40e-f13bc89cd412",
    "source_audit_event_id": "6114cc2a-115d-4bd3-bd70-d0af3083a2d2",
    "tamper_chain_seq": 19,
    "tamper_prev_hash": "5f4dcc3b5aa765d61d8327deb882cf99",
    "tamper_hash": "37b51d194a7513e45b56f6524f2d51f2",
    "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
    "step_id": "56d5c5a8-8ebd-48b5-9823-a95a786f3f40",
    "tenant_id": "single",
    "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
    "user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
    "actor": "worker",
    "event_type": "action.executed",
    "payload_json": {"action_type":"payment.send"},
    "request_id": "req-123",
    "session_id": "sess-abc",
    "action_request_id": "f34cf4a8-a2ff-47e8-9f5e-f84f62bb0420",
    "payment_request_id": "374cd9a7-b674-4929-869d-4879f9f894ca",
    "created_at": "2026-02-17T12:00:03Z",
    "recorded_at": "2026-02-17T12:00:03Z"
  }
]
```

Correlation note:
- `request_id` and `session_id` are optional transport/session correlation fields when present in routed event payloads.
- `action_request_id` and `payment_request_id` are optional UUID correlations extracted from routed payloads.

## GET /v1/audit/compliance/verify
Verifies the tenant compliance tamper-evidence chain and returns summary status.

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "checked_events": 1242,
  "verified": true,
  "first_invalid_event_id": null,
  "latest_chain_seq": 1242,
  "latest_tamper_hash": "b9f1cc4a0f5e8d58f1ba68cb7c7f56f8"
}
```

## GET /v1/audit/compliance/policy
Returns tenant compliance retention/legal-hold policy.

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "compliance_hot_retention_days": 180,
  "compliance_archive_retention_days": 2555,
  "legal_hold": false,
  "legal_hold_reason": null,
  "updated_at": null
}
```

## PUT /v1/audit/compliance/policy
Upserts tenant compliance retention/legal-hold policy (owner role only).

Request:
```json
{
  "compliance_hot_retention_days": 45,
  "compliance_archive_retention_days": 365,
  "legal_hold": true,
  "legal_hold_reason": "investigation-123"
}
```

Validation:
- `compliance_hot_retention_days > 0`
- `compliance_archive_retention_days > 0`
- `compliance_archive_retention_days >= compliance_hot_retention_days`

## POST /v1/audit/compliance/purge
Purges tenant compliance events older than policy `compliance_hot_retention_days` when legal hold is not active (owner role only).

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "deleted_count": 87,
  "legal_hold": false,
  "cutoff_at": "2026-01-01T00:00:00Z",
  "compliance_hot_retention_days": 45,
  "compliance_archive_retention_days": 365
}
```

## GET /v1/audit/compliance/export
Exports tenant-scoped compliance audit events as NDJSON for batch ingestion pipelines.

Query params:
- `limit` (optional, default `500`, min `1`, max `1000`)
- `run_id` (optional UUID filter)
- `event_type` (optional exact event type filter)

Response (`200 OK`):
- `Content-Type: application/x-ndjson`
- body: one JSON object per line, same core fields as `GET /v1/audit/compliance` (including tamper-evidence fields)

## GET /v1/audit/compliance/siem/export
Exports tenant-scoped compliance audit events in adapter-formatted NDJSON.

Query params:
- `limit` (optional, default `500`, min `1`, max `1000`)
- `run_id` (optional UUID filter)
- `event_type` (optional exact event type filter)
- `adapter` (optional, default `secureagnt_ndjson`):
  - `secureagnt_ndjson`
  - `splunk_hec`
  - `elastic_bulk`
- `elastic_index` (optional; used only with `adapter=elastic_bulk`, default `secureagnt-compliance-audit`)

Response (`200 OK`):
- `Content-Type: application/x-ndjson`
- body format by adapter:
  - `secureagnt_ndjson`: one SecureAgnt compliance event per line
  - `splunk_hec`: one HEC envelope per line
  - `elastic_bulk`: action/doc line pairs compatible with bulk ingestion

## GET /v1/audit/compliance/siem/deliveries
Lists tenant-scoped SIEM delivery outbox rows for delivery observability.

Query params:
- `limit` (optional, default `100`, min `1`, max `1000`)
- `run_id` (optional UUID filter)
- `status` (optional):
  - `pending`
  - `processing`
  - `failed`
  - `delivered`
  - `dead_lettered`

Response (`200 OK`):
```json
[
  {
    "id": "36556eca-8c6e-4d85-9d2b-0d2f5f2e05e8",
    "tenant_id": "single",
    "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
    "adapter": "splunk_hec",
    "delivery_target": "mock://success",
    "status": "pending",
    "attempts": 0,
    "max_attempts": 3,
    "next_attempt_at": "2026-02-17T12:00:00Z",
    "leased_by": null,
    "lease_expires_at": null,
    "last_error": null,
    "last_http_status": null,
    "created_at": "2026-02-17T12:00:00Z",
    "updated_at": "2026-02-17T12:00:00Z",
    "delivered_at": null
  }
]
```

## POST /v1/audit/compliance/siem/deliveries
Queues a tenant-scoped SIEM delivery outbox row for worker-side delivery processing.

Request:
```json
{
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "adapter": "splunk_hec",
  "delivery_target": "mock://success",
  "limit": 500,
  "max_attempts": 3
}
```

Validation:
- `delivery_target` must be non-empty.
- `adapter` is optional (defaults to `secureagnt_ndjson`).
- `max_attempts` is clamped to `1..20`.

Response (`202 Accepted`):
```json
{
  "id": "36556eca-8c6e-4d85-9d2b-0d2f5f2e05e8",
  "tenant_id": "single",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "adapter": "splunk_hec",
  "delivery_target": "mock://success",
  "status": "pending",
  "attempts": 0,
  "max_attempts": 3,
  "next_attempt_at": "2026-02-17T12:00:00Z",
  "created_at": "2026-02-17T12:00:00Z"
}
```

## GET /v1/audit/compliance/siem/deliveries/summary
Returns tenant-scoped SIEM delivery status counters.

Query params:
- `run_id` (optional UUID filter)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "pending_count": 2,
  "processing_count": 1,
  "failed_count": 0,
  "delivered_count": 18,
  "dead_lettered_count": 1,
  "oldest_pending_age_seconds": 42.7
}
```

## GET /v1/audit/compliance/siem/deliveries/slo
Returns tenant-scoped SIEM delivery SLO counters and rate metrics over a rolling window.

Query params:
- `run_id` (optional UUID filter)
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "window_secs": 3600,
  "since": "2026-02-17T11:00:00Z",
  "total_count": 42,
  "pending_count": 2,
  "processing_count": 1,
  "failed_count": 3,
  "delivered_count": 34,
  "dead_lettered_count": 2,
  "delivery_success_rate_pct": 80.95,
  "hard_failure_rate_pct": 11.9,
  "dead_letter_rate_pct": 4.76,
  "oldest_pending_age_seconds": 27.4
}
```

## GET /v1/audit/compliance/siem/deliveries/targets
Returns tenant-scoped SIEM delivery counters grouped by `delivery_target`.

Query params:
- `run_id` (optional UUID filter)
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)
- `limit` (optional, default `100`, min `1`, max `200`)

Response (`200 OK`):
```json
[
  {
    "delivery_target": "https://siem.example/hec",
    "total_count": 12,
    "pending_count": 1,
    "processing_count": 0,
    "failed_count": 2,
    "delivered_count": 8,
    "dead_lettered_count": 1,
    "last_error": "http status 503",
    "last_http_status": 503,
    "last_attempt_at": "2026-02-17T12:03:00Z"
  }
]
```

## GET /v1/audit/compliance/siem/deliveries/alerts
Returns threshold-based SIEM delivery alert rows derived from per-target counters.

Query params:
- `run_id` (optional UUID filter)
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)
- `limit` (optional, default `100`, min `1`, max `200`)
- `max_hard_failure_rate_pct` (optional, default `0`, min `0`, max `100`)
- `max_dead_letter_rate_pct` (optional, default `0`, min `0`, max `100`)
- `max_pending_count` (optional, default `0`, min `0`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "window_secs": 3600,
  "since": "2026-02-17T11:00:00Z",
  "thresholds": {
    "max_hard_failure_rate_pct": 10.0,
    "max_dead_letter_rate_pct": 5.0,
    "max_pending_count": 0
  },
  "alerts": [
    {
      "delivery_target": "https://siem-a.example/hec",
      "total_count": 3,
      "pending_count": 1,
      "processing_count": 0,
      "failed_count": 1,
      "delivered_count": 0,
      "dead_lettered_count": 1,
      "hard_failure_rate_pct": 66.67,
      "dead_letter_rate_pct": 33.33,
      "triggered_rules": [
        "pending_count 1 > 0",
        "hard_failure_rate_pct 66.67 > 10.00",
        "dead_letter_rate_pct 33.33 > 5.00"
      ],
      "severity": "critical",
      "last_error": "dead letter",
      "last_http_status": 500,
      "last_attempt_at": "2026-02-17T12:03:00Z",
      "acknowledged": true,
      "acknowledged_at": "2026-02-17T12:05:00Z",
      "acknowledged_by_user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
      "acknowledged_by_role": "operator",
      "acknowledgement_note": "mitigation applied"
    }
  ]
}
```

## POST /v1/audit/compliance/siem/deliveries/alerts/ack
Acknowledges an active SIEM delivery alert target for the tenant.

Headers:
- `x-user-id` is required (`owner`/`operator`) and must be a UUID.

Request:
```json
{
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "delivery_target": "https://siem-a.example/hec",
  "note": "mitigation applied"
}
```

Notes:
- `run_id` is optional; when omitted, acknowledgment applies to global scope (`*`) for that tenant + target.
- `note` is optional and limited to 2000 characters.

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "run_scope": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "delivery_target": "https://siem-a.example/hec",
  "acknowledged_by_user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
  "acknowledged_by_role": "operator",
  "acknowledgement_note": "mitigation applied",
  "created_at": "2026-02-17T12:00:00Z",
  "acknowledged_at": "2026-02-17T12:05:00Z"
}
```

## POST /v1/audit/compliance/siem/deliveries/{id}/replay
Requeues a tenant-scoped dead-letter SIEM delivery row back to `pending`.

Path params:
- `id` (required UUID of `compliance_siem_delivery_outbox` row)

Request:
```json
{
  "delay_secs": 15
}
```

Validation:
- row must exist for tenant and currently be `dead_lettered`
- `delay_secs` is optional and clamped to `0..86400`

Response (`202 Accepted`):
```json
{
  "id": "36556eca-8c6e-4d85-9d2b-0d2f5f2e05e8",
  "tenant_id": "single",
  "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
  "adapter": "splunk_hec",
  "delivery_target": "https://siem.example/hec",
  "status": "pending",
  "attempts": 0,
  "max_attempts": 3,
  "next_attempt_at": "2026-02-17T12:00:15Z",
  "leased_by": null,
  "lease_expires_at": null,
  "last_error": null,
  "last_http_status": null,
  "created_at": "2026-02-17T12:00:00Z",
  "updated_at": "2026-02-17T12:00:00Z",
  "delivered_at": null
}
```

## GET /v1/audit/compliance/replay-package
Builds a deterministic incident replay package for a single run.

Query params:
- `run_id` (required UUID)
- `audit_limit` (optional, default `2000`, min `1`, max `5000`)
- `compliance_limit` (optional, default `2000`, min `1`, max `5000`)
- `payment_limit` (optional, default `500`, min `1`, max `2000`)
- `include_payments` (optional, default `true`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "run": {"id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65"},
  "generated_at": "2026-02-17T12:00:00Z",
  "run_audit_events": [],
  "compliance_audit_events": [],
  "payment_ledger": [],
  "correlation": {
    "run_audit_event_count": 0,
    "compliance_event_count": 0,
    "payment_event_count": 0,
    "first_event_at": null,
    "last_event_at": null
  },
  "manifest": {
    "version": "v1",
    "digest_sha256": "e1df2fcf4f89f0f70f10db6b6f4ebf3f48d39d7906d44af6018ee1d2f8f16b75",
    "signing_mode": "unsigned",
    "signature": null
  }
}
```

## GET /v1/ops/summary
Returns tenant-scoped operational summary counters and run-duration telemetry for a rolling window.

Query params:
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "queued_runs": 4,
  "running_runs": 1,
  "succeeded_runs_window": 22,
  "failed_runs_window": 3,
  "dead_letter_trigger_events_window": 0,
  "avg_run_duration_ms": 842.3,
  "p95_run_duration_ms": 1960.1
}
```

## GET /v1/ops/llm-gateway
Returns tenant-scoped LLM gateway lane aggregates for a rolling window.

Query params:
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "total_count": 42,
  "lanes": [
    {
      "request_class": "interactive",
      "total_count": 30,
      "avg_duration_ms": 520.3,
      "p95_duration_ms": 1420.0,
      "cache_hit_count": 12,
      "distributed_cache_hit_count": 4,
      "cache_hit_rate_pct": 40.0,
      "verifier_escalated_count": 3,
      "verifier_escalated_rate_pct": 10.0,
      "slo_warn_count": 5,
      "slo_breach_count": 1,
      "distributed_fail_open_count": 0
    }
  ]
}
```

## GET /v1/ops/action-latency
Returns tenant-scoped action-latency aggregates for a rolling window.

Query params:
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "actions": [
    {
      "action_type": "payment.send",
      "total_count": 12,
      "avg_duration_ms": 410.7,
      "p95_duration_ms": 980.2,
      "max_duration_ms": 1320,
      "failed_count": 2,
      "denied_count": 1
    }
  ]
}
```

## GET /v1/ops/action-latency-traces
Returns tenant-scoped per-action latency traces for rolling-window regression analysis.

Query params:
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)
- `limit` (optional, default `500`, min `1`, max `5000`)
- `action_type` (optional exact action filter, for example `payment.send`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "limit": 2,
  "action_type": "payment.send",
  "traces": [
    {
      "action_request_id": "9fbc39f8-7bb2-4bb2-bde8-df2f6d7c2fb8",
      "run_id": "14f6d2f4-5f8c-4e1e-9ae4-2c7edbba1a4e",
      "step_id": "3f11f733-f8fc-4f95-b0c8-2f58f03fa5d4",
      "action_type": "payment.send",
      "status": "failed",
      "duration_ms": 3250,
      "created_at": "2026-02-17T12:54:00Z",
      "executed_at": "2026-02-17T12:54:03.250Z"
    }
  ]
}
```

## GET /v1/ops/latency-histogram
Returns tenant-scoped run-duration bucket counts for a rolling window.

Query params:
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "buckets": [
    {
      "bucket_label": "0-499ms",
      "lower_bound_ms": 0,
      "upper_bound_exclusive_ms": 500,
      "run_count": 12
    },
    {
      "bucket_label": "10000ms+",
      "lower_bound_ms": 10000,
      "upper_bound_exclusive_ms": null,
      "run_count": 1
    }
  ]
}
```

## GET /v1/ops/latency-traces
Returns tenant-scoped per-run latency traces for rolling-window regression analysis.

Query params:
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)
- `limit` (optional, default `500`, min `1`, max `5000`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "limit": 3,
  "traces": [
    {
      "run_id": "14f6d2f4-5f8c-4e1e-9ae4-2c7edbba1a4e",
      "status": "succeeded",
      "duration_ms": 410,
      "started_at": "2026-02-17T12:58:00Z",
      "finished_at": "2026-02-17T12:58:00.410Z"
    },
    {
      "run_id": "9fbc39f8-7bb2-4bb2-bde8-df2f6d7c2fb8",
      "status": "failed",
      "duration_ms": 3250,
      "started_at": "2026-02-17T12:54:00Z",
      "finished_at": "2026-02-17T12:54:03.250Z"
    }
  ]
}
```

## GET /v1/usage/llm/tokens
Returns tenant-scoped remote LLM token/cost usage totals over a rolling window.

Query params:
- `window_secs` (optional, default `86400`, min `1`, max `31536000`)
- `agent_id` (optional UUID filter)
- `model_key` (optional string filter, e.g. `remote:gpt-4o-mini`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "tokens": 12345,
  "estimated_cost_usd": 0.42,
  "agent_id": null,
  "model_key": null
}
```

## GET /v1/payments
Returns tenant-scoped payment request ledger rows with latest known settlement result.

Query params:
- `limit` (optional, default `100`, min `1`, max `500`)
- `run_id` (optional UUID filter)
- `agent_id` (optional UUID filter)
- `status` (optional request status filter, for example `requested`, `executed`, `failed`, `duplicate`)
- `destination` (optional exact destination filter, for example `nwc:wallet-main`)
- `idempotency_key` (optional exact filter)

Response (`200 OK`):
```json
[
  {
    "id": "8df25cc0-c8c3-4c44-bff9-cf65cfc9ef31",
    "action_request_id": "a7ecf0d7-f4c9-4f0a-8f9b-f3f6cf5f82f4",
    "run_id": "0b26f2f3-8af7-435e-b6fe-e0324f7d4c65",
    "tenant_id": "single",
    "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
    "provider": "nwc",
    "operation": "pay_invoice",
    "destination": "nwc:wallet-main",
    "idempotency_key": "pay-001",
    "amount_msat": 2100,
    "status": "executed",
    "request_json": {"operation":"pay_invoice"},
    "latest_result_status": "executed",
    "latest_result_json": {"settlement_status":"settled"},
    "latest_error_json": null,
    "settlement_status": "settled",
    "settlement_rail": "nwc",
    "normalized_outcome": "executed",
    "normalized_error_code": null,
    "normalized_error_class": null,
    "created_at": "2026-02-17T12:00:00Z",
    "updated_at": "2026-02-17T12:00:03Z",
    "latest_result_created_at": "2026-02-17T12:00:03Z"
  }
]
```

Normalization notes:
- `normalized_outcome` is a reconciliation-friendly status (`requested`, `executed`, `failed`, `duplicate`, `unknown`).
- `normalized_error_class` maps known error-code families into stable categories (`budget_limit`, `approval_required`, `configuration`, `disabled`, `upstream_failure`, `unknown`).

## GET /v1/payments/summary
Returns tenant-scoped payment summary counters and executed spend totals for reconciliation dashboards.

Query params:
- `window_secs` (optional rolling window; min `1`, max `31536000`)
- `agent_id` (optional UUID filter)
- `operation` (optional; one of `pay_invoice`, `make_invoice`, `get_balance`)

Response (`200 OK`):
```json
{
  "tenant_id": "single",
  "window_secs": 3600,
  "since": "2026-02-17T12:00:00Z",
  "agent_id": null,
  "operation": null,
  "total_requests": 42,
  "requested_count": 3,
  "executed_count": 35,
  "failed_count": 2,
  "duplicate_count": 2,
  "executed_spend_msat": 125000
}
```

## POST /v1/triggers
Creates an enabled interval trigger that the worker scheduler can fire into runs.

Request:
```json
{
  "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "triggered_by_user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
  "recipe_id": "show_notes_v1",
  "input": { "transcript_path": "podcasts/ep245/transcript.txt" },
  "requested_capabilities": [],
  "interval_seconds": 60,
  "jitter_seconds": 15
}
```

Response (`201 Created`): includes trigger metadata (`trigger_type=interval`, `next_fire_at`, capability grants).

Notes:
- `interval_seconds` must be `> 0` and `<= 31536000`.
- Capability grant resolution for triggers uses the same recipe + role preset logic as `POST /v1/runs`.
- When `API_TENANT_MAX_TRIGGERS` is configured, trigger create requests may return `429` with error code `TENANT_TRIGGER_LIMITED` if tenant trigger capacity is exhausted.
- Interval trigger defaults:
  - `misfire_policy = "fire_now"`
  - `max_attempts = 3`
  - `max_inflight_runs = 1`
  - `jitter_seconds = 0`

## POST /v1/triggers/cron
Creates an enabled cron trigger.

Request:
```json
{
  "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "triggered_by_user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
  "recipe_id": "show_notes_v1",
  "input": { "source": "cron" },
  "requested_capabilities": [],
  "cron_expression": "0/1 * * * * * *",
  "schedule_timezone": "UTC",
  "max_attempts": 3,
  "max_inflight_runs": 1,
  "jitter_seconds": 15
}
```

Notes:
- `cron_expression` and `schedule_timezone` are required.
- `schedule_timezone` must be a valid TZ database name (for example `UTC`, `America/Chicago`).
- `max_attempts` must be between `1` and `20`.
- `max_inflight_runs` must be between `1` and `1000`.
- `jitter_seconds` must be between `0` and `3600`.

## POST /v1/triggers/webhook
Creates an enabled webhook trigger that accepts external events and turns them into queued runs.

Request:
```json
{
  "agent_id": "9ef35789-2dc7-4655-bcdf-3327e63341b0",
  "triggered_by_user_id": "6df842f4-9e58-455f-8e05-a81eef20a388",
  "recipe_id": "show_notes_v1",
  "input": { "source": "external_hook" },
  "requested_capabilities": [],
  "webhook_secret_ref": "env:SECUREAGNT_TRIGGER_SECRET",
  "max_attempts": 3,
  "jitter_seconds": 0
}
```

Notes:
- `max_attempts` must be between `1` and `20`.
- `max_inflight_runs` must be between `1` and `1000`.
- `jitter_seconds` must be between `0` and `3600`.
- `webhook_secret_ref` is optional. If set, event ingestion requires `x-trigger-secret`.
- Secrets are resolved via the shared resolver (`env:`, `file:`, optional CLI adapters for `vault:`, `aws-sm:`, `gcp-sm:`, `azure-kv:`).
- Cloud secret refs can include version-pin query params (for example `vault:...?...`, `aws-sm:...?version_id=...`) per backend support.

## PATCH /v1/triggers/{trigger_id}
Updates mutable trigger settings.

Supported fields (trigger-type aware):
- `interval_seconds` (interval triggers)
- `cron_expression` and `schedule_timezone` (cron triggers)
- `misfire_policy` (`fire_now` or `skip`)
- `max_attempts` (`1..20`)
- `max_inflight_runs` (`1..1000`)
- `jitter_seconds` (`0..3600`)
- `webhook_secret_ref` (webhook triggers)

## POST /v1/triggers/{trigger_id}/enable
Sets trigger `status` to `enabled`.

## POST /v1/triggers/{trigger_id}/disable
Sets trigger `status` to `disabled`.

## POST /v1/triggers/{trigger_id}/events
Enqueues an event for a webhook trigger.

Request:
```json
{
  "event_id": "evt-001",
  "payload": { "hello": "world" }
}
```

Headers:
- Required: `x-tenant-id`
- Optional/conditional: `x-trigger-secret` (required when trigger has `webhook_secret_ref`)

Response (`202 Accepted`):
```json
{
  "trigger_id": "18f6d0f5-01f9-4fe6-9ecf-cf22c1f2b070",
  "event_id": "evt-001",
  "status": "queued"
}
```

`status` values:
- `queued`: new event accepted
- `duplicate`: same `event_id` or same canonicalized payload already recorded for this trigger

Notes:
- Returns `404 NOT_FOUND` if the trigger does not exist for tenant.
- Returns `409 CONFLICT` when the trigger exists but is unavailable (`disabled` or schedule-broken).
- Returns `400 BAD_REQUEST` when trigger type is not `webhook`.
- Enqueue outcomes are explicitly modeled (`queued`, `duplicate`, `trigger unavailable`) to keep idempotency and availability semantics distinct.

## POST /v1/triggers/{trigger_id}/events/{event_id}/replay
Requeues a dead-lettered webhook event for scheduler replay.

Response (`202 Accepted`):
```json
{
  "trigger_id": "18f6d0f5-01f9-4fe6-9ecf-cf22c1f2b070",
  "event_id": "evt-001",
  "status": "queued_for_replay"
}
```

Notes:
- Supported only for webhook triggers.
- Event must currently be in `dead_lettered` status.
- Returns `409 CONFLICT` when the event exists but is not replayable from its current status.

## POST /v1/triggers/{trigger_id}/fire
Manually fires an enabled trigger into a queued run with deterministic idempotency.

Request:
```json
{
  "idempotency_key": "manual-001",
  "payload": { "source": "operator" }
}
```

Response (`202 Accepted`, created):
```json
{
  "trigger_id": "18f6d0f5-01f9-4fe6-9ecf-cf22c1f2b070",
  "run_id": "ec6f9f75-6aeb-4a06-bf2f-5dd8b3b0f9ea",
  "idempotency_key": "manual-001",
  "status": "created"
}
```

Response (`200 OK`, duplicate idempotency key):
```json
{
  "trigger_id": "18f6d0f5-01f9-4fe6-9ecf-cf22c1f2b070",
  "run_id": "ec6f9f75-6aeb-4a06-bf2f-5dd8b3b0f9ea",
  "idempotency_key": "manual-001",
  "status": "duplicate"
}
```

Notes:
- `idempotency_key` is required, trimmed, and capped at 128 characters.
- Deduplication key format is namespaced internally (`manual:<idempotency_key>`).
- Returns `409 CONFLICT` when the trigger is unavailable (`disabled` or schedule-broken).

## Trigger response fields
Trigger responses include:
- `trigger_type`: `interval` or `webhook` or `cron`
- `interval_seconds`: `number|null` (null for webhook/cron)
- `misfire_policy`
- `max_attempts`
- `max_inflight_runs`
- `jitter_seconds`
- `consecutive_failures`
- `dead_lettered_at`
- `dead_letter_reason`
- `dead_lettered_at` on trigger rows is the schedule-broken marker (distinct from webhook event dead-letter status in `trigger_events`).
- `cron_expression`
- `schedule_timezone`
- `webhook_secret_configured` (`true` when a secret ref is configured)
