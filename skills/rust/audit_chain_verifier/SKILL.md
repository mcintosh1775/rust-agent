# audit_chain_verifier

Rust-based reference skill that verifies tamper-evidence chain integrity on structured events.

## Purpose

This skill takes a list of audit-like events and validates:

- sequence continuity,
- previous-hash chaining,
- computed tamper hash correctness.

It is intentionally compute-only and deterministic (safe for strict policy environments).

## Runtime contract

- Transport: NDJSON over stdin/stdout.
- Supported message types: `describe`, `invoke`, `describe_result`, `invoke_result`.
- `describe` returns:
  - capability requirements (`[]`),
  - output schema,
  - skill metadata.

## Invoke input

- `events`: required array of event objects.
- `seed` (optional): chain seed value (default: `GENESIS`).
- `seq_field` (optional): sequence field name (default: `tamper_chain_seq`).
- `prev_hash_field` (optional): previous hash field name (default: `tamper_prev_hash`).
- `hash_field` (optional): hash field name (default: `tamper_hash`).
- `request_write` (optional): if true, requests one `object.write` action with a
  summary artifact payload.

## Output

- `markdown`: human-readable summary.
- `chain_valid`: boolean.
- `verified_count`, `failed_count`.
- `issues`: structured list with `index`, `event_id`, `code`, `detail`, `severity`.

## Notes

- No side effects are performed by the skill itself.
- Side effects still require policy approval and execution through `object.write` action requests.
