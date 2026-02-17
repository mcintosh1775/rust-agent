# API (MVP)

All endpoints require header `x-tenant-id`.

Optional run policy header:
- `x-user-role`: `owner` | `operator` | `viewer` (default: `owner`)

Trigger mutation note:
- `POST /v1/triggers`, `POST /v1/triggers/webhook`, and `POST /v1/triggers/{id}/fire` require role `owner` or `operator`.
- `viewer` receives `403 FORBIDDEN` for trigger mutation endpoints.

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

## GET /v1/runs/{run_id}/audit
Returns ordered run audit events (`created_at`, then `id`), with optional query param:
- `limit` (default `200`, max `1000`)

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
  "interval_seconds": 60
}
```

Response (`201 Created`): includes trigger metadata (`trigger_type=interval`, `next_fire_at`, capability grants).

Notes:
- `interval_seconds` must be `> 0` and `<= 31536000`.
- Capability grant resolution for triggers uses the same recipe + role preset logic as `POST /v1/runs`.
- Interval trigger defaults:
  - `misfire_policy = "fire_now"`
  - `max_attempts = 3`

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
  "webhook_secret_ref": "env:AEGIS_TRIGGER_SECRET",
  "max_attempts": 3
}
```

Notes:
- `max_attempts` must be between `1` and `20`.
- `webhook_secret_ref` is optional. If set, event ingestion requires `x-trigger-secret`.
- Secrets are resolved via the shared resolver (`env:`, `file:`, optional CLI adapters for `vault:`, `aws-sm:`, `gcp-sm:`, `azure-kv:`).

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
- `trigger_type`: `interval` or `webhook`
- `interval_seconds`: `number|null` (null for webhook)
- `misfire_policy`
- `max_attempts`
- `consecutive_failures`
- `dead_lettered_at`
- `dead_letter_reason`
- `webhook_secret_configured` (`true` when a secret ref is configured)
