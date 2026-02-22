# pii_scrub_report

This is a dedicated top-20 skill wrapper.

It delegates to the shared implementation in
`skills/python/top20_skill_impl.py` and returns a single-skill NDJSON
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
    "markdown": "# pii_scrub_report...",
    "skill": "pii_scrub_report",
    "generated_at": "2026-02-22T00:00:00Z"
  },
  "action_requests": []
}
```
