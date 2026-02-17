# ROADMAP (SecureAgnt)

This roadmap sequences delivery from scaffold to enterprise-ready platform while preserving the MVP security model and thin vertical-slice focus.

## M0N — Naming and Packaging Migration (SecureAgnt)
Status:
- Completed:
  - product brand moved to `SecureAgnt`
  - primary operator CLI scaffold added as `agntctl`
  - daemon binary alias added as `secureagntd`
  - API binary alias added as `secureagnt-api`
  - docs/handoff now reference SecureAgnt naming conventions
  - runtime env naming baseline finalized:
    - `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`
    - `SECUREAGNT_SKILL_SANDBOXED=1`
  - SecureAgnt systemd packaging templates are now included:
    - `infra/systemd/secureagnt.service`
    - `infra/systemd/secureagnt-api.service`

Scope:
- Complete naming transition across runtime env vars, packaging metadata, and deployment manifests.
- Define final crate/package naming strategy for public release.

Exit criteria:
- Primary docs and operator commands use SecureAgnt naming by default.
- Legacy `AEGIS_*` runtime compatibility paths are removed.

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
- Use one standardized app schema per environment (for example `secureagnt`) in shared Postgres.
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

## M4B — Triggering & Orchestration Plane (Week 3-4)
Status:
- Implemented expanded baseline:
  - interval triggers can be created via `POST /v1/triggers`
  - cron triggers can be created via `POST /v1/triggers/cron` with timezone-aware schedule parsing
  - webhook triggers can be created via `POST /v1/triggers/webhook`
  - webhook events can be enqueued via `POST /v1/triggers/{id}/events`
  - dead-lettered webhook events can be replayed via `POST /v1/triggers/{id}/events/{event_id}/replay`
  - manual/API trigger fire is supported via `POST /v1/triggers/{id}/fire` with deterministic idempotency keys
  - trigger edit and lifecycle mutation APIs are supported:
    - `PATCH /v1/triggers/{id}`
    - `POST /v1/triggers/{id}/enable`
    - `POST /v1/triggers/{id}/disable`
  - worker dispatches due interval triggers and queued webhook trigger events into queued runs
  - worker dispatches due cron triggers into queued runs
  - trigger run ledger (`trigger_runs`) persists run linkage and dedupe keys
  - trigger event queue (`trigger_events`) supports dedupe and dead-letter status
  - trigger mutation audit records persist in `trigger_audit_events`
  - in-flight run guardrails are enforced:
    - per-trigger (`triggers.max_inflight_runs`)
    - per-tenant worker scheduler guardrail (`WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS`)
  - scheduler dispatch HA lease control is implemented:
    - DB lease table `scheduler_leases`
    - worker lease gating knobs (`WORKER_TRIGGER_SCHEDULER_LEASE_*`)
  - trigger schedule jitter is implemented:
    - persisted `triggers.jitter_seconds`
    - applied to interval and cron next-fire calculation paths
  - interval misfire skip policy is implemented (`misfire_policy=skip`)
  - triggered-run provenance now includes `trigger_type` and optional `trigger_event_id`
  - trigger mutation RBAC baseline is enforced in API (`viewer` denied trigger mutation endpoints)
  - trigger mutation ownership guardrail is enforced for operators:
    - `x-user-id` required on operator-trigger mutation endpoints
    - operators can only mutate triggers owned by that same user id

Scope:
- Add first-class run triggers (not ad-hoc shell cron):
  - schedule triggers (`at`, `every`, `cron` with timezone support)
  - event triggers (webhook/hook/message-bus adapters)
  - manual/API triggers with deterministic idempotency keys
- Persist trigger definitions and execution history in DB:
  - trigger specs, next-fire timestamps, misfire policy, retry policy, delivery mode
  - trigger run ledger with status and dedupe metadata
- Add a scheduler/dispatcher service with lease-based claiming and HA-safe coordination.
- Add trigger guardrails:
  - max concurrent runs per trigger/tenant
  - jitter windows and backpressure controls
  - dead-letter queue for repeatedly failing triggers
- Add policy controls for who can create/edit/enable/disable triggers.

Landmarks:
- Trigger definitions survive restarts and support deterministic replay/backfill windows.
- No duplicate run creation under multi-worker scheduler instances.
- Triggered runs carry provenance (`trigger_id`, `trigger_type`, `trigger_event_id`) into audit logs.

Exit criteria:
- Integration tests cover cron accuracy, event-trigger dedupe, misfire recovery, and dead-letter behavior.
- Soak tests validate scheduler correctness under concurrent workers.

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
- Implemented: worker executes `message.send` for `whitenoise:*` and `slack:*` destinations by policy, with local outbox persistence, White Noise relay publish, and Slack webhook delivery transport.
- Implemented: Slack webhook retry/backoff with dead-lettered outbox state when retry budgets are exhausted.

Scope:
- Implement first-class White Noise connector flows (Marmot over Nostr) for `message.send`.
- Implement Slack connector as enterprise-secondary path.
- Add capability scope conventions for channel destinations and payload caps.

Landmarks:
- White Noise delivery path is default in example recipes and demos.
- Slack delivery path is policy-gated and allowlist-scoped.
- Worker can sign and publish Nostr text-note events to configured relays for White Noise destinations.
- Relay publish ACK/failure results are persisted in action result payloads and outbox artifacts.
- Slack webhook delivery success/failure metadata is persisted in action results and outbox artifacts.

Exit criteria:
- Integration tests cover allowed/denied `message.send` for White Noise and Slack destinations.

## M5B — Nostr Signer Modes (Week 3-4)
Status:
- Implemented: worker supports pluggable signer configuration with `local_key` (default) and optional `nip46_signer`, including NIP-46-backed relay publish signing.

Scope:
- Add a signer-provider configuration layer for Nostr identity handling.
- Keep local-key mode available for self-hosted users.
- Support NIP-46 remote signer mode for hardened deployments.

Landmarks:
- Worker startup validates signer mode config.
- Local mode derives `npub` from local secret key (`env` or file).
- NIP-46 mode validates bunker URI + public key identity.
- White Noise `message.send` can publish to relays with either local signing (`local_key`) or remote signing (`nip46_signer`).

Exit criteria:
- Unit tests cover local and NIP-46 config parsing/identity resolution.
- Worker startup surfaces signer mode/public key or explicit disabled warning.
- Integration tests cover end-to-end relay publish in both local-key and NIP-46 signer modes.

## M5C — Agent Payments (Nostr-First, Sats-Native) (Week 4-5)
Status:
- In progress expanded baseline:
  - `payment.send` capability is now policy-recognized (`core/policy`)
  - API capability normalization/validation supports `payment.send` with `nwc:*` scope
  - recipe bundle `payments_v1` grants `payment.send`
  - worker executes `payment.send` in an NWC-first baseline with:
    - required `idempotency_key`
    - supported operations: `pay_invoice`, `make_invoice`, `get_balance`
    - live NIP-47 relay request/response path when `PAYMENT_NWC_URI` (or `PAYMENT_NWC_URI_REF`) is configured
    - wallet-id routing map for NWC URIs:
      - `PAYMENT_NWC_WALLET_URIS` / `PAYMENT_NWC_WALLET_URIS_REF`
      - optional wildcard default route (`*`)
      - missing wallet-id routes fail closed when map mode is in use
    - route orchestration controls:
      - multi-route wallet values (`uri_a|uri_b`)
      - deterministic route strategy option (`PAYMENT_NWC_ROUTE_STRATEGY=deterministic_hash`)
      - explicit failover toggle (`PAYMENT_NWC_ROUTE_FALLBACK_ENABLED`)
      - canary rollout control (`PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT`)
      - route health quarantine controls:
        - `PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD`
        - `PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS`
    - relay timeout control (`PAYMENT_NWC_TIMEOUT_MS`)
    - fail-closed ledgering for NIP-47 transport/response failures
    - optional per-run spend budget guardrail (`PAYMENT_MAX_SPEND_MSAT_PER_RUN`)
    - optional tenant/agent spend budget guardrails (`PAYMENT_MAX_SPEND_MSAT_PER_TENANT`, `PAYMENT_MAX_SPEND_MSAT_PER_AGENT`)
    - optional approval threshold guardrail (`PAYMENT_APPROVAL_THRESHOLD_MSAT`) requiring explicit approval flag
    - mock fallback path when no NWC URI is configured (`nwc_mock`)
  - payment ledger persistence is implemented:
    - `payment_requests` table with tenant idempotency key uniqueness
    - `payment_results` table with execution result/error records
  - tenant payment ledger query endpoint:
    - `GET /v1/payments` (run/agent/status/destination/idempotency filters + latest result)
  - tenant payment summary endpoint:
    - `GET /v1/payments/summary` (window/agent/operation summary counters + executed spend)
  - payment outbox artifacts are persisted under `payments/...`
  - Cashu planning scaffold is documented (full settlement execution not enabled yet):
    - `docs/PAYMENTS.md`
    - `docs/ADR/ADR-0008-cashu-rail-planning.md`
  - Cashu execution scaffold baseline is now implemented:
    - API capability normalization accepts `cashu:*` scopes
    - recipe bundle `payments_cashu_v1` grants `payment.send` with `cashu:*`
    - worker parses `cashu:<mint_id>` destinations and validates Cashu rail config controls:
      - `PAYMENT_CASHU_ENABLED`
      - `PAYMENT_CASHU_MINT_URIS` / `PAYMENT_CASHU_MINT_URIS_REF`
      - `PAYMENT_CASHU_DEFAULT_MINT`
      - `PAYMENT_CASHU_TIMEOUT_MS`
      - `PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN`
    - Cashu settlement execution remains fail-closed with deterministic ledger/audit failures until full rail transport is implemented

Scope:
- Add a policy-gated `payment.send` primitive and typed connector layer.
- Implement Nostr Wallet Connect (NIP-47) connector as the first payment rail:
  - remote wallet command flow for `pay_invoice`, `make_invoice`, `get_balance`
  - encrypted request/response handling and relay policy controls
- Add payment safety controls:
  - idempotency keys for settlement requests
  - per-run/per-agent/per-tenant spend budgets and rate limits
  - optional approval gate for spend above configured thresholds
- Persist payment ledger records (`payment_requests`, `payment_results`) with audit linkage.
- Plan optional Cashu path after NWC baseline (NIP-60 wallet state + NIP-61 nutzaps) for low-friction agent-to-agent micropayments.

Landmarks:
- Agent can request invoice creation and invoice payment through policy-approved actions.
- Worker records payment hash/preimage/fees and final settlement status in action results.
- No direct wallet private keys required on worker hosts for NWC mode.

Exit criteria:
- Integration tests cover allow/deny for `payment.send`, budget enforcement, and idempotent replay behavior.
- Mock NWC relay tests validate encrypted request/response correctness.

## M6 — Security Hardening (Week 4)
Status:
- In progress. Implemented hardening baseline:
  - skill subprocess env scrubbing by default (`env_clear`) with explicit env allowlist support
  - centralized sensitive-value redaction utilities in `core`
  - worker persistence path now redacts action request/result + audit payloads before DB writes
  - integration tests for env containment and redaction behavior
  - sandboxed `local.exec` primitive with template allowlist, path-root constraints, and runtime limits
  - `llm.infer` local-first routing model with separate policy scopes for local vs remote routes
  - remote `llm.infer` per-run token budget guardrail with action-level token/cost accounting metadata
  - `message.send` provider destination allowlist controls with fail-closed enforcement:
    - `WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST`
    - `WORKER_MESSAGE_SLACK_DEST_ALLOWLIST`

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

## M6C — Token Budget Governance (Week 5-6)
Status:
- Completed expanded baseline:
  - remote `llm.infer` per-run token budget guardrail is implemented (`LLM_REMOTE_TOKEN_BUDGET_PER_RUN`)
  - remote budget window guardrails are implemented for:
    - tenant (`LLM_REMOTE_TOKEN_BUDGET_PER_TENANT`)
    - agent (`LLM_REMOTE_TOKEN_BUDGET_PER_AGENT`)
    - model (`LLM_REMOTE_TOKEN_BUDGET_PER_MODEL`)
    - shared window (`LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS`)
  - soft-alert threshold emission is implemented:
    - `LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT`
    - worker audit event `llm.budget.soft_alert` when configured thresholds are reached
  - deterministic remote usage accounting is persisted in `llm_token_usage`
  - tenant-scoped usage query endpoint is implemented:
    - `GET /v1/usage/llm/tokens` (optional `window_secs`, `agent_id`, `model_key`)
  - optional remote token-cost estimation is implemented (`LLM_REMOTE_COST_PER_1K_TOKENS_USD`)

Scope:
- Add broader token-budget governance to prevent unbounded token spend:
  - per-tenant budgets
  - per-agent budgets
  - per-recipe/model route budgets
  - configurable hard-stop and soft-alert thresholds
- Persist token budget events in audit/action metadata for supportability.
- Add operator-visible usage accounting and reset windows.

Landmarks:
- Exceeded token budget paths fail closed with deterministic error codes.
- Budget usage is queryable by tenant/agent over time windows.

Exit criteria:
- Integration tests cover denial on budget exceed, budget reset behavior, and deterministic accounting updates.

## M6B — Secrets Provider Abstraction (Week 5-6)
Status:
- Completed expanded baseline:
  - shared secret reference parser/resolver abstraction is in place (`core/src/secrets.rs`)
  - env/file secret references are supported now
  - Vault/AWS/GCP/Azure CLI-backed adapters are wired behind a fail-closed gate (`SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`)
  - worker/API secret-consuming paths now resolve through the shared resolver (`CliSecretResolver`)
  - runtime TTL secret cache wrapper is implemented and used by API/worker:
    - `SECUREAGNT_SECRET_CACHE_TTL_SECS`
    - `SECUREAGNT_SECRET_CACHE_MAX_ENTRIES`
  - backend version-pin query params are supported for cloud secret references (Vault/AWS/GCP/Azure)
  - rotation-focused resolver tests are implemented (cache hit before TTL, refresh after TTL)
  - provider-adapter integration tests with mocked CLI backends are implemented:
    - Vault version-pin + field extraction
    - AWS provider error propagation
    - Azure version-pin argument path
    - cached resolver version-rollover refresh after TTL

Scope:
- Add a provider-agnostic secrets interface for runtime secret resolution.
- Support multiple backends:
  - local env/file (dev only)
  - HashiCorp Vault
  - AWS Secrets Manager / SSM Parameter Store
  - Google Secret Manager
  - Azure Key Vault
- Store secret references in config/state, not raw secret values.
- Add short-lived caching with TTL, refresh, and version pin support.
- Enforce secret-boundary invariants:
  - secrets never passed to skills
  - redaction in logs/audit payloads
  - explicit capability checks for secret-consuming connectors
- Add rotation and access-failure handling:
  - proactive refresh hooks
  - fail-closed on missing required secrets
  - audit trail for secret fetch failures and version changes.

Landmarks:
- Connectors resolve secrets through one interface regardless of backend.
- Cloud-native auth methods are supported (workload identity/OIDC where available).
- Vault/AppRole or K8s auth flows documented for self-hosted deployments.

Exit criteria:
- Integration tests per provider adapter with mocked backends.
- Rotation test validates zero-downtime secret version rollover for active workers.

## M6A — Durable Memory Plane (Week 5)
Status:
- In progress baseline:
  - memory persistence schema is now implemented:
    - `memory_records`
    - `memory_compactions`
    - purge function `purge_expired_memory_records(tenant_id, as_of)`
  - core DB primitives are now implemented:
    - create/list memory records
    - create compaction records
    - purge expired memory rows
  - API baseline is now implemented:
    - `POST /v1/memory/records`
    - `GET /v1/memory/records`
    - `GET /v1/memory/retrieve`
    - `POST /v1/memory/records/purge-expired` (owner only)
  - retrieval path baseline is now implemented:
    - deterministic ranked response payload
    - citation metadata (`memory_id`, `created_at`, `source`, `memory_kind`, `scope`)
  - memory redaction-before-indexing baseline is now implemented:
    - API memory writes now apply redaction to JSON/text memory content prior to persistence/indexing
    - `redaction_applied` flag is set when automatic redaction occurs (or caller explicitly sets it)
  - worker compaction baseline is now implemented:
    - background compaction pass in worker cycle
    - compaction controls:
      - `WORKER_MEMORY_COMPACTION_ENABLED`
      - `WORKER_MEMORY_COMPACTION_MIN_RECORDS`
      - `WORKER_MEMORY_COMPACTION_MAX_GROUPS_PER_CYCLE`
      - `WORKER_MEMORY_COMPACTION_MIN_AGE_SECS`
    - compaction output:
      - `memory_compactions` entries with source lineage ids
      - source rows marked with `compacted_at`
      - run-linked `memory.compacted` audit events
  - compaction stats baseline is now implemented:
    - `GET /v1/memory/compactions/stats`
  - retention purge audit baseline:
    - run-linked `memory.purged` audit events on memory purge endpoint
  - policy/capability baseline now includes:
    - `memory.read`
    - `memory.write`
    - `memory_v1` recipe bundle defaults
  - integration coverage now validates:
    - memory create/list/purge path
    - memory endpoint role guardrails
    - tenant-scoped DB query and purge behavior

Scope:
- Define memory as retrieval state, not model retraining.
- Add layered memory stores:
  - short-term/session memory: run/step/action traces and compacted summaries
  - long-term semantic memory: tenant/agent-scoped indexed facts and decisions
  - procedural memory: versioned playbooks/policies/skills metadata
- Add memory write/read policies:
  - explicit capability-gated memory writes
  - PII/secrets redaction before indexing
  - retention and deletion controls per tenant
- Add background compaction and summarization jobs to prevent context bloat and preserve recall quality.
- Add inter-agent handoff memory artifacts (structured task packets instead of raw transcript forwarding).

Landmarks:
- Deterministic memory retrieval path with citations/provenance for injected context.
- Memory compaction metrics visible in operations dashboards.
- Group/channel contexts avoid leaking private long-term memory across sessions by policy.

Exit criteria:
- Integration tests cover memory isolation, retention enforcement, and redaction before persistence/indexing.
- Benchmark shows stable retrieval latency under concurrent multi-agent load.

## M7 — Enterprise Multi-Tenancy (Week 5-6)
Status:
- In progress baseline: API-managed recipe capability bundles now gate grants in `POST /v1/runs` (requested capabilities are intersected with recipe policy scope).
- Added role-aware preset baseline: optional `x-user-role` (`owner`, `operator`, `viewer`) further constrains recipe bundle grants.
- Added API tenant in-flight capacity guardrail:
  - `API_TENANT_MAX_INFLIGHT_RUNS`
  - `POST /v1/runs` now fails with `429 TENANT_INFLIGHT_LIMITED` when tenant queued+running capacity is reached
- Added API tenant trigger-capacity guardrail:
  - `API_TENANT_MAX_TRIGGERS`
  - trigger creation endpoints now fail with `429 TENANT_TRIGGER_LIMITED` when tenant trigger capacity is reached
- Added API isolation integration coverage:
  - cross-tenant `GET /v1/runs/{id}` and `GET /v1/runs/{id}/audit` access returns `404`
  - cross-tenant trigger mutation routes (`PATCH/disable/fire`) return `404`
  - compliance endpoints are tenant-isolated:
    - `GET /v1/audit/compliance`
    - `GET /v1/audit/compliance/export`
    - `GET /v1/audit/compliance/verify`
- Added tenant index tuning migration for high-concurrency paths:
  - `migrations/0012_tenant_isolation_indexes.sql`

Scope:
- Add tenant-aware authz and per-tenant scoping across run/step/action/audit operations.
- Add capacity controls and query/index tuning for high concurrency.

Landmarks:
- Tenant boundaries enforced in API and worker query paths.
- Agent/user attribution is complete for operational and audit events.

Exit criteria:
- Isolation tests demonstrate no cross-tenant data access.

## M8 — Production Readiness (Week 7-8)
Status:
- In progress baseline:
  - tenant operational summary endpoint is now implemented:
    - `GET /v1/ops/summary` (owner/operator only)
    - rolling-window counters for queued/running/succeeded/failed runs and dead-letter trigger events
    - rolling-window run duration telemetry (`avg_run_duration_ms`, `p95_run_duration_ms`)
  - API integration coverage now validates:
    - summary counter behavior
    - role guardrail enforcement (`viewer` denied)
  - runbook baseline is expanded with:
    - production incident checklist
    - backup/restore drill commands
    - migration rollback workflow guidance
    - soak-check loop using `GET /v1/ops/summary`
  - operator soak/perf gate baseline is now implemented:
    - `agntctl ops soak-gate` threshold evaluator for `/v1/ops/summary`
    - staging automation script: `scripts/ops/soak_gate.sh`
    - runbook checklist validation script: `scripts/ops/validate_runbook.sh`
    - CI gates now include:
      - `make runbook-validate`
      - fixture-backed `agntctl ops soak-gate` regression check

Scope:
- Add metrics/tracing/logging coverage for run and action paths.
- Finalize runbooks for incident response, backup/restore, migration rollback.
- Add performance baseline and soak checks.

Landmarks:
- Per-run traceability is available end-to-end.
- Operational checklist is complete and repeatable.

Exit criteria:
- Staging soak run completes with no blocker issues.

## M8A — Enterprise Audit and Compliance Plane (Week 7-9)
Status:
- In progress baseline:
  - compliance-plane persistence table is now implemented: `compliance_audit_events`
  - DB trigger-based audit routing is now implemented from `audit_events` to compliance plane
  - baseline compliance routing classes are implemented for:
    - `action.denied`
    - `action.failed`
    - high-risk action telemetry where `payload_json.action_type` is `payment.send` or `message.send` for `action.requested|action.allowed|action.executed`
    - `run.failed`
  - tenant compliance read endpoint is now implemented:
    - `GET /v1/audit/compliance` with `run_id`/`event_type`/`limit` filters
  - tenant compliance export endpoint is now implemented:
    - `GET /v1/audit/compliance/export` with NDJSON output for batch ingestion
  - tamper-evidence baseline is now implemented:
    - per-tenant compliance hash chain fields on `compliance_audit_events`:
      - `tamper_chain_seq`
      - `tamper_prev_hash`
      - `tamper_hash`
    - verification function: `verify_compliance_audit_chain(tenant_id)`
    - API verification endpoint: `GET /v1/audit/compliance/verify`
  - retention/legal-hold baseline is now implemented:
    - policy table: `compliance_audit_policies`
    - policy endpoints:
      - `GET /v1/audit/compliance/policy`
      - `PUT /v1/audit/compliance/policy` (owner only)
    - purge endpoint:
      - `POST /v1/audit/compliance/purge` (owner only)
    - DB purge function:
      - `purge_expired_compliance_audit_events(tenant_id, as_of)`
  - SIEM export adapter baseline is now implemented:
    - endpoint: `GET /v1/audit/compliance/siem/export`
    - adapters:
      - `secureagnt_ndjson`
      - `splunk_hec`
      - `elastic_bulk`
  - deterministic replay package baseline is now implemented:
    - endpoint: `GET /v1/audit/compliance/replay-package`
    - package bundles run status, run-audit events, compliance events, and optional payment ledger
    - includes correlation summary counters and event time bounds
  - replay package manifest signing baseline is now implemented:
    - manifest fields now include:
      - `version`
      - `digest_sha256`
      - `signing_mode`
      - `signature` (when signing key is configured)
    - optional replay signing key controls:
      - `COMPLIANCE_REPLAY_SIGNING_KEY`
      - `COMPLIANCE_REPLAY_SIGNING_KEY_REF`
  - SIEM delivery outbox scaffold is now implemented:
    - table: `compliance_siem_delivery_outbox`
    - queue endpoint:
      - `POST /v1/audit/compliance/siem/deliveries`
    - observability endpoint:
      - `GET /v1/audit/compliance/siem/deliveries`
    - worker delivery cycle claims outbox rows and advances status transitions:
      - `pending -> processing -> delivered|failed|dead_lettered`
    - worker controls:
      - `WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED`
      - `WORKER_COMPLIANCE_SIEM_DELIVERY_BATCH_SIZE`
      - `WORKER_COMPLIANCE_SIEM_DELIVERY_LEASE_MS`
      - `WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_BACKOFF_MS`
      - `WORKER_COMPLIANCE_SIEM_HTTP_ENABLED`
      - `WORKER_COMPLIANCE_SIEM_HTTP_TIMEOUT_MS`
    - local mock delivery targets:
      - `mock://success`
      - `mock://fail`
  - integration coverage added for compliance-plane routing and API role guardrails
  - failure-path coverage added for SIEM queue guardrails and outbox dead-letter transitions

Scope:
- Define two audit planes with explicit schema/event class separation:
  - `Operational Audit` (high-volume, troubleshooting/support)
  - `Compliance Audit` (high-trust, low-volume, control/forensics)
- Strengthen audit durability and trust guarantees:
  - immutable/WORM-capable audit export path
  - hash-chain or signature-based tamper-evidence for audit batches
  - tenant-aware retention windows, legal-hold controls, and purge workflows
- Add enterprise integration paths:
  - SIEM export adapters (batch/stream)
  - alertable compliance event classes for high-risk actions
- Improve operator supportability:
  - actor/session/request correlation enrichment
  - deterministic replay package for incident investigations

Event class baseline:
- Operational Audit classes:
  - run lifecycle (`run.created`, `run.claimed`, `run.completed`, `run.failed`)
  - step lifecycle (`step.started`, `step.completed`, `step.failed`)
  - action execution telemetry (`action.requested`, `action.allowed`, `action.executed`, `action.failed`)
  - connector transport telemetry (relay/webhook delivery metadata)
- Compliance Audit classes:
  - policy and authz decisions for privileged actions (allow/deny + reason)
  - approval gate outcomes (who approved/denied, when, scope)
  - funds movement (`payment.send`) request/result + idempotency lineage
  - external side effects (message delivery to external systems, secret backend access failures)
  - configuration/control-plane mutations (trigger lifecycle, policy changes, role changes)

Retention baseline (defaults, tenant-overridable):
- Operational Audit:
  - hot query window: 30 days
  - archive window: 180 days
- Compliance Audit:
  - hot query window: 180 days
  - archive window: 2555 days (7 years)
  - legal-hold flag prevents purge regardless of retention window

Landmarks:
- Audit records are queryable by tenant/agent/run/action with stable correlation IDs.
- Operational and compliance audit planes are independently queryable/exportable.
- Tamper-evidence verification is documented and test-covered.
- Retention/legal-hold controls are enforceable per tenant policy.

Exit criteria:
- Integration tests cover:
  - audit plane routing/classification
  - audit export
  - retention/legal-hold enforcement
  - tamper-evidence verification
- Operational runbook includes incident-response audit workflows end-to-end.

## M9 — Governance & Supply Chain (Post-MVP)
Scope:
- Signed connector/skill artifacts, version pinning, and approval gates for sensitive actions.
- Provenance and policy workflows for reviewed extension promotion.

Landmarks:
- Verified signature checks in install/enable paths.
- Approval gate workflow for irreversible actions.

Exit criteria:
- Governance controls enforced by policy and covered by tests.

## M10 — Cross-Platform Runtime & Packaging (Very Last Priority)
Status:
- Planned (execute only after all higher milestones are complete).

Scope:
- Validate and support first-class operation on:
  - Ubuntu/Debian
  - Fedora/RHEL-family
  - Arch
  - openSUSE
  - macOS
- Add OS-specific packaging/deployment docs:
  - `systemd` baseline for Linux
  - `launchd` baseline for macOS
- Remove Linux-path assumptions from defaults/docs by adding configurable config/state/log roots.
- Add cross-platform CI matrix coverage for build/test sanity.

Landmarks:
- Platform docs provide reproducible setup for each supported OS family.
- Service supervision templates exist for both Linux and macOS.
- CI catches portability regressions before release.

Exit criteria:
- Verified install/run/test instructions on at least one host per target OS family.
- No blocking portability issues for standard dev + operator workflows.
