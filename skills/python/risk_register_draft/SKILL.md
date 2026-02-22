# risk_register_draft

This is a dedicated shared-skill wrapper.

It delegates to the shared implementation in
`skills/python/skill_impl.py` and returns a single-skill NDJSON
`describe` and `invoke` contract.

## Example input

```json
{
  "id": "example-invoke-1",
  "type": "invoke",
  "input": {
    "text": "incident observed in service A"
  }
}
```

## Example output

```json
{
  "type": "invoke_result",
  "id": "example-invoke-1",
  "output": {
    "markdown": "# risk_register_draft...",
    "skill": "risk_register_draft",
    "generated_at": "2026-02-22T00:00:00Z"
  },
  "action_requests": []
}
```
