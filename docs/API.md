# API (MVP)

All endpoints require header `x-tenant-id`.

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
  "granted_capabilities": [],
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

## GET /v1/runs/{run_id}/audit
Returns ordered run audit events (`created_at`, then `id`), with optional query param:
- `limit` (default `200`, max `1000`)
