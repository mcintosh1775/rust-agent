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
  - trigger enqueue outcomes now distinguish duplicate vs trigger-unavailable states (not found/disabled/type mismatch/schedule-broken)
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
  - trigger failure metadata now uses consistent reason payload fields (`code`, `message`, `reason_class`) for misfire/cron/event-size failure paths
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
- Completed expanded baseline:
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
    - reconciliation normalization fields now included:
      - `settlement_rail`
      - `normalized_outcome`
      - `normalized_error_code`
      - `normalized_error_class`
  - tenant payment summary endpoint:
    - `GET /v1/payments/summary` (window/agent/operation summary counters + executed spend)
  - payment outbox artifacts are persisted under `payments/...`
  - idempotent replay hardening is now implemented:
    - duplicate `payment.send` replay responses include prior request/result metadata
    - duplicate replays no longer overwrite original payment request settlement status
    - integration coverage validates replay behavior preserves single-ledger settlement semantics
    - DB integration coverage validates idempotency keys are tenant-scoped (same key can exist in different tenants)
  - Cashu planning scaffold is documented (full settlement execution not enabled yet):
    - `docs/PAYMENTS.md`
    - `docs/ADR/ADR-0008-cashu-rail-planning.md`
  - Cashu execution scaffold baseline is now implemented:
    - API capability normalization accepts `cashu:*` scopes
    - recipe bundle `payments_cashu_v1` grants `payment.send` with `cashu:*`
    - migration updates payment provider constraint to allow `cashu` ledger rows (`migrations/0016_payment_provider_cashu.sql`)
    - worker parses `cashu:<mint_id>` destinations and validates Cashu rail config controls:
      - `PAYMENT_CASHU_ENABLED`
      - `PAYMENT_CASHU_MINT_URIS` / `PAYMENT_CASHU_MINT_URIS_REF`
      - `PAYMENT_CASHU_DEFAULT_MINT`
      - `PAYMENT_CASHU_TIMEOUT_MS`
      - `PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN`
      - `PAYMENT_CASHU_MOCK_ENABLED`
      - `PAYMENT_CASHU_MOCK_BALANCE_MSAT`
    - optional deterministic mock execution path is implemented for `pay_invoice`, `make_invoice`, and `get_balance`
    - optional live HTTP execution path is implemented (`PAYMENT_CASHU_HTTP_ENABLED=1`) with:
      - endpoint mapping: `pay_invoice`/`make_invoice`/`get_balance`
      - HTTPS-by-default guardrail (`PAYMENT_CASHU_HTTP_ALLOW_INSECURE=0` default)
      - optional auth-header/token injection (`PAYMENT_CASHU_AUTH_HEADER`, `PAYMENT_CASHU_AUTH_TOKEN(_REF)`)
      - normalized result payload fields for reconciliation (`payment_hash`, `payment_preimage`, `fee_msat`, `invoice`, `balance_msat`)
      - `get_balance` uses explicit `GET /v1/balance` transport semantics
    - Cashu route orchestration parity is now implemented:
      - multi-route mint values (`uri_a|uri_b`)
      - deterministic route strategy option (`PAYMENT_CASHU_ROUTE_STRATEGY=deterministic_hash`)
      - explicit failover toggle (`PAYMENT_CASHU_ROUTE_FALLBACK_ENABLED`)
      - canary rollout control (`PAYMENT_CASHU_ROUTE_ROLLOUT_PERCENT`)
      - route health quarantine controls:
        - `PAYMENT_CASHU_ROUTE_HEALTH_FAIL_THRESHOLD`
        - `PAYMENT_CASHU_ROUTE_HEALTH_COOLDOWN_SECS`
      - route metadata now persists in Cashu payment results (`result.route`)
      - integration coverage validates Cashu route failover enabled/disabled behavior
    - default runtime remains fail-closed when both mock and live HTTP modes are disabled
  - milestone sign-off automation is now included:
    - `scripts/ops/m5c_signoff.sh`
    - Makefile target: `make m5c-signoff`

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
- Completed expanded baseline:
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
  - security integration gate profile is now implemented:
    - deterministic core/skillrunner security checks in all environments
    - opt-in DB-backed worker security checks (`RUN_DB_SECURITY=1` or `RUN_DB_TESTS=1`)
  - milestone sign-off automation is now included:
    - `scripts/ops/m6_signoff.sh`
    - Makefile target: `make m6-signoff`

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
- Completed expanded baseline:
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
    - `POST /v1/memory/handoff-packets`
    - `GET /v1/memory/handoff-packets`
    - `GET /v1/memory/retrieve`
    - `POST /v1/memory/records/purge-expired` (owner only)
  - retrieval path baseline is now implemented:
    - deterministic ranked response payload
    - citation metadata (`memory_id`, `created_at`, `source`, `memory_kind`, `scope`)
    - expired memory records are excluded from list/retrieve query paths before purge
  - retrieval quality controls are now implemented:
    - optional query-time ranking/filter knobs on `GET /v1/memory/retrieve`:
      - `query_text`
      - `min_score`
      - `source_prefix`
      - `require_summary`
    - retrieval responses now include per-item `score` plus query/filter echo fields
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
    - handoff packet create/list filters + tenant/role guardrails
    - memory endpoint role guardrails
    - tenant-scoped DB query and purge behavior
    - concurrent memory retrieval benchmark with tenant isolation and bounded-latency threshold (`MEMORY_RETRIEVAL_BENCH_MAX_MS`)
  - milestone sign-off automation is now included:
    - `scripts/ops/m6a_signoff.sh`
    - Makefile target: `make m6a-signoff`

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
- Completed expanded baseline: API-managed recipe capability bundles now gate grants in `POST /v1/runs` (requested capabilities are intersected with recipe policy scope).
- Added role-aware preset baseline: optional `x-user-role` (`owner`, `operator`, `viewer`) further constrains recipe bundle grants.
- Added worker artifact filesystem isolation baseline:
  - worker side-effect artifacts now write under tenant-scoped roots (`<artifact_root>/tenants/<tenant_id>/...`)
  - integration coverage validates same relative artifact paths across two tenants do not collide on disk
  - worker message outbox tenant isolation coverage validates cross-tenant `message.send` artifacts do not collide on shared workers
- Added tenant isolation regression gate tooling:
  - `scripts/ops/isolation_gate.sh`
  - Makefile target: `make isolation-gate`
  - validation-gate DB profile now executes isolation checks before full DB suites
- Added API tenant in-flight capacity guardrail:
  - `API_TENANT_MAX_INFLIGHT_RUNS`
  - `POST /v1/runs` now fails with `429 TENANT_INFLIGHT_LIMITED` when tenant queued+running capacity is reached
- Added API tenant trigger-capacity guardrail:
  - `API_TENANT_MAX_TRIGGERS`
  - trigger creation endpoints now fail with `429 TENANT_TRIGGER_LIMITED` when tenant trigger capacity is reached
- Added API tenant memory-capacity guardrail:
  - `API_TENANT_MAX_MEMORY_RECORDS`
  - memory write endpoints (`POST /v1/memory/records`, `POST /v1/memory/handoff-packets`) now fail with `429 TENANT_MEMORY_LIMITED` when active record capacity is reached
- Added API isolation integration coverage:
  - cross-tenant `GET /v1/runs/{id}` and `GET /v1/runs/{id}/audit` access returns `404`
  - cross-tenant trigger mutation routes (`PATCH/disable/fire`) return `404`
  - compliance endpoints are tenant-isolated:
    - `GET /v1/audit/compliance`
    - `GET /v1/audit/compliance/export`
    - `GET /v1/audit/compliance/verify`
- Added tenant index tuning migration for high-concurrency paths:
  - `migrations/0012_tenant_isolation_indexes.sql`
- Milestone sign-off automation is now included:
  - `scripts/ops/m7_signoff.sh`
  - Makefile target: `make m7-signoff`

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
- Completed expanded baseline:
  - tenant operational summary endpoint is now implemented:
    - `GET /v1/ops/summary` (owner/operator only)
    - rolling-window counters for queued/running/succeeded/failed runs and dead-letter trigger events
    - rolling-window run duration telemetry (`avg_run_duration_ms`, `p95_run_duration_ms`)
  - tenant latency distribution endpoint is now implemented:
    - `GET /v1/ops/latency-histogram` (owner/operator only)
    - fixed run-duration buckets for dashboarding and regression checks
  - tenant latency trace endpoint is now implemented:
    - `GET /v1/ops/latency-traces` (owner/operator only)
    - rolling-window per-run duration samples for regression analysis
  - tenant action-latency endpoint is now implemented:
    - `GET /v1/ops/action-latency` (owner/operator only)
    - action-type aggregates (`avg`/`p95`/`max`, `failed_count`, `denied_count`)
  - tenant action-latency trace endpoint is now implemented:
    - `GET /v1/ops/action-latency-traces` (owner/operator only)
    - rolling-window per-action samples (`action_request_id`, `run_id`, `step_id`, `action_type`, `status`, `duration_ms`)
  - API integration coverage now validates:
    - summary counter behavior
    - latency histogram bucket behavior
    - latency trace sample endpoint behavior
    - role guardrail enforcement (`viewer` denied)
  - runbook baseline is expanded with:
    - production incident checklist
    - backup/restore drill commands
    - migration rollback workflow guidance
    - soak-check loop using `GET /v1/ops/summary`
    - perf baseline capture workflow for staged regression gates
  - operator soak/perf gate baseline is now implemented:
    - `agntctl ops soak-gate` threshold evaluator for `/v1/ops/summary`
      - optional per-action p95 threshold via `--max-action-p95-ms` and `/v1/ops/action-latency`
      - optional per-action failed/denied rate thresholds via:
        - `--max-action-failed-rate-pct`
        - `--max-action-denied-rate-pct`
      - optional local fixture input is now supported for summary payloads:
        - `--summary-json`
    - `agntctl ops perf-gate` regression evaluator for summary + latency histogram + latency trace deltas
    - `agntctl ops capture-baseline` snapshot capture for summary + latency histogram + latency traces baseline JSON
    - staging automation script: `scripts/ops/soak_gate.sh`
    - staging automation script: `scripts/ops/perf_gate.sh`
    - staging automation script: `scripts/ops/capture_perf_baseline.sh`
    - Makefile capture target: `make capture-perf-baseline`
    - validation gate script: `scripts/ops/validation_gate.sh`
    - Makefile validation gate target: `make validation-gate`
    - release gate script: `scripts/ops/release_gate.sh`
    - security gate script: `scripts/ops/security_gate.sh`
      - core/skillrunner security checks run by default
      - DB-backed worker security checks are opt-in (`RUN_DB_SECURITY=1` or `RUN_DB_TESTS=1`)
    - Makefile security gate target: `make security-gate`
    - Makefile release gate target: `make release-gate`
    - milestone sign-off automation is now included:
      - `scripts/ops/m8_signoff.sh`
      - Makefile target: `make m8-signoff`
    - runbook checklist validation script: `scripts/ops/validate_runbook.sh`
    - CI gates now include:
      - consolidated release gate (`RELEASE_GATE_SKIP_SOAK=0 make release-gate`) including:
        - runbook validation
        - workspace verify
        - security integration gate
        - fixture-backed perf regression gate
        - fixture-backed soak regression gate
    - validation gate profile supports optional DB suite and coverage passes:
      - `VALIDATION_GATE_RUN_DB_SUITES=1`
      - `VALIDATION_GATE_RUN_COVERAGE=1`

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
- Completed expanded baseline:
  - compliance-plane persistence table is now implemented: `compliance_audit_events`
  - DB trigger-based audit routing is now implemented from `audit_events` to compliance plane
  - baseline compliance routing classes are implemented for:
    - `action.denied`
    - `action.failed`
    - high-risk action telemetry where `payload_json.action_type` is `payment.send` or `message.send` for `action.requested|action.allowed|action.executed`
    - `run.failed`
  - tenant compliance read endpoint is now implemented:
    - `GET /v1/audit/compliance` with `run_id`/`event_type`/`limit` filters
    - response now includes optional correlation fields when present in routed payloads:
      - `request_id`
      - `session_id`
      - `action_request_id`
      - `payment_request_id`
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
    - runbook rotation workflow now documents version-pinned key cutover + rollback checks
  - SIEM delivery outbox scaffold is now implemented:
    - table: `compliance_siem_delivery_outbox`
    - queue endpoint:
      - `POST /v1/audit/compliance/siem/deliveries`
    - observability endpoint:
      - `GET /v1/audit/compliance/siem/deliveries`
      - `GET /v1/audit/compliance/siem/deliveries/summary`
      - `GET /v1/audit/compliance/siem/deliveries/slo`
      - `GET /v1/audit/compliance/siem/deliveries/targets`
      - `GET /v1/audit/compliance/siem/deliveries/alerts`
    - operator replay endpoint:
      - `POST /v1/audit/compliance/siem/deliveries/{id}/replay`
    - worker delivery cycle claims outbox rows and advances status transitions:
      - `pending -> processing -> delivered|failed|dead_lettered`
      - non-retryable failure classes now dead-letter immediately (no retry backoff):
        - HTTP: `400`, `401`, `403`, `404`, `405`, `410`, `422`
        - unsupported target/configuration errors
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
    - local mock delivery targets:
      - `mock://success`
      - `mock://fail`
  - compliance durability gate tooling is now implemented:
    - `agntctl ops compliance-gate`
    - `scripts/ops/compliance_gate.sh`
    - Makefile target: `make compliance-gate`
    - validation gate integration with fixture-backed compliance inputs
    - per-target SIEM threshold checks in compliance gate:
      - target hard-failure rate
      - target dead-letter rate
      - target pending-count pressure
  - integration coverage added for compliance-plane routing and API role guardrails
  - failure-path coverage added for SIEM queue guardrails and outbox dead-letter transitions
  - milestone sign-off automation is now included:
    - `scripts/ops/m8a_signoff.sh`
    - Makefile target: `make m8a-signoff`

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
Status:
- Completed expanded baseline:
  - release artifact manifest tooling is now implemented:
    - `scripts/ops/generate_release_manifest.sh`
    - `scripts/ops/verify_release_manifest.sh`
    - Makefile targets:
      - `make release-manifest`
      - `make release-manifest-verify`
  - deployment preflight scaffold is now implemented:
    - `scripts/ops/deploy_preflight.sh`
    - Makefile target:
      - `make deploy-preflight`
  - governance enforcement gate is now implemented:
    - `scripts/ops/governance_gate.sh`
    - Makefile target:
      - `make governance-gate`
    - validation/release gate integration:
      - `VALIDATION_GATE_RUN_GOVERNANCE` (default enabled)
      - `RELEASE_GATE_RUN_GOVERNANCE` pass-through
    - governance gate workflow:
      - generates release manifest
      - verifies release manifest
      - runs deploy preflight with manifest verification enabled
  - worker governance approval gate baseline is now implemented:
    - `WORKER_APPROVAL_REQUIRED_ACTION_TYPES` enforces explicit approval flags for configured irreversible action types
    - missing approval is fail-closed with denied action result (`reason=approval_required`)
  - worker skill provenance baseline is now implemented:
    - `WORKER_SKILL_SCRIPT_SHA256` enforces configured skill script digest matching before invoke
    - digest mismatch fails run/step before privileged action execution
  - milestone sign-off automation is now included:
    - `scripts/ops/m9_signoff.sh`
    - Makefile target: `make m9-signoff`

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
- In progress prep baseline (full runtime validation still deferred until higher milestones complete):
  - containerized runtime stack baseline is now included:
    - API runtime image: `infra/containers/Dockerfile.api`
    - worker runtime image: `infra/containers/Dockerfile.worker`
    - compose `stack` profile (`infra/containers/compose.yml`) for Postgres + API + worker
    - API container migration bootstrap control (`API_RUN_MIGRATIONS=1`)
    - Makefile stack targets:
      - `make stack-build`
      - `make stack-up`
      - `make stack-up-build`
      - `make stack-down`
      - `make stack-ps`
      - `make stack-logs`
    - build throttle control:
      - `SECUREAGNT_CARGO_BUILD_JOBS` (default `2`)
  - macOS launchd service templates are now included:
    - `infra/launchd/secureagnt.plist`
    - `infra/launchd/secureagnt-api.plist`
  - baseline shared config template is now included:
    - `infra/config/secureagnt.yaml`
  - M10 sign-off scaffold baseline is now included:
    - script: `scripts/ops/m10_signoff.sh`
    - Makefile target: `make m10-signoff`
    - portability doc baseline: `docs/CROSS_PLATFORM.md`
  - deployment preflight portability checks are expanded:
    - `scripts/ops/deploy_preflight.sh` now supports optional compose config validation:
      - `DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1`
    - `docs/CROSS_PLATFORM.md` now includes explicit portability signoff checklist and preflight command sequence
  - M10 matrix/self-check baseline added:
    - script: `scripts/ops/m10_matrix_gate.sh`
    - Makefile target: `make m10-matrix-gate`
    - CI portability matrix job (`ubuntu-latest`, `macos-latest`) runs the matrix gate
    - execution evidence template added:
      - `docs/M10_EXECUTION_CHECKLIST.md`

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

## M11 — Web Operations Console (Post-MVP)
Status:
- Completed M11A baseline:
  - M11A implementation plan is now documented:
    - `docs/M11A_PLAN.md`
  - API-served console shell route is now implemented:
    - `GET /console`
    - lightweight HTML/CSS/JS dashboard served directly by `secureagnt-api`
  - console baseline is wired to existing read-only APIs:
    - `/v1/ops/summary`
    - `/v1/ops/latency-histogram`
    - `/v1/ops/action-latency`
    - `/v1/usage/llm/tokens`
    - `/v1/payments/summary`
    - `/v1/audit/compliance/siem/deliveries/slo`
  - API integration coverage includes console route availability/content type checks
- Completed M11B baseline:
  - console role selector now includes `viewer` in addition to `owner`/`operator`
  - per-panel RBAC handling is explicit in the UI:
    - role-restricted panels render structured `ROLE_FORBIDDEN` state instead of opaque failures
    - API `403` responses are rendered as panel-level `FORBIDDEN` states
  - console shell now documents required RBAC headers inline
  - API integration coverage asserts M11B shell RBAC markers are present
- Completed M11C baseline:
  - console drill-down panels now include:
    - run latency traces (`/v1/ops/latency-traces`)
    - action latency traces (`/v1/ops/action-latency-traces`)
    - run detail (`/v1/runs/:id`)
    - run audit (`/v1/runs/:id/audit`)
    - payments ledger (`/v1/payments`)
    - compliance alerts (`/v1/audit/compliance/siem/deliveries/alerts`)
  - console supports run-context focused refresh (`Load Run Context`) for detail/audit panels
  - operator filters persist in browser local storage (`secureagnt_console_controls_v1`)
  - run trace panel can auto-populate `run-id` from the latest trace entry
- Completed M11D baseline:
  - console threshold chips now surface warning/critical posture for run failures, run latency, token burn, payment failures, and SIEM delivery failure rates
  - console export actions added:
    - `Export Snapshot JSON`
    - `Export Health JSON`
  - API integration coverage now asserts console shell role/error/export markers:
    - `ROLE_FORBIDDEN`
    - `FORBIDDEN`
    - `FETCH_FAILED`
    - `INPUT_REQUIRED`
- M11 baseline status:
  - completed (M11A through M11D)
- Completed M11E auth-boundary hardening baseline:
  - API now supports trusted proxy auth enforcement for role/user header flows:
    - `API_TRUSTED_PROXY_AUTH_ENABLED`
    - `API_TRUSTED_PROXY_SHARED_SECRET` / `API_TRUSTED_PROXY_SHARED_SECRET_REF`
  - when enabled, role-scoped requests require `x-auth-proxy-token` (`401` on missing/invalid token)
  - console now includes optional `Auth Proxy Token` control so `/console` panel fetches can operate in trusted-proxy mode
- Completed M11F alert-ack workflow baseline:
  - API alert acknowledgment endpoint added:
    - `POST /v1/audit/compliance/siem/deliveries/alerts/ack`
  - alert rows now include acknowledgment metadata:
    - `acknowledged`
    - `acknowledged_at`
    - `acknowledged_by_user_id`
    - `acknowledged_by_role`
    - `acknowledgement_note`
  - console now supports alert acknowledgment actions with user-id attribution and optional run-scoped acknowledgement
  - integration coverage validates ack role/header guardrails and alert-state propagation
- Completed M11G LLM lane-visibility baseline:
  - new tenant ops endpoint added:
    - `GET /v1/ops/llm-gateway` (`owner`/`operator`, `viewer` denied)
  - endpoint returns lane-scoped LLM gateway metrics:
    - request class totals
    - avg/p95 latency
    - cache hit and distributed cache hit counters/rates
    - verifier escalation counters/rates
    - SLO warn/breach counters
    - distributed fail-open admission counters
  - console now includes an `LLM Gateway Lanes` panel using `/v1/ops/llm-gateway`
  - console threshold posture now includes LLM gateway SLO and verifier pressure signals
  - integration coverage validates lane endpoint behavior and role guardrails
- Completed M11H operator action workflow baseline:
  - console now includes heartbeat materialization controls:
    - `Preview Heartbeat Plan`
    - `Apply Heartbeat Plan`
  - action wiring uses:
    - `POST /v1/agents/{id}/heartbeat/materialize`
    - apply mode sends explicit approval confirmation and `x-user-id` attribution
  - console panel added for heartbeat materialization results:
    - `/v1/agents/:id/heartbeat/materialize`
  - console health export now includes latest heartbeat materialization payload
  - integration coverage asserts M11H console action markers are present in console shell
- Completed M11I bootstrap-console workflow baseline:
  - console now includes bootstrap actions:
    - `Load Bootstrap`
    - `Complete Bootstrap`
  - action wiring uses:
    - `GET /v1/agents/{id}/bootstrap`
    - `POST /v1/agents/{id}/bootstrap/complete`
  - console panel added for bootstrap status:
    - `/v1/agents/:id/bootstrap`
  - console health export now includes latest bootstrap payload
  - integration coverage asserts M11I console action markers are present in console shell

Scope:
- Add a web interface for operator workflows:
  - agent and worker health/status
  - run queue depth and failure trends
  - LLM token usage/burn visibility and budget pressure
  - payment/compliance delivery health snapshots
- Reuse existing API endpoints first, then add UI-focused API surfaces only where needed.
- Keep RBAC-aligned views (`owner`/`operator`/`viewer`) consistent with API role policy.

Landmarks:
- Dashboard page shows live tenant health summary from `/v1/ops/*`.
- Token spend page shows remote usage trends from `/v1/usage/llm/tokens`.
- Compliance/payment widgets surface delivery failures and dead-letter pressure.

Exit criteria:
- Operators can monitor fleet health and token burn from one UI without using direct API calls.
- UI has integration coverage for role-guarded data visibility.

## M12 — Agent Context Profile (Post-MVP)
Status:
- Completed M12A planning baseline:
  - agent files profile documented:
    - `docs/AGENT_FILES.md`
  - architectural decision recorded:
    - `docs/ADR/ADR-0009-agent-context-files-profile.md`
  - canonical file set and precedence are now explicit (`AGENTS.md` / `TOOLS.md` / `IDENTITY.md` / `SOUL.md` / `USER.md` / `MEMORY.md` / `HEARTBEAT.md` / `BOOTSTRAP.md`)
  - mutability model is defined (human-admin controlled vs agent-managed artifacts)
  - heartbeat intent model is defined as trigger-governed workflow, not direct privileged execution
- Completed M12B runtime loader baseline:
  - typed context loader/validator implemented:
    - `core/src/agent_context.rs`
  - worker runtime integration implemented:
    - context load + skill input injection under `agent_context`
    - context audit events:
      - `agent.context.loaded`
      - `agent.context.not_found`
      - `agent.context.error`
  - worker controls added:
    - `WORKER_AGENT_CONTEXT_ENABLED`
    - `WORKER_AGENT_CONTEXT_REQUIRED`
    - `WORKER_AGENT_CONTEXT_ROOT`
    - `WORKER_AGENT_CONTEXT_REQUIRED_FILES`
    - `WORKER_AGENT_CONTEXT_MAX_FILE_BYTES`
    - `WORKER_AGENT_CONTEXT_MAX_TOTAL_BYTES`
    - `WORKER_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR`
  - compose stack support added:
    - worker env passthrough for context controls
    - read-only host mount `../../agent_context:/var/lib/secureagnt/agent-context:ro`
  - bootstrap tooling added:
    - `scripts/ops/init_agent_context.sh`
    - `make agent-context-init`
  - integration coverage added:
    - worker succeeds with loaded required profile
    - worker fails when profile is required but missing
- Completed M12C control-plane enforcement and inspection baseline:
  - core agent-context module now includes:
    - mutability classifier (`immutable`, `human_primary`, `agent_managed`)
    - heartbeat intent compiler with typed candidate/issue output and cron/timezone validation
    - canonical context-summary digest helper (`summary_digest_sha256`)
  - API operator tooling added:
    - `GET /v1/agents/{id}/context`
    - `POST /v1/agents/{id}/heartbeat/compile`
  - API context mutation path added with fail-closed gate:
    - `POST /v1/agents/{id}/context`
    - disabled by default unless `API_AGENT_CONTEXT_MUTATION_ENABLED=1`
    - enforced mutability boundaries:
      - immutable: `AGENTS.md`, `TOOLS.md`, `IDENTITY.md`, `SOUL.md` (always denied)
      - human-primary: `USER.md`, `HEARTBEAT.md`, `BOOTSTRAP.md` (owner only)
      - agent-managed: `MEMORY.md`, `memory/*.md`, `sessions/*.jsonl` (owner/operator)
      - `sessions/*.jsonl` append-only
  - integration coverage added for:
    - context inspect endpoint
    - heartbeat compile endpoint
    - mutability enforcement and append-only session guardrails
- Completed M12D heartbeat materialization baseline:
  - API governed materialization endpoint added:
    - `POST /v1/agents/{id}/heartbeat/materialize`
  - supports plan-only and apply modes:
    - `apply=false` returns candidate plan without side effects
    - `apply=true` requires explicit approval confirmation and user attribution (`x-user-id`)
  - materialization behavior:
    - compile issues fail-closed for apply mode
    - matching existing schedules are detected and skipped
    - trigger audit provenance emitted (`trigger.materialized`)
  - integration coverage added for:
    - approval gate enforcement
    - trigger creation
    - idempotent re-apply skip behavior
- Completed M12E bootstrap workflow baseline:
  - dedicated bootstrap control-plane endpoints added:
    - `GET /v1/agents/{id}/bootstrap`
    - `POST /v1/agents/{id}/bootstrap/complete`
  - bootstrap completion event contract added:
    - append-only status record at `sessions/bootstrap.status.jsonl`
  - owner-attributed completion controls:
    - requires `x-user-id`
    - supports optional initial file writes (`IDENTITY.md`, `SOUL.md`, `USER.md`, `HEARTBEAT.md`)
    - optional replay with `force=true`
  - rollout posture controls:
    - `API_AGENT_BOOTSTRAP_ENABLED=1` default (solo/dev)
    - enterprise profile disables bootstrap API by default (`API_AGENT_BOOTSTRAP_ENABLED=0`)
  - context scaffold utility now includes `BOOTSTRAP.md` template:
    - `scripts/ops/init_agent_context.sh`
  - integration coverage added for:
    - bootstrap pending/completed status transitions
    - disabled mode behavior
    - role guardrails

Scope:
- Keep M12 controls deterministic and policy-safe as more agent-context automation is added.

Landmarks:
- Effective context can be inspected/debugged per run without exposing secrets.
- Conflicting context directives resolve deterministically via precedence rules.
- Heartbeat-generated schedules are auditable and policy-gated.

Exit criteria:
- Integration coverage validates precedence, mutability enforcement, and heartbeat compile guardrails.
- Ops docs define how to bootstrap and validate agent context files in production.

## M13 — Operations Excellence Documentation (Post-MVP)
Status:
- Completed M13A manual baseline:
  - comprehensive operations manual published:
    - `docs/OPERATIONS_MANUAL.md`
  - existing concise guides now explicitly reference the manual:
    - `docs/OPERATIONS.md`
    - `docs/RUNBOOK.md`
  - docs index updated:
    - `docs/README.md`
- Completed M13B synchronization pass:
  - operations manual now includes explicit agent-context validation procedure and hardened API context controls
  - API/development/operations/quickstart docs now include:
    - agent-context inspect endpoint usage
    - heartbeat compile endpoint usage
    - mutation endpoint guardrails and opt-in posture
- Completed M13C appendices baseline:
  - operations manual now includes environment-specific escalation rosters:
    - solo/dev
    - team/self-hosted
    - enterprise production
  - operations manual now includes standardized change-ticket templates:
    - standard planned change
    - emergency change
  - reusable templates are now published:
    - `docs/templates/ESCALATION_ROSTER_TEMPLATE.md`
    - `docs/templates/CHANGE_TICKET_TEMPLATE.md`

Scope:
- Define day-0/day-1/day-2 operator workflows with deterministic procedures.
- Standardize incident, change, and release operations documentation.
- Keep manual synchronized with capability, policy, and compliance milestones.

Landmarks:
- Operators can run installation, validation, incident response, and release workflows from one manual.
- Manual captures security/compliance posture and escalation expectations.

Exit criteria:
- Manual covers production topology, controls, incident classes, DR, release gates, and tenant lifecycle operations.
- Roadmap/handoff/changelog track manual updates as first-class operational deliverables.

## M14 — LLM Gateway and Tiered Model Routing (Post-MVP)
Status:
- Completed:
  - completed M14A baseline:
    - gateway decision contract added to `llm.infer` results (`gateway.*`)
    - deterministic route reason codes implemented (`mode_*`, `local_first_*`, `prefer_remote_local_first`)
    - remote egress classification gate implemented:
      - `LLM_REMOTE_EGRESS_CLASS=cloud_allowed|redacted_only|never_leaves_prem`
      - `redacted_only` requires `llm.infer` args `redacted=true`
    - dual deployment profile presets added:
      - `infra/config/profile.solo-dev.env`
      - `infra/config/profile.enterprise.env`
    - profile env pass-through wired in container stack:
      - `infra/containers/compose.yml`
  - completed M14B baseline:
    - worker run-claim prioritization now supports queue lanes:
      - `interactive` lane prioritized for low-latency work
      - `batch` lane deprioritized with anti-starvation aging
      - lane source keys in run input: `queue_class` or `llm_queue_class`
    - `llm.infer` gateway metadata now includes request lane labels:
      - `gateway.request_class`
      - `gateway.queue_lane`
  - completed M14C baseline:
    - large-input policy engine added for `llm.infer`:
      - `direct`
      - `summarize_first`
      - `chunk_and_retrieve`
      - `escalate_remote`
    - configurable thresholds and budgets:
      - `LLM_MAX_INPUT_BYTES`
      - `LLM_LARGE_INPUT_THRESHOLD_BYTES`
      - `LLM_LARGE_INPUT_SUMMARY_TARGET_BYTES`
    - gateway metadata now includes preprocessing reason/audit fields:
      - `gateway.large_input_policy`
      - `gateway.large_input_applied`
      - `gateway.large_input_reason_code`
      - `gateway.prompt_bytes_original`
      - `gateway.prompt_bytes_effective`
  - completed M14D baseline:
    - code/context retrieval guardrails added to `llm.infer`:
      - optional `context_documents` + `context_query`
      - top-k context selection and bounded prompt packing
      - chunk-retrieval fallback for oversized prompt payloads
    - configurable retrieval controls:
      - `LLM_CONTEXT_RETRIEVAL_TOP_K`
      - `LLM_CONTEXT_RETRIEVAL_MAX_BYTES`
      - `LLM_CONTEXT_RETRIEVAL_CHUNK_BYTES`
    - retrieval telemetry added to gateway metadata:
      - `gateway.retrieval_candidate_documents`
      - `gateway.retrieval_selected_documents`
  - completed M14E baseline:
    - gateway admission controls added:
      - `LLM_ADMISSION_ENABLED`
      - `LLM_ADMISSION_INTERACTIVE_MAX_INFLIGHT`
      - `LLM_ADMISSION_BATCH_MAX_INFLIGHT`
    - gateway response cache added (namespace-scoped keying):
      - `LLM_CACHE_ENABLED`
      - `LLM_CACHE_TTL_SECS`
      - `LLM_CACHE_MAX_ENTRIES`
      - gateway metadata:
        - `cache_status`
        - `cache_key_sha256`
    - heuristic verifier escalation added:
      - `LLM_VERIFIER_ENABLED`
      - `LLM_VERIFIER_MIN_SCORE_PCT`
      - `LLM_VERIFIER_ESCALATE_REMOTE`
      - `LLM_VERIFIER_MIN_RESPONSE_CHARS`
      - gateway metadata:
        - `verifier_enabled`
        - `verifier_score_pct`
        - `verifier_threshold_pct`
        - `verifier_escalated`
        - `verifier_reason_code`
  - completed M14F baseline:
    - optional distributed gateway controls added (Postgres-backed; default off for solo/small deployments):
      - `LLM_DISTRIBUTED_ENABLED`
      - `LLM_DISTRIBUTED_FAIL_OPEN`
      - `LLM_DISTRIBUTED_OWNER`
      - `LLM_DISTRIBUTED_ADMISSION_ENABLED`
      - `LLM_DISTRIBUTED_ADMISSION_LEASE_MS`
      - `LLM_DISTRIBUTED_CACHE_ENABLED`
      - `LLM_DISTRIBUTED_CACHE_NAMESPACE_MAX_ENTRIES`
    - distributed persistence added:
      - `llm_gateway_admission_leases`
      - `llm_gateway_cache_entries`
    - gateway metadata/status now differentiates local vs distributed paths:
      - admission (`admitted`, `distributed_admitted`, `distributed_fail_open_local`)
      - cache (`hit|miss` and `distributed_hit|distributed_miss`)
  - completed M14G baseline:
    - verifier mode framework added:
      - `LLM_VERIFIER_MODE=heuristic|deterministic|model_judge|hybrid`
    - deterministic verifier reason-code path implemented for maintainable policy-style checks.
    - optional model-judge endpoint support added:
      - `LLM_VERIFIER_JUDGE_BASE_URL`
      - `LLM_VERIFIER_JUDGE_MODEL`
      - `LLM_VERIFIER_JUDGE_API_KEY` / `LLM_VERIFIER_JUDGE_API_KEY_REF`
      - `LLM_VERIFIER_JUDGE_TIMEOUT_MS`
      - `LLM_VERIFIER_JUDGE_FAIL_OPEN`
    - `llm.infer` gateway metadata now includes:
      - `gateway.verifier_mode`
      - `gateway.verifier_judge_score_pct`
  - completed M14H baseline:
    - lane-SLO controls added:
      - `LLM_SLO_INTERACTIVE_MAX_LATENCY_MS`
      - `LLM_SLO_BATCH_MAX_LATENCY_MS`
      - `LLM_SLO_ALERT_THRESHOLD_PCT`
      - `LLM_SLO_BREACH_ESCALATE_REMOTE`
    - `llm.infer` gateway metadata now includes:
      - `gateway.slo_threshold_ms`
      - `gateway.slo_latency_ms`
      - `gateway.slo_status`
      - `gateway.slo_reason_code`
    - worker now emits `llm.slo.alert` audit events for warn/breach outcomes.
    - lane-SLO posture is surfaced in startup telemetry and deployment profile wiring.
    - lane-level visibility is now queryable via `/v1/ops/llm-gateway`.
  - completed M14I baseline:
    - local-tier activation controls are now wired for on-prem/hybrid deployments:
      - optional secondary local endpoint:
        - `LLM_LOCAL_SMALL_BASE_URL`
        - `LLM_LOCAL_SMALL_MODEL`
        - `LLM_LOCAL_SMALL_API_KEY` / `LLM_LOCAL_SMALL_API_KEY_REF`
      - lane default tier controls:
        - `LLM_LOCAL_INTERACTIVE_TIER`
        - `LLM_LOCAL_BATCH_TIER`
      - per-action override:
        - `llm.infer` args `local_tier=workhorse|small`
    - deterministic local-tier fallback behavior added:
      - small -> workhorse fallback when small endpoint is unavailable
      - workhorse -> small fallback when workhorse endpoint is unavailable
    - `llm.infer` gateway metadata now includes:
      - `gateway.local_tier_requested`
      - `gateway.local_tier_selected`
      - `gateway.local_tier_reason_code`
    - gateway decision version is now `m14i.v1`

Scope:
- Add a centralized model gateway boundary so agents do not call model providers directly.
- Keep one product surface:
  - same binaries and same API contract across profiles
  - enterprise features are additive via config/profile flags, not mandatory for solo/dev use
- Implement deterministic tiered routing:
  - Tier 0: deterministic/rules and lightweight extraction/classification
  - Tier 1: on-prem/local model tier (deployable but optional at first)
  - Tier 2: premium remote tier (OpenAI-class) for escalations
- Keep remote-first operation viable from day one:
  - policy-safe egress classification (`never_leaves_prem`, `redacted_only`, `cloud_allowed`)
  - fail-closed behavior when local tier is disabled/unavailable and cloud use is disallowed
- Add escalation policy contracts:
  - risk class triggers (payments, security mutations, customer-facing outputs)
  - schema/tool failure triggers
  - optional verifier-score threshold trigger
- Add gateway controls:
  - per-tenant/per-agent/per-workflow token/cost budgets
  - queue/admission control and overload fallback policy
  - response caching with tenant/policy scope isolation
- Add observability:
  - routing decision logs
  - per-tier latency/cost/success metrics
  - escalation reason analytics and budget pressure signals
- Define deployment profiles:
  - `solo/dev`:
    - remote-only permitted by default
    - no SIEM/Vault/SSO required
    - minimal operational prerequisites
  - `enterprise`:
    - strict auth boundary and governance controls
    - compliance export/SIEM pathways enabled
    - hardened egress and secret-backend posture

Landmarks:
- Agents invoke a single internal LLM gateway contract; provider selection is no longer agent-local logic.
- Remote-only mode works with the same gateway contract used later for local-first.
- Escalation decisions are auditable with stable reason codes.
- Operators can see per-tier burn and fallback rates in existing ops surfaces.
- Solo/dev users can run a minimal remote-first setup without enabling enterprise-only dependencies.
- Enterprise users can enable hardened controls without changing agent integration code.

Exit criteria:
- Integration coverage validates:
  - deterministic routing decisions for remote-only and hybrid profiles
  - escalation/fallback reason-code behavior
  - budget and egress policy enforcement
  - cache isolation and deny-on-policy-mismatch behavior
  - profile compatibility:
    - solo/dev profile boots and runs without enterprise dependencies
    - enterprise profile enforces full control set with same API surface
- Runbook/manual include:
  - solo/dev quickstart profile
  - remote-only deployment profile
  - hybrid on-prem + cloud deployment profile
  - enterprise hardened deployment profile
  - incident playbook for local-tier saturation and provider outages

## M15 — Solo-Lite Storage Profile (Post-MVP)
Status:
- In progress scaffold baseline:
  - M15A storage-backend seam scaffold added:
    - `core/src/storage.rs` backend detection (`postgres` vs `sqlite`)
    - `core/src/db_pool.rs` runtime pool abstraction (`DbPool`)
    - API/worker startup now parse and log backend intent from `DATABASE_URL`
  - M15B SQLite schema + smoke-test scaffold added:
    - SQLite migration baseline: `migrations/sqlite/0001_init.sql`
    - SQLite lifecycle smoke test: `core/tests/sqlite_solo_lite_integration.rs`
    - dual-db run/step/audit/ops-summary path scaffold:
      - `core/src/db_dual.rs`
      - `core/tests/db_dual_sqlite_integration.rs`
    - API read/write path wiring now uses dual-core calls for:
      - `POST /v1/runs`
      - `GET /v1/runs/{id}`
      - `GET /v1/runs/{id}/audit`
      - `GET /v1/ops/summary`
    - API sqlite runtime profile now serves:
      - `app_router_sqlite(...)`
      - trigger endpoints:
        - `POST /v1/triggers`
        - `POST /v1/triggers/cron`
        - `POST /v1/triggers/webhook`
        - `PATCH /v1/triggers/{id}`
        - `POST /v1/triggers/{id}/enable`
        - `POST /v1/triggers/{id}/disable`
        - `POST /v1/triggers/{id}/events`
        - `POST /v1/triggers/{id}/events/{event_id}/replay`
        - `POST /v1/triggers/{id}/fire`
      - memory endpoints:
        - `GET/POST /v1/memory/records`
        - `GET/POST /v1/memory/handoff-packets`
        - `GET /v1/memory/retrieve`
        - `GET /v1/memory/compactions/stats`
        - `POST /v1/memory/records/purge-expired`
      - reporting endpoints:
        - `GET /v1/payments`
        - `GET /v1/payments/summary`
        - `GET /v1/usage/llm/tokens`
      - compliance endpoints:
        - `GET /v1/audit/compliance`
        - `GET /v1/audit/compliance/export`
        - `GET /v1/audit/compliance/siem/export`
        - `GET /v1/audit/compliance/siem/deliveries`
        - `POST /v1/audit/compliance/siem/deliveries`
        - `GET /v1/audit/compliance/siem/deliveries/summary`
        - `GET /v1/audit/compliance/siem/deliveries/slo`
        - `GET /v1/audit/compliance/siem/deliveries/targets`
        - `GET /v1/audit/compliance/siem/deliveries/alerts`
        - `POST /v1/audit/compliance/siem/deliveries/alerts/ack`
        - `POST /v1/audit/compliance/siem/deliveries/{id}/replay`
        - `GET /v1/audit/compliance/policy`
        - `PUT /v1/audit/compliance/policy`
        - `POST /v1/audit/compliance/purge`
        - `GET /v1/audit/compliance/verify`
        - `GET /v1/audit/compliance/replay-package`
      - ops endpoints:
        - `GET /v1/ops/summary`
        - `GET /v1/ops/latency-histogram`
        - `GET /v1/ops/latency-traces`
        - `GET /v1/ops/action-latency`
        - `GET /v1/ops/action-latency-traces`
        - `GET /v1/ops/llm-gateway`
      - non-profile routes fail closed with `SQLITE_PROFILE_ENDPOINT_UNAVAILABLE`
    - worker runtime now uses dual-core DB helpers for core run-loop paths:
      - run claim/lease/requeue
      - run/step transitions + run audit appends
      - action request/result persistence
      - artifact persistence
      - payment request/result ledger path + spend counters
      - LLM token usage persistence + budget counters
    - worker SQLite mode now includes parity for:
      - trigger scheduler dispatch
      - memory compaction
      - compliance SIEM outbox delivery
    - worker dual helper coverage:
      - `core/src/db_worker_dual.rs`
      - `core/tests/db_worker_dual_sqlite_integration.rs`
  - M15C solo-lite profile + operator scaffolding added:
    - profile preset: `infra/config/profile.solo-lite.env`
    - init/smoke tooling:
      - `scripts/ops/solo_lite_init.py`
      - `scripts/ops/solo_lite_smoke.py`
      - `scripts/ops/stack_lite_smoke.py`
      - `scripts/ops/stack_lite_soak.py`
      - `make solo-lite-init`
      - `make solo-lite-smoke`
      - `make stack-lite-smoke`
      - `make stack-lite-soak`
    - no-Postgres compose profile baseline:
      - compose services: `api-lite`, `worker-lite` (`profiles: ["solo-lite"]`)
      - Make targets:
        - `make stack-lite-build`
        - `make stack-lite-up`
        - `make stack-lite-up-build`
        - `make stack-lite-ps`
        - `make stack-lite-smoke`
        - `make stack-lite-soak`
        - `make stack-lite-logs`
        - `make stack-lite-down`
- Remaining for full M15 completion:
  - runtime query parity across all API/worker paths (API currently ships a scoped SQLite route profile; worker has broad SQLite parity on core run-loop subsystems)
  - broaden no-Postgres profile validation/soak coverage for release-grade sign-off
- Goal: provide a simpler single-user deployment path using SQLite, while keeping Postgres as the default for team/enterprise.

Scope:
- Add a solo-lite runtime/storage profile for one-off and small self-hosted usage.
- Keep enterprise topology unchanged:
  - shared Postgres remains default for `dev`/`staging`/`prod` team deployments.
  - no weakening of policy/audit/security controls.
- Ensure same product surface:
  - same API endpoints
  - same worker semantics
  - profile differences are deployment/storage backend only.

Landmarks:
- M15A storage backend seam:
  - introduce DB backend abstraction boundary (Postgres + SQLite implementations).
  - keep current Postgres behavior as reference baseline.
- M15B SQLite parity for core runtime paths:
  - run/step lifecycle
  - trigger/scheduler state
  - audit and compliance persistence
  - memory/payment/usage tables needed by current API + worker flows.
- M15C solo-lite packaging and ops profile:
  - add compose profile or compose variant with no Postgres dependency.
  - SQLite file persisted via mounted volume (not container ephemeral FS).
  - add `infra/config/profile.solo-lite.env` and quickstart/runbook path.

Guardrails:
- SQLite is single-user/small-footprint profile only.
- No claim of horizontal multi-writer scale for SQLite profile.
- WAL mode and durability posture must be explicit:
  - `journal_mode=WAL`
  - `foreign_keys=ON`
  - `busy_timeout` configured
  - sync mode documented (`NORMAL` dev, `FULL` stricter durability).
- Profile must fail closed on misconfiguration (missing writable DB path, permissions, migration mismatch).

Exit criteria:
- Integration coverage validates parity for key API/worker flows on SQLite and Postgres backends.
- Solo-lite quickstart boots without Postgres and passes baseline smoke:
  - create run
  - execute worker step
  - query audit/events
  - inspect ops summary.
- Ops docs clearly differentiate:
  - solo-lite (SQLite, single-user)
  - standard/enterprise (Postgres, scalable/shared service).
