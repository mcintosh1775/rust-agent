# API (MVP)

All endpoints require header `x-tenant-id`.

Optional run policy header:
- `x-user-role`: `owner` | `operator` | `viewer` (default: `owner`)

Optional API capacity guardrails:
- `API_TENANT_MAX_INFLIGHT_RUNS` (positive integer): if set, `POST /v1/runs` returns `429 TENANT_INFLIGHT_LIMITED` when tenant queued+running runs are at/above the limit.
- `API_TENANT_MAX_TRIGGERS` (positive integer): if set, trigger create endpoints return `429 TENANT_TRIGGER_LIMITED` when tenant trigger count is at/above the limit.

Trigger mutation note:
- `POST /v1/triggers`, `POST /v1/triggers/cron`, `POST /v1/triggers/webhook`,
  `PATCH /v1/triggers/{id}`, `POST /v1/triggers/{id}/enable`,
  `POST /v1/triggers/{id}/disable`, `POST /v1/triggers/{id}/fire`, and
  `POST /v1/triggers/{id}/events/{event_id}/replay` require role `owner` or `operator`.
- `viewer` receives `403 FORBIDDEN` for trigger mutation endpoints.
- When `x-user-role=operator` is used on trigger mutation endpoints, `x-user-id` is required:
  - create operations require operator `x-user-id` to match `triggered_by_user_id` (or set it implicitly)
  - update/enable/disable/fire operations allow only triggers owned by the same user id

Usage query note:
- `GET /v1/usage/llm/tokens` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on usage query endpoints.
- `GET /v1/payments` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on payment ledger query endpoints.
- `GET /v1/payments/summary` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on payment summary query endpoints.
- `GET /v1/ops/summary` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on ops summary endpoints.
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
- `GET /v1/audit/compliance/replay-package` is allowed for `owner` and `operator`.
- `viewer` receives `403 FORBIDDEN` on replay package endpoints.
- `POST /v1/audit/compliance/purge` is allowed for `owner` only.
- `operator` and `viewer` receive `403 FORBIDDEN` on compliance purge endpoints.

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

Response (`201 Created`):
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
  - `payment.send` is supported with `nwc:*` scope only (NWC-first baseline).
  - Cashu rail support is planned but not active yet (see `docs/PAYMENTS.md`).
  - Recipe `payments_v1` grants `payment.send` by default.

## GET /v1/runs/{run_id}/audit
Returns ordered run audit events (`created_at`, then `id`), with optional query param:
- `limit` (default `200`, max `1000`)

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
    "created_at": "2026-02-17T12:00:03Z",
    "recorded_at": "2026-02-17T12:00:03Z"
  }
]
```

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
    "created_at": "2026-02-17T12:00:00Z",
    "updated_at": "2026-02-17T12:00:03Z",
    "latest_result_created_at": "2026-02-17T12:00:03Z"
  }
]
```

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
- `duplicate`: same `event_id` already recorded for this trigger

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
- `cron_expression`
- `schedule_timezone`
- `webhook_secret_configured` (`true` when a secret ref is configured)
