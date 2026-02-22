# Code Organization Notes

This repository is already large. Use explicit subsection markers to make growth predictable and onboarding easier.

## Current convention

- Add one-line section headers in large files using `// --- ... ---` comments.
- Keep markers at the start of logical groups (e.g., run API endpoints, trigger lifecycle, memory APIs).
- Avoid adding new markers for one-off tests unless the area grows or the next test is in a different domain.
- For new files, place a top-of-file section banner before the first item and keep helper sections at the end.

## Current marker map

### `api/tests/api_integration.rs`

- `// --- Runs endpoints: create/get/audit and baseline guards ---`
  - `create_run*`, `tenant/role` validation, run audit checks.
- `// --- Profile compatibility and sqlite-only flow coverage ---`
  - sqlite profile/compatibility tests and parity snapshots.
- `// --- Trigger endpoints (interval/cron/webhook/manual) and trigger limits ---`
  - trigger CRUD, fire, webhook secret, and related validations.
- `// --- Operations and metrics endpoints ---`
  - `ops/*`, latency, summary, and LLM gateway metrics.
- `// --- Memory records, handoff packets, and compaction ---`
  - memory CRUD/retrieve/redaction/compaction and handoff packets.
- `// --- Payments ledger endpoints ---`
  - payment query/summary and request state checks.
- `// --- Compliance audit and SIEM endpoints ---`
  - compliance reads, exports, SIEM delivery operations, alerts, replay, policy.
- `// --- Capability/validation edge cases ---`
  - request validation, role/tenant guards, and capability filtering edge cases.
- `// --- Shared test fixtures and request/DB helpers ---`
  - `setup_test_db`, seed helpers, request builders, and response helpers.

### `api/src/lib.rs`

- Existing section headers (added during the last sweep):
  - API module structure
  - API state model
  - Router entrypoints and variants
  - Shared configuration and environment helpers
  - Request/response contracts
  - API handlers and response helpers
  - Shared helper utilities

### `core/src/db.rs`

- `// --- Core record/domain types ---`
  - `NewRun`/`NewStep`/`NewActionRequest`/`NewMemoryRecord` record definitions.
- `// --- Constants and persistence utility helpers ---`
  - default tenant constants and SQL payload/transformation helpers.
- `// --- Run and step persistence ---`
  - `create_run`, `get_run_status`, inflight and step lifecycle helpers.
- `// --- Step transition persistence ---`
  - `create_step`, `mark_step_succeeded`, `mark_step_failed`.
- `// --- Action request persistence ---`
  - `create_action_request`, `update_action_request_status`.
- `// --- Action result persistence ---`
  - `create_action_result`, action results queries.
- `// --- Memory and token usage persistence ---`
  - `create_memory_record`, memory compaction queries, memory stats.
- `// --- LLM token usage persistence ---`
  - `create_llm_token_usage_record`, usage queries.
- `// --- Compliance and audit persistence ---`
  - `append_audit_event`, compliance policy, SIEM delivery records.
- `// --- Trigger and scheduler persistence ---`
  - `create_interval_trigger`, `dispatch_next_due_*`, claim helpers.

### `worker/src/lib.rs`

- `// --- Worker strategy and runtime configuration ---`
  - `PaymentNwcRouteStrategy`, `WorkerConfig`.
- `// --- Worker cycle entrypoint and control flow ---`
  - `process_once`, `process_once_dual`, scheduler and claimed-run flow.
- `// --- Compliance SIEM outbox processor ---`
  - `process_compliance_siem_delivery_outbox`, delivery attempts and lifecycle handling.
- `// --- Action execution and governance contract checks ---`
  - `execute_action`, action invocation helpers, contract validation.
- `// --- Parsing and configuration helpers ---`
  - environment readers, capability parsing, wallet routing parsers.

### `worker/src/llm.rs`

- `// --- LLM core enums and typed models ---`
  - enum and config struct definitions.
- `// --- Action scope helpers ---`
  - `policy_scope_for_action`, route/payload helpers.
- `// --- Routing and endpoint selection ---`
  - `execute_llm_infer`, route selection, endpoint picking.
- `// --- Verifier pipeline and SLO evaluation ---`
  - verifier modes, score/rule composition, SLO checks.
- `// --- Context retrieval and prompt shaping ---`
  - prompt-plan + retrieval + summarization helpers.
- `// --- Parsing and environment helpers ---`
  - action arg parsing and env parsing in llm module.

## When to add new markers

- Add a marker when adding 3+ related tests to a new domain.
- Add a marker when a file crosses ~250 lines of tests with mixed domains.
- Add/update this document whenever you introduce a major subsection.
