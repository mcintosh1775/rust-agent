# SESSION_HANDOFF

Use this file to bootstrap a new Codex session quickly and consistently.

## Project Identity
- Name: `SecureAgnt`
- Domain: `SecureAgnt.ai`
- Primary CLI: `agntctl`
- Primary daemon binary: `secureagntd`
- Goal: secure, high-performance Rust agent runtime replacing OpenClaw-style architecture
- Messaging direction: Nostr-first, White Noise first-class, Slack enterprise-secondary

## Current State Snapshot
- Milestones completed:
  - M1 policy contracts and tests (`core/policy`)
  - M0N naming migration baseline:
    - brand/docs moved to `SecureAgnt`
    - new CLI scaffold `agntctl`
    - daemon/API binary aliases `secureagntd` and `secureagnt-api`
    - legacy env alias deprecation window documented:
      - `AEGIS_SECRET_ENABLE_CLOUD_CLI` accepted through `2026-06-30`
      - planned removal date `2026-07-01`
    - legacy skill marker emission can now be toggled:
      - `WORKER_SKILL_EMIT_LEGACY_AEGIS_MARKER=0` disables `AEGIS_SKILL_SANDBOXED`
    - systemd packaging templates added:
      - `infra/systemd/secureagnt.service`
      - `infra/systemd/secureagnt-api.service`
  - M8A baseline started: enterprise audit/compliance dual-plane routing/query
    - compliance-plane table: `compliance_audit_events`
    - trigger-routed classification from `audit_events` now active for high-risk classes:
      - `action.denied`
      - `action.failed`
      - `action.requested|action.allowed|action.executed` for `payment.send`/`message.send`
      - `run.failed`
    - tenant compliance endpoint:
      - `GET /v1/audit/compliance` (owner/operator only)
      - filters: `run_id`, `event_type`, `limit`
  - M2 schema + DB layer + integration tests (`core/db`, `migrations/0001_init.sql`)
  - M3 NDJSON skill protocol + subprocess runner + Python reference skill
  - M4 worker vertical slice with run leasing + step execution + action policy/execution (`object.write`)
  - M5 API baseline with run create/status/audit endpoints and DB integration tests
  - M5 API capability grant resolver baseline (requested capabilities are now normalized/filtered to policy-authoritative grants)
  - M7 baseline started: API-managed recipe capability bundles with request/bundle intersection in `POST /v1/runs`
  - M7 role-aware policy baseline: optional `x-user-role` preset (`owner`/`operator`/`viewer`) now constrains recipe bundle grants
  - M5A messaging baseline with `message.send` execution, local connector outbox persistence, and White Noise relay publish support (`NOSTR_RELAYS`)
  - M5A Slack transport added: `message.send` to `slack:*` now supports webhook delivery when configured
  - M5A Slack reliability update: configurable webhook retries/backoff with dead-letter outbox state on exhaustion
  - M5B signer baseline with pluggable Nostr identity modes (`local_key` default, optional `nip46_signer`) and NIP-46-backed relay publish signing
  - M6 hardening baseline with skill env scrubbing (`env_clear` + allowlist) and redacted action/audit payload persistence
  - M6 sandbox additions: constrained `local.exec` templates with path allowlists and local-first `llm.infer` routing with route-scoped policy grants
  - M6 spend controls: per-run remote `llm.infer` token budget enforcement + estimated cost metadata
  - M6C expanded baseline implemented:
    - remote `llm.infer` token usage ledger table (`llm_token_usage`)
    - remote budget windows for tenant/agent/model:
      - `LLM_REMOTE_TOKEN_BUDGET_PER_TENANT`
      - `LLM_REMOTE_TOKEN_BUDGET_PER_AGENT`
      - `LLM_REMOTE_TOKEN_BUDGET_PER_MODEL`
      - `LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS`
    - optional soft-alert threshold for near-exhaustion telemetry:
      - `LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT`
      - worker emits `llm.budget.soft_alert` audits when threshold is reached
    - fail-closed budget prechecks at run + tenant + agent + model levels
    - tenant usage query endpoint:
      - `GET /v1/usage/llm/tokens` (`window_secs`, optional `agent_id`, optional `model_key`)
  - M5C/M6A planning captured: Nostr-first sats payments rail and durable memory-plane milestone definitions
  - M5C baseline implementation started:
    - policy/API/worker support for `payment.send` with `nwc:*` scope
    - payment ledger tables (`payment_requests`, `payment_results`) with tenant idempotency key uniqueness
    - worker payment execution baseline (`pay_invoice`, `make_invoice`, `get_balance`) with per-run spend cap guardrail
    - live NIP-47 relay request/response path when `PAYMENT_NWC_URI`/`PAYMENT_NWC_URI_REF` is configured
    - wallet-id to NWC URI routing map support (`PAYMENT_NWC_WALLET_URIS`/`PAYMENT_NWC_WALLET_URIS_REF`) with optional wildcard default route
    - fail-closed wallet route enforcement when map mode is configured and destination wallet id is missing
    - route orchestration controls:
      - multi-route wallet values (`uri_a|uri_b`)
      - route strategy (`PAYMENT_NWC_ROUTE_STRATEGY`)
      - failover toggle (`PAYMENT_NWC_ROUTE_FALLBACK_ENABLED`)
      - canary rollout control (`PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT`)
      - route health controls:
        - `PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD`
        - `PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS`
    - relay timeout guardrail for NIP-47 (`PAYMENT_NWC_TIMEOUT_MS`)
    - worker payment tenant/agent spend guardrails (`PAYMENT_MAX_SPEND_MSAT_PER_TENANT`, `PAYMENT_MAX_SPEND_MSAT_PER_AGENT`)
    - approval threshold guardrail (`PAYMENT_APPROVAL_THRESHOLD_MSAT`) requiring explicit `payment_approved` flag on higher-value payouts
    - payment outbox artifact persistence under `payments/...`
    - tenant payment reconciliation/reporting endpoint:
      - `GET /v1/payments` with filters (`run_id`, `agent_id`, `status`, `destination`, `idempotency_key`)
      - returns latest payment result/status per request for settlement reconciliation workflows
    - tenant payment summary endpoint:
      - `GET /v1/payments/summary` with filters (`window_secs`, `agent_id`, `operation`)
      - returns aggregate counters and executed spend totals for ops dashboards
  - M4B/M6B planning captured: durable trigger plane and provider-agnostic secrets interface (Vault + cloud backends)
  - M4B baseline implemented: interval trigger creation (`POST /v1/triggers`) + worker due-trigger dispatch + `trigger_runs` ledger
  - M4B expanded baseline implemented:
    - cron trigger creation (`POST /v1/triggers/cron`) with timezone-aware schedule parsing
    - webhook trigger creation (`POST /v1/triggers/webhook`)
    - webhook event ingestion (`POST /v1/triggers/{id}/events`) with idempotent `event_id` dedupe
    - dead-letter webhook event replay endpoint (`POST /v1/triggers/{id}/events/{event_id}/replay`)
    - manual trigger fire endpoint (`POST /v1/triggers/{id}/fire`) with deterministic idempotency keys
    - trigger edit/lifecycle endpoints (`PATCH /v1/triggers/{id}`, `POST /v1/triggers/{id}/enable`, `POST /v1/triggers/{id}/disable`)
    - trigger event queue (`trigger_events`) with pending/processed/dead-lettered states
    - trigger audit stream persisted in `trigger_audit_events`
    - scheduler in-flight guardrails:
      - per-trigger `max_inflight_runs`
      - per-tenant scheduler limit (`WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS`)
    - interval misfire skip handling and persisted trigger-run failure ledger entries
    - trigger provenance now includes `trigger_type` and optional `trigger_event_id` in worker `run.created` audits
    - API trigger mutation role guardrail baseline (`viewer` denied trigger mutation endpoints)
    - scheduler HA lease coordination via `scheduler_leases` and worker `WORKER_TRIGGER_SCHEDULER_LEASE_*` controls
    - schedule jitter support via `triggers.jitter_seconds` for interval/cron dispatch
    - operator ownership guardrails for trigger mutation:
      - operator requests require `x-user-id`
      - operators can only create/mutate triggers for self
  - M6B expanded baseline implemented:
    - shared secret reference abstraction with `env:`/`file:` runtime resolution
    - CLI-backed Vault/AWS/GCP/Azure resolver adapters behind fail-closed gate `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`
    - worker/API secret-consuming paths now use `CliSecretResolver`
  - M6B cache/version expansion implemented:
    - shared TTL secret cache wrapper used by API + worker secret resolution paths
    - cache controls:
      - `SECUREAGNT_SECRET_CACHE_TTL_SECS`
      - `SECUREAGNT_SECRET_CACHE_MAX_ENTRIES`
    - cloud secret reference version-pin query support:
      - Vault (`?version=`)
      - AWS (`?version_id=` / `?version_stage=`)
      - GCP (`?version=`)
      - Azure (`?version=`)
    - rotation-focused unit coverage for cache hit/expiry refresh behavior
    - provider-adapter integration coverage with mocked CLI backends:
      - Vault version-pin + field-selection command-path test
      - AWS provider error propagation test
      - Azure version-pin command argument test
      - cached resolver version-rollover pickup test after TTL expiry
  - Coverage gate baseline implemented:
    - `make coverage` / `make coverage-db` via `cargo-llvm-cov`
    - CI enforces line coverage threshold (`COVERAGE_MIN_LINES`, default `70`)

## Mandatory Read Order (for new sessions)
1. `AGENTS.md`
2. `docs/SESSION_HANDOFF.md` (this file)
3. `docs/NAMING.md`
4. `docs/agent_platform.md`
5. `docs/ARCHITECTURE.md`
6. `docs/SECURITY.md`
7. `docs/POLICY.md`
8. `docs/SECRETS.md`
9. `docs/ROADMAP.md`
10. `CHANGELOG.md` (latest entries first)

## Critical ADRs
- `docs/ADR/ADR-0004-shared-postgres-topology.md` (shared DB topology)
- `docs/ADR/ADR-0005-nostr-first-whitenoise.md` (messaging priority)
- `docs/ADR/ADR-0006-sandboxed-local-exec-primitive.md` (sandbox boundary)
- `docs/ADR/ADR-0007-pluggable-nostr-signer-modes.md` (self-hosted + enterprise signer modes)

## Environment + Runtime Notes
- Operator entrypoints:
  - CLI scaffold: `agntctl`
  - daemon alias: `secureagntd` (same runtime as `worker`)
  - API alias: `secureagnt-api` (same runtime as `api`)
- Container runtime workflow is Podman/Docker compatible via `Makefile`.
- Default compose file: `infra/containers/compose.yml`
- Postgres image: `docker.io/library/postgres:18`
- PG18 volume mount must be `/var/lib/postgresql` (already set).
- `make test-db` defaults to DB URL `postgres://postgres:postgres@localhost:5432/agentdb`.
- Worker Nostr signer modes:
  - default `NOSTR_SIGNER_MODE=local_key`
  - optional `NOSTR_SIGNER_MODE=nip46_signer` with `NOSTR_NIP46_BUNKER_URI`
  - optional `NOSTR_NIP46_CLIENT_SECRET_KEY` for stable app-key identity when using NIP-46
  - relay publish knobs: `NOSTR_RELAYS` and `NOSTR_PUBLISH_TIMEOUT_MS`
- Slack transport knobs:
  - `SLACK_WEBHOOK_URL` and `SLACK_SEND_TIMEOUT_MS`
  - `SLACK_MAX_ATTEMPTS` and `SLACK_RETRY_BACKOFF_MS`
- Secret reference knobs:
  - `SLACK_WEBHOOK_URL_REF`, `LLM_LOCAL_API_KEY_REF`, `LLM_REMOTE_API_KEY_REF`
  - currently resolved: `env:...`, `file:...`
  - optional CLI adapters (disabled by default): `vault:...`, `aws-sm:...`, `gcp-sm:...`, `azure-kv:...`
  - secret cache controls:
    - `SECUREAGNT_SECRET_CACHE_TTL_SECS` (default `30`, `0` disables cache)
    - `SECUREAGNT_SECRET_CACHE_MAX_ENTRIES` (default `1024`)
  - gate: `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI=1`
  - migration compatibility gate: `AEGIS_SECRET_ENABLE_CLOUD_CLI=1` (accepted through `2026-06-30`, planned removal `2026-07-01`)
- Webhook trigger knobs/behavior:
  - API create endpoint: `POST /v1/triggers/webhook`
  - API event ingest endpoint: `POST /v1/triggers/{id}/events`
  - API dead-letter event replay endpoint: `POST /v1/triggers/{id}/events/{event_id}/replay`
  - API manual fire endpoint: `POST /v1/triggers/{id}/fire`
  - API lifecycle endpoints: `PATCH /v1/triggers/{id}`, `POST /v1/triggers/{id}/enable`, `POST /v1/triggers/{id}/disable`
  - optional `x-trigger-secret` header validation when trigger has `webhook_secret_ref`
  - trigger event payload guardrail: events above 64KB are rejected into retry/dead-letter flow
- API role preset knob:
  - optional request header `x-user-role` (`owner` default, `operator`, `viewer`)
  - usage/compliance query guardrails:
    - `GET /v1/usage/llm/tokens` (owner/operator only)
    - `GET /v1/payments` (owner/operator only)
    - `GET /v1/audit/compliance` (owner/operator only)
- Skill runtime env control:
  - optional `WORKER_SKILL_ENV_ALLOWLIST` (comma-separated env vars passed through to skill process)
  - optional `WORKER_SKILL_EMIT_LEGACY_AEGIS_MARKER` (`1` default; set `0` to disable `AEGIS_SKILL_SANDBOXED`)
- Trigger scheduler control:
  - `WORKER_TRIGGER_SCHEDULER_ENABLED` (default on)
  - `WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS` (default `100`)
  - optional scheduler lease gate (default on):
    - `WORKER_TRIGGER_SCHEDULER_LEASE_ENABLED`
    - `WORKER_TRIGGER_SCHEDULER_LEASE_NAME`
    - `WORKER_TRIGGER_SCHEDULER_LEASE_TTL_MS`
- Trigger operator ownership header:
  - `x-user-id` is required when `x-user-role=operator` is used on trigger mutation endpoints
- Payment rail controls:
  - `PAYMENT_NWC_ENABLED`
  - `PAYMENT_NWC_URI` / `PAYMENT_NWC_URI_REF`
  - `PAYMENT_NWC_WALLET_URIS` / `PAYMENT_NWC_WALLET_URIS_REF`
  - `PAYMENT_NWC_ROUTE_STRATEGY`
  - `PAYMENT_NWC_ROUTE_FALLBACK_ENABLED`
  - `PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT`
  - `PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD`
  - `PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS`
  - `PAYMENT_NWC_TIMEOUT_MS`
  - `PAYMENT_MAX_SPEND_MSAT_PER_RUN`
  - `PAYMENT_MAX_SPEND_MSAT_PER_TENANT`
  - `PAYMENT_MAX_SPEND_MSAT_PER_AGENT`
  - `PAYMENT_APPROVAL_THRESHOLD_MSAT`
  - `PAYMENT_NWC_MOCK_BALANCE_MSAT`
- Local exec sandbox control:
  - `WORKER_LOCAL_EXEC_ENABLED` plus path roots (`WORKER_LOCAL_EXEC_READ_ROOTS`, `WORKER_LOCAL_EXEC_WRITE_ROOTS`)
- LLM routing control:
  - `LLM_MODE` (`local_only`, `local_first`, `remote_only`)
  - local endpoint: `LLM_LOCAL_BASE_URL`, `LLM_LOCAL_MODEL`
  - optional remote endpoint: `LLM_REMOTE_BASE_URL`, `LLM_REMOTE_MODEL`, `LLM_REMOTE_API_KEY`
  - remote egress gate: `LLM_REMOTE_EGRESS_ENABLED` + `LLM_REMOTE_HOST_ALLOWLIST`
  - optional remote spend controls:
    - `LLM_REMOTE_TOKEN_BUDGET_PER_RUN`
    - `LLM_REMOTE_TOKEN_BUDGET_PER_TENANT`
    - `LLM_REMOTE_TOKEN_BUDGET_PER_AGENT`
    - `LLM_REMOTE_TOKEN_BUDGET_PER_MODEL`
    - `LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS`
    - `LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT`
    - `LLM_REMOTE_COST_PER_1K_TOKENS_USD`

## Local Verification Commands
```bash
make container-info
make db-up
make test-db
make test-worker-db
make test-api-db
make test
make coverage
make coverage-db
make agntctl
make secureagntd
make secureagnt-api
```

## Key Code Areas
- Policy engine: `core/src/policy.rs`
- DB primitives and run-lease APIs: `core/src/db.rs`
- DB integration tests: `core/tests/db_integration.rs`
- Skill protocol: `skillrunner/src/protocol.rs`
- Skill runner: `skillrunner/src/runner.rs`
- CLI scaffold: `agntctl/src/main.rs`
- API router/handlers: `api/src/lib.rs`
- Worker execution + action policy path: `worker/src/lib.rs`
- Worker Nostr signer config/identity handling: `worker/src/signer.rs`
- Worker NIP-46 remote signer transport: `worker/src/nip46_signer.rs`
- Worker relay publish transport: `worker/src/nostr_transport.rs`
- Worker Slack webhook transport: `worker/src/slack.rs`
- Worker local exec sandbox primitive: `worker/src/local_exec.rs`
- Worker LLM routing/execution: `worker/src/llm.rs`
- Redaction utilities: `core/src/redaction.rs`
- Reference Python skill: `skills/python/summarize_transcript/main.py`

## High-Priority Next Steps
1. Continue M5C payment hardening: add optional Cashu rail planning and deeper reconciliation workflows after the payment ledger API baseline.
2. Continue M0N naming migration: finish runtime/deployment alias cleanup so remaining `AEGIS_*` env compatibility can be removed on/after `2026-07-01`.
3. Continue M8A enterprise audit/compliance implementation: immutable export path, tamper-evidence, SIEM adapters, and retention/legal-hold controls.
4. Continue M6A durable memory-plane implementation: retrieval-backed memory model, compaction, and retention controls.
5. Advance M7 multi-tenancy hardening: deeper tenant isolation tests and quota/index tuning.

## New Session Prompt (copy/paste)
```text
Read AGENTS.md and docs/SESSION_HANDOFF.md first, then docs/NAMING.md, docs/agent_platform.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/POLICY.md, docs/SECRETS.md, docs/ROADMAP.md, and recent CHANGELOG entries. Summarize current implemented state vs remaining roadmap, then continue with the next unfinished milestone.
```
