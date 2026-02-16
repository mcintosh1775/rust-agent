# summarize_transcript

Reference Python skill for protocol and runner integration testing.

## Behavior modes
- `mode=success` (default): returns markdown summary output.
- `mode=timeout`: sleeps for `sleep_s` seconds before responding.
- `mode=crash`: exits non-zero (`17`) before producing a response.
- `mode=oversize`: returns a markdown payload sized by `bytes`.

## Notes
- This skill is intentionally minimal and only used for local/CI validation.
- Side effects are still requested through `action_requests`; the platform executes them.
