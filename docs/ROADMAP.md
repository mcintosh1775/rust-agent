# ROADMAP (Aegis)

This roadmap sequences delivery from scaffold to enterprise-ready platform while preserving the MVP security model and thin vertical-slice focus.

## M1 — Core Contracts (Week 1)
Scope:
- Implement shared domain types in `core/` for capabilities, action requests, policy decisions, and deny reasons.
- Implement default-deny policy evaluator with deterministic deny reasons.
- Add required unit tests for allow/deny behavior and limits.

Landmarks:
- Policy engine compiles into reusable `core` API.
- Unit tests prove:
  - deny unknown action type
  - deny when capability missing
  - deny when scope mismatch
  - deny when payload exceeds limits
  - allow when exact capability + scope match

Exit criteria:
- `cargo test -p core` passes with required policy coverage.

## M2 — Persistence Foundation (Week 1-2)
Scope:
- Add first migration set for `runs`, `steps`, `artifacts`, `action_requests`, `action_results`, `audit_events`, `agents`, `users`.
- Use one standardized app schema per environment (for example `aegis`) in shared Postgres.
- Add minimal DB layer for run lifecycle + audit append.

Landmarks:
- Migrations are idempotent and apply in local and CI flows.
- DB layer supports create run/step + append audit.

Exit criteria:
- Integration tests validate migration apply + basic inserts + audit append.

## M3 — Skill Protocol v0 + Runner (Week 2)
Scope:
- Implement NDJSON protocol types and codec (`describe`, `invoke`, `invoke_result`).
- Implement subprocess skill runner with timeout, crash handling, and output-size caps.
- Add reference compute-only Python skill.

Landmarks:
- Runner returns structured error codes for timeout/crash/oversize.
- Protocol round-trip tests pass.

Exit criteria:
- Integration tests validate success, timeout kill, crash containment, oversized output rejection.

## M4 — Worker Vertical Slice (Week 2-3)
Status:
- Implemented baseline: worker now invokes the reference skill, evaluates action requests through policy, executes allowed `object.write`, persists action request/results, and records step/run audit events.

Scope:
- Build worker queue loop for queued runs.
- Invoke skill, evaluate action requests, execute allowed actions.
- MVP side effects: `object.write` and `message.send` (White Noise first; Slack optional).

Landmarks:
- Run state transitions are persisted (`queued -> running -> succeeded|failed`).
- Worker claims queued runs with lease semantics (`FOR UPDATE SKIP LOCKED`) to avoid duplicate execution.
- Action requests/results and audit records are persisted per step.

Exit criteria:
- Worker integration test validates lifecycle, action decisions, and audit trail completeness.

## M5 — API Surface (Week 3)
Status:
- Implemented baseline: `POST /v1/runs`, `GET /v1/runs/{id}`, and `GET /v1/runs/{id}/audit` are live with tenant-scoped DB queries and integration tests.
- Policy-authoritative capability grant resolution added to `POST /v1/runs`; API no longer mirrors requested grants.

Scope:
- Implement:
  - `POST /v1/runs`
  - `GET /v1/runs/{id}`
  - `GET /v1/runs/{id}/audit`
- Keep auth minimal but replaceable.

Landmarks:
- API creates runs with capability requests and returns stable identifiers.
- Audit endpoint streams persisted run events in order.

Exit criteria:
- API integration tests pass for create/status/audit happy paths.

## M5A — Channel Communication Connectors (Week 3-4)
Status:
- Implemented baseline: worker now executes `message.send` for `whitenoise:*` and `slack:*` destinations by policy, with local outbox persistence, audit/action result tracking, and Nostr relay publish for White Noise when relays are configured.

Scope:
- Implement first-class White Noise connector flows (Marmot over Nostr) for `message.send`.
- Implement Slack connector as enterprise-secondary path.
- Add capability scope conventions for channel destinations and payload caps.

Landmarks:
- White Noise delivery path is default in example recipes and demos.
- Slack delivery path is policy-gated and allowlist-scoped.
- Worker can sign and publish Nostr text-note events to configured relays for White Noise destinations.
- Relay publish ACK/failure results are persisted in action result payloads and outbox artifacts.

Exit criteria:
- Integration tests cover allowed/denied `message.send` for White Noise and Slack destinations.

## M5B — Nostr Signer Modes (Week 3-4)
Status:
- Implemented baseline: worker now supports pluggable signer configuration with `local_key` (default) and optional `nip46_signer` identity mode.

Scope:
- Add a signer-provider configuration layer for Nostr identity handling.
- Keep local-key mode available for self-hosted users.
- Support NIP-46 remote signer mode for hardened deployments.

Landmarks:
- Worker startup validates signer mode config.
- Local mode derives `npub` from local secret key (`env` or file).
- NIP-46 mode validates bunker URI + public key identity.

Exit criteria:
- Unit tests cover local and NIP-46 config parsing/identity resolution.
- Worker startup surfaces signer mode/public key or explicit disabled warning.

## M6 — Security Hardening (Week 4)
Scope:
- Enforce strict boundaries:
  - only `api`/`worker` DB access
  - no secrets to skills
  - deny-by-default egress in worker/skill runtime
- Introduce host sandbox controls for any local execution primitives:
  - explicit command templates (no arbitrary shell)
  - scoped filesystem access (allowlisted paths only)
  - strict per-step time/memory/output limits
  - implementation aligned with `docs/ADR/ADR-0006-sandboxed-local-exec-primitive.md`
- Add validation caps/rate limits and redaction enforcement.

Landmarks:
- Threat model items have mapped tests/controls.
- High-risk defaults remain deny-first.

Exit criteria:
- Security-focused test suite covers denial, containment, and redaction paths.

## M7 — Enterprise Multi-Tenancy (Week 5-6)
Scope:
- Add tenant-aware authz and per-tenant scoping across run/step/action/audit operations.
- Add capacity controls and query/index tuning for high concurrency.

Landmarks:
- Tenant boundaries enforced in API and worker query paths.
- Agent/user attribution is complete for operational and audit events.

Exit criteria:
- Isolation tests demonstrate no cross-tenant data access.

## M8 — Production Readiness (Week 7-8)
Scope:
- Add metrics/tracing/logging coverage for run and action paths.
- Finalize runbooks for incident response, backup/restore, migration rollback.
- Add performance baseline and soak checks.

Landmarks:
- Per-run traceability is available end-to-end.
- Operational checklist is complete and repeatable.

Exit criteria:
- Staging soak run completes with no blocker issues.

## M9 — Governance & Supply Chain (Post-MVP)
Scope:
- Signed connector/skill artifacts, version pinning, and approval gates for sensitive actions.
- Provenance and policy workflows for reviewed extension promotion.

Landmarks:
- Verified signature checks in install/enable paths.
- Approval gate workflow for irreversible actions.

Exit criteria:
- Governance controls enforced by policy and covered by tests.
