# MVP_PLAN (Vertical Slice)

This plan is designed to keep implementation narrow and verifiable. The MVP is complete when the acceptance criteria at the bottom are met.

## MVP goal (one sentence)
Run a single recipe end-to-end:
**read transcript → invoke compute-only skill → write markdown artifact → send White Noise notification → produce an auditable run record.**

> MVP explicitly avoids: general `http.request`, multi-tenancy, UI, marketplace/signing, microVM isolation.

---

## 0) Repo scaffolding (Day 0–1)
### Deliverables
- Rust workspace with crates:
  - `api/`, `worker/`, `core/`, `skillrunner/`
- `Makefile` targets exist: `fmt`, `lint`, `test`, `check`, `api`, `worker`
- `infra/containers/compose.yml` for Postgres
- Docs present: `AGENTS.md`, `docs/SECURITY.md`, `docs/POLICY.md`, `docs/ARCHITECTURE.md`, `docs/agent_platform.md`

### Tests
- `cargo test` passes (even if only a placeholder test).
- Add a `core` unit test that asserts "deny by default" in policy evaluator.

---

## 1) Define the data model + migrations (Day 1–2)
### Deliverables
- Postgres migrations for:
  - `runs`, `steps`, `artifacts`
  - `action_requests`, `action_results`
  - `audit_events`
- Migration strategy must target one standardized app schema in a shared Postgres service (not per-agent databases/schemas).
- Minimal Rust DB layer (sqlx recommended) in `core/`:
  - create run
  - create step
  - append audit event
  - persist artifact metadata

### Tests
- **Integration tests** spin up Postgres and validate:
  - migrations apply successfully
  - inserting a run/step works
  - audit_events append works
- Keep tests deterministic; use a separate test database/schema.

---

## 2) Implement the capability/policy engine (Day 2–3)
### Deliverables
- Capability model:
  - `Capability { kind, scope, limits }`
  - `GrantSet` (list of capabilities)
- Policy evaluator:
  - `is_action_allowed(grants, action_request) -> Allow|Deny(reason)`
- Default policy: deny all unless explicitly granted.

### Tests (MUST)
Unit tests in `core`:
- Deny unknown action type.
- Deny when capability missing.
- Deny when scope mismatch.
- Deny when payload exceeds max bytes.
- Allow when exact capability+scope match.

---

## 3) Skill Protocol v0 codec + Skill Runner (Day 3–5)
### Deliverables
- NDJSON protocol codec in `skillrunner/`
- Subprocess runner:
  - spawn skill executable
  - send `invoke`
  - read `invoke_result`
  - enforce:
    - timeout
    - max output bytes (defensive)
    - exit/crash handling
- **Reference Python skill** in `skills/python/summarize_transcript/`:
  - compute-only: returns markdown output
  - may optionally include an `object.write` action_request

### Tests (MUST)
- **Unit tests** for protocol codec:
  - round-trip serialization for `invoke` and `invoke_result`
- **Integration tests** that run the reference Python skill:
  - skill returns output under timeout
  - skill crash is handled and recorded
  - oversized response is rejected

---

## 4) Worker execution loop (Day 5–7)
### Deliverables
- Worker polls for queued runs
- Executes steps:
  - load step input
  - invoke skill
  - evaluate action requests
  - execute allowed actions (MVP: `object.write` + `message.send` to White Noise; Slack optional)
  - record action_results
  - append audit events for allow/deny and execution result

### Tests (MUST)
- Integration test that creates a run in Postgres, starts worker in test mode, and verifies:
  - run transitions: queued → running → succeeded
  - artifact record exists
  - audit trail contains:
    - skill invoked
    - action requested
    - action allowed/denied
    - action executed
  - action_results status matches expectation

---

## 5) API endpoints (Day 7–10)
### Deliverables
- `POST /v1/runs` creates a run
- `GET /v1/runs/{id}` returns status and outputs
- `GET /v1/runs/{id}/audit` returns audit events

Auth can be minimal for MVP (single-tenant token), but must be structured so real auth can replace it.

### Tests
- HTTP API integration tests:
  - create run returns run_id
  - get run returns expected state
  - audit endpoint returns events

---

## 6) MVP demo script (Day 10+)
### Deliverables
- one-command demo path via `make solo-lite-agent` (implemented by `scripts/ops/solo_lite_agent_run.py`) that:
  - starts the solo-lite stack as needed
  - seeds agent/user baseline rows
  - submits a text-backed run
  - waits/polls for completion
  - prints artifact path + audit summary

---

# Acceptance Criteria (Definition of Done)
MVP is DONE when:
1. `make check` succeeds on a clean checkout.
2. `make db-up` + `make migrate` + `make api` + `make worker` runs locally.
3. A demo run completes end-to-end producing:
   - a markdown artifact record
   - a complete audit trail
   - a White Noise message to an allowlisted destination
   - (optional) a Slack message to an allowlisted destination
4. Tests cover:
   - policy allow/deny logic (unit tests)
   - skill runner timeout/crash (integration tests)
   - worker run lifecycle (integration tests)
