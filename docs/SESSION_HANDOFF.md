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
  - M0N naming migration completed:
    - brand/docs moved to `SecureAgnt`
    - new CLI scaffold `agntctl`
    - daemon/API binary aliases `secureagntd` and `secureagnt-api`
    - runtime env naming finalized:
      - `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`
      - `SECUREAGNT_SKILL_SANDBOXED=1`
    - systemd packaging templates added:
      - `infra/systemd/secureagnt.service`
      - `infra/systemd/secureagnt-api.service`
  - M8A baseline advanced: enterprise audit/compliance dual-plane routing/query + tamper-evidence + retention/legal-hold controls
    - compliance-plane table: `compliance_audit_events`
    - trigger-routed classification from `audit_events` now active for high-risk classes:
      - `action.denied`
      - `action.failed`
      - `action.requested|action.allowed|action.executed` for `payment.send`/`message.send`
      - `run.failed`
    - tenant compliance endpoint:
      - `GET /v1/audit/compliance` (owner/operator only)
      - `GET /v1/audit/compliance/verify` (owner/operator only)
      - filters: `run_id`, `event_type`, `limit`
      - optional correlation fields in response:
        - `request_id`
        - `session_id`
        - `action_request_id`
        - `payment_request_id`
    - tenant compliance export endpoint:
      - `GET /v1/audit/compliance/export` (owner/operator only)
      - NDJSON output for batch export/ingestion workflows
    - tamper-evidence baseline:
      - compliance hash-chain fields: `tamper_chain_seq`, `tamper_prev_hash`, `tamper_hash`
      - DB verifier function: `verify_compliance_audit_chain(tenant_id)`
    - retention/legal-hold baseline:
      - policy table: `compliance_audit_policies`
      - policy endpoints:
        - `GET /v1/audit/compliance/policy` (owner/operator)
        - `PUT /v1/audit/compliance/policy` (owner only)
      - purge endpoint:
        - `POST /v1/audit/compliance/purge` (owner only)
      - DB purge function:
        - `purge_expired_compliance_audit_events(tenant_id, as_of)`
    - SIEM export adapter baseline:
      - `GET /v1/audit/compliance/siem/export`
      - adapters:
        - `secureagnt_ndjson`
        - `splunk_hec`
        - `elastic_bulk`
    - deterministic replay package baseline:
      - `GET /v1/audit/compliance/replay-package`
      - package includes run status, run audit events, compliance events, optional payment ledger, and correlation summary
    - replay package manifest baseline:
      - replay responses include deterministic `manifest` payload (`version`, `digest_sha256`, `signing_mode`, optional `signature`)
      - optional signing key controls:
        - `COMPLIANCE_REPLAY_SIGNING_KEY`
        - `COMPLIANCE_REPLAY_SIGNING_KEY_REF`
      - runbook rotation workflow now documented (`docs/RUNBOOK.md`, section `Compliance replay signing-key rotation`)
    - SIEM delivery outbox scaffold:
      - table: `compliance_siem_delivery_outbox`
      - queue endpoint:
        - `POST /v1/audit/compliance/siem/deliveries` (owner/operator)
      - observability endpoint:
        - `GET /v1/audit/compliance/siem/deliveries` (owner/operator)
        - `GET /v1/audit/compliance/siem/deliveries/summary` (owner/operator)
        - `GET /v1/audit/compliance/siem/deliveries/slo` (owner/operator)
        - `GET /v1/audit/compliance/siem/deliveries/targets` (owner/operator)
      - replay endpoint:
        - `POST /v1/audit/compliance/siem/deliveries/{id}/replay` (owner/operator)
      - worker outbox processing and status lifecycle:
        - `pending -> processing -> delivered|failed|dead_lettered`
      - worker controls:
        - `WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED`
        - `WORKER_COMPLIANCE_SIEM_DELIVERY_BATCH_SIZE`
        - `WORKER_COMPLIANCE_SIEM_DELIVERY_LEASE_MS`
        - `WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_BACKOFF_MS`
        - `WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_JITTER_MAX_MS`
        - `WORKER_COMPLIANCE_SIEM_HTTP_ENABLED`
        - `WORKER_COMPLIANCE_SIEM_HTTP_TIMEOUT_MS`
        - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_HEADER`
        - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN`
        - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN_REF`
      - local scaffold targets:
        - `mock://success`
        - `mock://fail`
  - M8 baseline advanced: tenant operations summary, soak/perf gate automation, and runbook validation
    - tenant ops summary endpoint:
      - `GET /v1/ops/summary` (owner/operator only; `viewer` denied)
      - rolling-window counters for run states, dead-letter trigger events, and duration telemetry
    - tenant latency distribution endpoint:
      - `GET /v1/ops/latency-histogram` (owner/operator only; `viewer` denied)
      - fixed run-duration buckets for regression monitoring
    - tenant latency trace endpoint:
      - `GET /v1/ops/latency-traces` (owner/operator only; `viewer` denied)
      - per-run duration samples for regression analysis
    - runbook baseline includes:
      - incident checklist
      - backup/restore drill commands
      - migration rollback workflow
      - soak-check loop using ops summary endpoint
      - perf baseline capture workflow for staged regression checks
      - compliance replay signing-key rotation workflow
    - operator gate tooling:
      - `agntctl ops soak-gate` for threshold checks
      - `agntctl ops perf-gate` for summary/histogram/trace regression checks
      - `agntctl ops capture-baseline` for staged summary/histogram/trace baseline snapshot capture
      - `scripts/ops/soak_gate.sh` for staged repeated checks
      - `scripts/ops/perf_gate.sh` for baseline-vs-candidate regression checks
      - `scripts/ops/capture_perf_baseline.sh` for baseline snapshot automation
      - `scripts/ops/security_gate.sh` for security-critical integration checks
        - deterministic core/skillrunner checks always run
        - DB-backed worker security checks are opt-in (`RUN_DB_SECURITY=1` or `RUN_DB_TESTS=1`)
      - `scripts/ops/release_gate.sh` for pre-release gate workflow
      - `scripts/ops/validate_runbook.sh` for checklist section validation
    - CI now runs:
      - consolidated release gate (`RELEASE_GATE_SKIP_SOAK=0 make release-gate`) which includes:
        - runbook validation
        - workspace verify
        - security integration gate
        - fixture-backed perf gate
        - fixture-backed soak gate
  - M2 schema + DB layer + integration tests (`core/db`, `migrations/0001_init.sql`)
  - M3 NDJSON skill protocol + subprocess runner + Python reference skill
  - M4 worker vertical slice with run leasing + step execution + action policy/execution (`object.write`)
  - M5 API baseline with run create/status/audit endpoints and DB integration tests
  - M5 API capability grant resolver baseline (requested capabilities are now normalized/filtered to policy-authoritative grants)
  - M7 baseline started: API-managed recipe capability bundles with request/bundle intersection in `POST /v1/runs`
  - M7 role-aware policy baseline: optional `x-user-role` preset (`owner`/`operator`/`viewer`) now constrains recipe bundle grants
  - M7 API tenant capacity guardrail baseline:
    - optional `API_TENANT_MAX_INFLIGHT_RUNS` caps queued+running runs per tenant for `POST /v1/runs`
    - over-capacity create requests return `429` (`TENANT_INFLIGHT_LIMITED`)
    - optional `API_TENANT_MAX_TRIGGERS` caps trigger definitions per tenant for trigger create endpoints
    - over-capacity trigger create requests return `429` (`TENANT_TRIGGER_LIMITED`)
  - M7 isolation test baseline expanded:
    - cross-tenant run/audit API access returns `404`
    - cross-tenant trigger mutation routes (`PATCH/disable/fire`) return `404`
    - compliance query/export/verify endpoints are tenant-isolated in API integration coverage
  - M5A messaging baseline with `message.send` execution, local connector outbox persistence, and White Noise relay publish support (`NOSTR_RELAYS`)
  - M5A Slack transport added: `message.send` to `slack:*` now supports webhook delivery when configured
  - M5A Slack reliability update: configurable webhook retries/backoff with dead-letter outbox state on exhaustion
  - M5B signer baseline with pluggable Nostr identity modes (`local_key` default, optional `nip46_signer`) and NIP-46-backed relay publish signing
  - M6 hardening baseline with skill env scrubbing (`env_clear` + allowlist) and redacted action/audit payload persistence
  - M6 sandbox additions: constrained `local.exec` templates with path allowlists and local-first `llm.infer` routing with route-scoped policy grants
  - M6 spend controls: per-run remote `llm.infer` token budget enforcement + estimated cost metadata
  - M6 message destination hardening:
    - optional allowlist gates for `message.send` destinations:
      - `WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST`
      - `WORKER_MESSAGE_SLACK_DEST_ALLOWLIST`
    - when configured, non-allowlisted destinations fail closed
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
  - M6A baseline advanced: durable memory-plane schema + API + worker compaction + query visibility
    - schema/migrations:
      - `memory_records`
      - `memory_compactions`
      - `purge_expired_memory_records(tenant_id, as_of)`
    - core DB APIs:
      - `create_memory_record`
      - `list_tenant_memory_records`
      - `list_tenant_handoff_memory_records`
      - `create_memory_compaction_record`
      - `purge_expired_tenant_memory_records`
    - API endpoints:
      - `POST /v1/memory/records`
      - `GET /v1/memory/records`
      - `POST /v1/memory/handoff-packets`
      - `GET /v1/memory/handoff-packets`
      - `GET /v1/memory/retrieve`
      - `POST /v1/memory/records/purge-expired` (owner only)
    - retrieval baseline:
      - deterministic ranked retrieval payload
      - citation metadata for each item (`memory_id`, `created_at`, `source`, `memory_kind`, `scope`)
    - worker compaction baseline:
      - source rows are compacted via `memory_records.compacted_at`
      - `memory_compactions` lineage rows are created from grouped source records
      - worker emits run-linked `memory.compacted` events using representative run/step context
      - controls:
        - `WORKER_MEMORY_COMPACTION_ENABLED`
        - `WORKER_MEMORY_COMPACTION_MIN_RECORDS`
        - `WORKER_MEMORY_COMPACTION_MAX_GROUPS_PER_CYCLE`
        - `WORKER_MEMORY_COMPACTION_MIN_AGE_SECS`
    - compaction stats endpoint:
      - `GET /v1/memory/compactions/stats` (`owner`/`operator`)
      - counters: `compacted_groups_window`, `compacted_source_records_window`, `pending_uncompacted_records`, `last_compacted_at`
    - purge audit baseline:
      - `POST /v1/memory/records/purge-expired` appends run-linked `memory.purged` audit events
    - redaction-before-indexing baseline:
      - API memory writes now redact JSON/text content before persistence/indexing
      - `redaction_applied` is automatically set when redaction occurs
    - capability baseline:
      - `memory.read`
      - `memory.write`
      - recipe bundle `memory_v1`
    - handoff packet baseline:
      - structured packet writes are persisted as `memory_kind=handoff`
      - packet query filters support `to_agent_id` and `from_agent_id`
      - tenant/role guardrails are covered in API integration tests
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
      - includes reconciliation normalization fields:
        - `settlement_rail`
        - `normalized_outcome`
        - `normalized_error_code`
        - `normalized_error_class`
    - tenant payment summary endpoint:
      - `GET /v1/payments/summary` with filters (`window_secs`, `agent_id`, `operation`)
      - returns aggregate counters and executed spend totals for ops dashboards
    - Cashu planning scaffold captured:
      - `docs/PAYMENTS.md`
      - `docs/ADR/ADR-0008-cashu-rail-planning.md`
    - Cashu execution scaffold baseline:
      - API capability normalization now accepts `cashu:*` scopes
      - recipe bundle `payments_cashu_v1` grants `payment.send` with `cashu:*`
      - worker parses `cashu:<mint_id>` destinations and validates Cashu rail config controls
      - optional deterministic mock execution path is implemented (`PAYMENT_CASHU_MOCK_ENABLED=1`)
      - optional live HTTP execution path is implemented (`PAYMENT_CASHU_HTTP_ENABLED=1`) with HTTPS-by-default and optional auth header/token injection
      - default runtime remains fail-closed when both mock and live HTTP modes are disabled
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
  - Build+test gate baseline implemented:
    - `make verify` runs `cargo build --workspace` then `cargo test`
    - `make verify-db` runs build + DB integration suites (`core`, `api`, `worker`)
    - CI now runs `RELEASE_GATE_SKIP_SOAK=0 make release-gate` before coverage
  - Migration test build reliability update:
    - `api/build.rs`, `core/build.rs`, and `worker/build.rs` now force test recompilation when `migrations/` changes

## Mandatory Read Order (for new sessions)
1. `AGENTS.md`
2. `docs/SESSION_HANDOFF.md` (this file)
3. `docs/NAMING.md`
4. `docs/agent_platform.md`
5. `docs/ARCHITECTURE.md`
6. `docs/SECURITY.md`
7. `docs/POLICY.md`
8. `docs/SECRETS.md`
9. `docs/PAYMENTS.md`
10. `docs/ROADMAP.md`
11. `CHANGELOG.md` (latest entries first)

## Critical ADRs
- `docs/ADR/ADR-0004-shared-postgres-topology.md` (shared DB topology)
- `docs/ADR/ADR-0005-nostr-first-whitenoise.md` (messaging priority)
- `docs/ADR/ADR-0006-sandboxed-local-exec-primitive.md` (sandbox boundary)
- `docs/ADR/ADR-0007-pluggable-nostr-signer-modes.md` (self-hosted + enterprise signer modes)
- `docs/ADR/ADR-0008-cashu-rail-planning.md` (optional Cashu rail planning contract)

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
  - optional destination allowlists:
    - `WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST`
    - `WORKER_MESSAGE_SLACK_DEST_ALLOWLIST`
- Secret reference knobs:
  - `SLACK_WEBHOOK_URL_REF`, `LLM_LOCAL_API_KEY_REF`, `LLM_REMOTE_API_KEY_REF`
  - currently resolved: `env:...`, `file:...`
  - optional CLI adapters (disabled by default): `vault:...`, `aws-sm:...`, `gcp-sm:...`, `azure-kv:...`
  - secret cache controls:
    - `SECUREAGNT_SECRET_CACHE_TTL_SECS` (default `30`, `0` disables cache)
    - `SECUREAGNT_SECRET_CACHE_MAX_ENTRIES` (default `1024`)
  - gate: `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI=1`
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
  - optional tenant capacity guardrail: `API_TENANT_MAX_INFLIGHT_RUNS`
  - optional tenant trigger capacity guardrail: `API_TENANT_MAX_TRIGGERS`
  - usage/compliance query guardrails:
    - `GET /v1/ops/summary` (owner/operator only)
    - `GET /v1/ops/latency-histogram` (owner/operator only)
    - `GET /v1/ops/latency-traces` (owner/operator only)
    - `GET /v1/usage/llm/tokens` (owner/operator only)
    - `GET /v1/payments` (owner/operator only)
    - `GET /v1/audit/compliance` (owner/operator only)
    - `GET /v1/audit/compliance/policy` (owner/operator only)
    - `GET /v1/audit/compliance/verify` (owner/operator only)
    - `GET /v1/audit/compliance/export` (owner/operator only)
    - `GET /v1/audit/compliance/siem/deliveries/summary` (owner/operator only)
    - `GET /v1/audit/compliance/siem/deliveries/slo` (owner/operator only)
    - `PUT /v1/audit/compliance/policy` (owner only)
    - `POST /v1/audit/compliance/purge` (owner only)
- Skill runtime env control:
  - optional `WORKER_SKILL_ENV_ALLOWLIST` (comma-separated env vars passed through to skill process)
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
  - Cashu scaffold knobs (routing/validation active):
    - `PAYMENT_CASHU_ENABLED`
    - `PAYMENT_CASHU_MINT_URIS` / `PAYMENT_CASHU_MINT_URIS_REF`
    - `PAYMENT_CASHU_DEFAULT_MINT`
    - `PAYMENT_CASHU_TIMEOUT_MS`
    - `PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN`
    - `PAYMENT_CASHU_MOCK_ENABLED`
    - `PAYMENT_CASHU_MOCK_BALANCE_MSAT`
    - `PAYMENT_CASHU_HTTP_ENABLED`
    - `PAYMENT_CASHU_HTTP_ALLOW_INSECURE`
    - `PAYMENT_CASHU_AUTH_HEADER`
    - `PAYMENT_CASHU_AUTH_TOKEN` / `PAYMENT_CASHU_AUTH_TOKEN_REF`
  - Cashu execution mode is fail-closed unless `PAYMENT_CASHU_MOCK_ENABLED=1` or `PAYMENT_CASHU_HTTP_ENABLED=1`.
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
make build
make verify
make verify-db
make test-db
make test-worker-db
make test-api-db
make test
make coverage
make coverage-db
make runbook-validate
make soak-gate
make perf-gate
make capture-perf-baseline
make security-gate
RUN_DB_SECURITY=1 make security-gate
make release-gate
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
1. Continue M5C payment hardening: implement live Cashu settlement transport (beyond mock mode) and deeper reconciliation workflows.
2. Continue M8A enterprise audit/compliance implementation: productionize SIEM delivery adapters and expand delivery observability/alerting workflows.
3. Continue M8 production readiness: add action-path tracing/metrics export and alert threshold tuning.
4. Continue M6A durable memory-plane implementation: retrieval quality controls and memory-tier policy refinements.
5. Advance M7 multi-tenancy hardening: deeper tenant isolation tests and quota/index tuning.

## New Session Prompt (copy/paste)
```text
Read AGENTS.md and docs/SESSION_HANDOFF.md first, then docs/NAMING.md, docs/agent_platform.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/POLICY.md, docs/SECRETS.md, docs/PAYMENTS.md, docs/ROADMAP.md, and recent CHANGELOG entries. Summarize current implemented state vs remaining roadmap, then continue with the next unfinished milestone.
```
