# audit_finding_summary

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
    "markdown": "# audit_finding_summary...",
    "skill": "audit_finding_summary",
    "generated_at": "2026-02-22T00:00:00Z"
  },
  "action_requests": []
}
```
