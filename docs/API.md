# API (MVP sketch)

## POST /v1/runs
Request:
```json
{
  "recipe_id": "show_notes_v1",
  "input": { "transcript_path": "podcasts/ep245/transcript.txt" },
  "requested_capabilities": [
    { "capability": "object.read", "scope": "podcasts/*" },
    { "capability": "object.write", "scope": "shownotes/*" },
    { "capability": "message.send", "scope": "slack:C123456" }
  ]
}
```

Response:
```json
{ "run_id": "r-123", "status": "queued" }
```

## GET /v1/runs/{run_id}
Status + outputs.

## GET /v1/runs/{run_id}/audit
Audit stream.
