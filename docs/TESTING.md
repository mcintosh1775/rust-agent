# TESTING

This repo uses a **tests-as-you-go** workflow. New functionality is not “done” until it is covered by tests that prove:
- **security posture** (deny-by-default, scoped allow)
- **auditability** (events written for allow/deny + execution)
- **failure containment** (timeouts/crashes don’t break the worker or leak authority)

## Test layers

### 1) Unit tests (fast, no external deps)
Location:
- `core/src/...` with `#[cfg(test)]` modules, or `core/tests/*.rs`

Use unit tests for:
- capability scope matching
- policy allow/deny decisions + reasons
- payload size limits / validation
- protocol encode/decode (pure functions)

Required unit tests for policy:
- Deny unknown action type
- Deny when capability missing
- Deny when scope mismatch
- Deny when payload exceeds limits
- Allow when exact capability + scope match
- Stable, deterministic “deny reason” strings/enums (so audits are consistent)

### 2) Integration tests (real Postgres, real subprocess skill)
Location:
- `tests/*.rs` at workspace root, or `worker/tests/*.rs` for worker-specific tests

Integration tests must cover:
- migrations apply successfully
- run lifecycle state transitions (queued → running → succeeded/failed)
- worker step execution with skill invocation and action request/result persistence
- policy allow/deny behavior for requested actions
- audit events exist for:
  - run created
  - step started/finished
  - skill invoked
  - action requested
  - action allowed/denied (+ reason)
  - action executed/failed (+ result)
- skill runner behavior:
  - successful invoke
  - timeout kill + recorded failure
  - crash/exit non-zero + recorded failure
  - oversized output rejected

## Running tests locally

### Prereqs
- Podman (with compose) or Docker available
- `make db-up` starts Postgres
- default compose file is `infra/containers/compose.yml`
- `cargo-llvm-cov` for measured coverage runs (`cargo install cargo-llvm-cov`)

### Commands
- `make container-info` (shows detected compose runtime/versions)
- `make test` (runs `cargo test`)
- `make test-db` (runs `core` DB integration tests with `RUN_DB_TESTS=1`)
- `make test-worker-db` (runs `worker` DB integration tests with `RUN_DB_TESTS=1`)
- `make test-api-db` (runs `api` DB integration tests with `RUN_DB_TESTS=1`)
- `make check` (fmt + clippy + test)
- `make coverage` (workspace coverage summary with line-threshold gate)
- `make coverage-db` (coverage summary including DB integration tests)
- `make security-gate` (security-focused deny/containment/redaction gate; DB-backed worker checks enabled with `RUN_DB_SECURITY=1` or `RUN_DB_TESTS=1`)
- `make validation-gate` (runbook + verify + security + perf; optional DB/coverage via `VALIDATION_GATE_RUN_DB_SUITES=1` and `VALIDATION_GATE_RUN_COVERAGE=1`)
- `make release-manifest` / `make release-manifest-verify` (deployment artifact integrity manifest generation/verification)
- `make deploy-preflight` (deployment template validation; optional manifest verification)
- `make db-up` / `make db-down`
- `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test` (enables DB integration tests)

## Database test strategy
Integration tests must run against an isolated database:
- Default: `postgres://postgres:postgres@localhost:5432/agentdb`

Recommended approach:
- Use a **unique schema per test**:
  - create schema `test_<uuid>`
  - set `search_path` to that schema
  - run migrations into it
  - drop schema at test end

## Timeouts and limits
- Skill runner tests MUST use timeouts (e.g., 1–5s) so CI can’t hang.
- Worker tests MUST cap polling loops with a deadline.
- Enforce output size caps in protocol handling and test them.

## Coverage gate
- CI enforces a minimum line-coverage threshold via `make coverage-db`.
- Current threshold is configured via `COVERAGE_MIN_LINES` (default `70`).
- If you change test topology significantly, update threshold deliberately and document the reason in `CHANGELOG.md`.

## What Codex must do
When adding a feature, Codex must:
1. Add/extend unit tests for the core logic.
2. Add/extend integration tests if behavior crosses boundaries (DB, skill runner, worker).
3. Ensure `make check` passes.
