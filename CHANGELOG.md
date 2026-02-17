# CHANGELOG

All notable changes to this project will be documented in this file.

This project follows a lightweight, practical changelog format. Versions are early and pre-stable.

---

## v0.0.47 — Expand M4B with cron triggers, trigger lifecycle APIs, and in-flight guardrails

### Added
- New trigger migration `migrations/0005_trigger_cron_and_guardrails.sql`:
  - cron scheduling fields on `triggers` (`cron_expression`, `schedule_timezone`)
  - per-trigger concurrency limit (`max_inflight_runs`)
  - trigger audit table (`trigger_audit_events`)
  - trigger type expansion to include `cron`
- Core trigger DB capabilities in `core/src/db.rs`:
  - `create_cron_trigger(...)`
  - `update_trigger_config(...)`
  - `update_trigger_status(...)`
  - `append_trigger_audit_event(...)`
  - scheduler wrappers with tenant limits:
    - `dispatch_next_due_trigger_with_limits(...)`
    - `dispatch_next_due_interval_trigger_with_limits(...)`
  - manual fire wrapper with tenant limits:
    - `fire_trigger_manually_with_limits(...)`
- API trigger lifecycle endpoints in `api/src/lib.rs`:
  - `POST /v1/triggers/cron`
  - `PATCH /v1/triggers/:id`
  - `POST /v1/triggers/:id/enable`
  - `POST /v1/triggers/:id/disable`

### Changed
- Trigger dispatch now supports cron runs and enforces in-flight guardrails:
  - per-trigger (`triggers.max_inflight_runs`)
  - per-tenant (worker-configured scheduler limit)
- Manual trigger fire now returns `429` when trigger/tenant is at max in-flight capacity.
- Worker scheduler now uses tenant in-flight limit config:
  - `WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS` (default `100`)
- API trigger mutation flow now appends persistent trigger audit records for create/update/enable/disable/manual-fire actions.
- Trigger response payloads now include:
  - `cron_expression`
  - `schedule_timezone`
  - `max_inflight_runs`
- Updated and expanded test coverage:
  - `core/tests/db_integration.rs`: cron dispatch + in-flight guardrails + manual fire guardrail
  - `api/tests/api_integration.rs`: cron create + trigger update + enable/disable lifecycle
  - `worker/tests/worker_integration.rs`: updated trigger builders for new guardrail fields
- Added cron/timezone dependencies in `core/Cargo.toml` and refreshed `Cargo.lock`.

## v0.0.46 — Add manual trigger fire API with idempotency and trigger mutation role guardrails

### Added
- Core manual trigger fire primitive in `core/src/db.rs`:
  - `fire_trigger_manually(...)` with namespaced dedupe keys (`manual:<idempotency_key>`)
  - `ManualTriggerFireOutcome` for created/duplicate/unavailable outcomes
- API manual fire endpoint in `api/src/lib.rs`:
  - `POST /v1/triggers/:id/fire`
  - accepts `idempotency_key` and optional payload envelope
  - returns deterministic `created` vs `duplicate` status and run linkage
- Integration coverage:
  - `core/tests/db_integration.rs`: manual fire dedupe behavior
  - `api/tests/api_integration.rs`: manual fire create+dedupe path and viewer denial path

### Changed
- Trigger mutation role guardrails in API:
  - `viewer` is now denied for `POST /v1/triggers`, `POST /v1/triggers/webhook`, and `POST /v1/triggers/:id/fire` (`403 FORBIDDEN`)
  - `owner`/`operator` remain allowed
- Manual-triggered runs now append `run.created` audit events with `trigger_manual_api` provenance.
- Updated docs for new trigger fire endpoint and role policy behavior:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/POLICY.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.45 — Expand trigger plane with webhook events and wire CLI secret adapters

### Added
- Trigger/event migration in `migrations/0004_trigger_events.sql`:
  - trigger metadata columns: `misfire_policy`, `max_attempts`, `consecutive_failures`, dead-letter fields, `webhook_secret_ref`
  - trigger type expansion (`interval` + `webhook`) and conditional interval validation
  - new `trigger_events` queue table with dedupe (`trigger_id`, `event_id`) and due index
- Core trigger APIs in `core/src/db.rs`:
  - `create_webhook_trigger(...)`
  - `enqueue_trigger_event(...)`
  - `get_trigger(...)`
  - `dispatch_next_due_trigger(...)` (webhook-first, interval fallback)
- API webhook trigger endpoints in `api/src/lib.rs`:
  - `POST /v1/triggers/webhook`
  - `POST /v1/triggers/:id/events`
  - optional trigger secret validation via `x-trigger-secret`
- CLI-backed secret provider adapters in `core/src/secrets.rs` for:
  - `vault:...` (`vault` CLI)
  - `aws-sm:...` (`aws` CLI)
  - `gcp-sm:...` (`gcloud` CLI)
  - `azure-kv:...` (`az` CLI)

### Changed
- Trigger dispatch behavior:
  - interval dispatch now supports misfire skip policy (`misfire_policy=skip`) with failed trigger-run ledger entries
  - webhook event dispatch creates queued runs with trigger envelope context and marks events `processed`/`dead_lettered`
  - run-created audit payload now includes `trigger_type` and `trigger_event_id` when applicable (`worker/src/lib.rs`)
- Secret resolution paths now use `CliSecretResolver::from_env()` in worker runtime config resolution (`worker/src/lib.rs`, `worker/src/llm.rs`).
- Cloud secret adapters are fail-closed by default and require `AEGIS_SECRET_ENABLE_CLOUD_CLI=1`.
- Expanded tests:
  - `api/tests/api_integration.rs`: webhook trigger creation, secret-gated event ingest, event dedupe
  - `core/tests/db_integration.rs`: misfire-skip interval behavior, webhook enqueue/dispatch flow
  - `worker/tests/worker_integration.rs`: webhook event dispatch through worker loop
  - `core/src/secrets.rs`: parser + fail-closed resolver behavior
- Updated docs for new trigger and secret-adapter behavior:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.44 — Add interval trigger dispatch baseline and secret references

### Added
- Trigger schema in `migrations/0003_triggers.sql`:
  - `triggers` table for durable interval trigger definitions
  - `trigger_runs` ledger for fired trigger/run linkage with dedupe keys
- Core trigger DB APIs in `core/src/db.rs`:
  - `create_interval_trigger(...)`
  - `dispatch_next_due_interval_trigger(...)`
- API trigger creation endpoint in `api/src/lib.rs`:
  - `POST /v1/triggers` for interval triggers with recipe-aware capability grant resolution
- Worker trigger scheduler baseline in `worker/src/lib.rs`:
  - optional due-trigger dispatch each poll cycle before queue claim
  - trigger-created run provenance persisted via `run.created` audit payload
- Shared secret reference abstraction in `core/src/secrets.rs`:
  - reference parsing for `env:`, `file:`, `vault:`, `aws-sm:`, `gcp-sm:`, `azure-kv:`
  - live resolution for `env:` and `file:`
  - fail-closed behavior for unconfigured cloud backends

### Changed
- Worker config now supports `WORKER_TRIGGER_SCHEDULER_ENABLED` (`worker/src/lib.rs`, `worker/src/main.rs`).
- Worker LLM/Slack config now supports secret references:
  - `LLM_LOCAL_API_KEY_REF`
  - `LLM_REMOTE_API_KEY_REF`
  - `SLACK_WEBHOOK_URL_REF`
- Added/updated test coverage:
  - `core/tests/db_integration.rs`: trigger dispatch + run creation flow
  - `api/tests/api_integration.rs`: trigger creation endpoint and interval validation
  - `worker/tests/worker_integration.rs`: end-to-end due-trigger dispatch and processing
- Updated docs/handoff/roadmap for new trigger and secrets baselines:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.43 — Add roadmap milestones for triggers and multi-provider secrets

### Added
- New roadmap milestone `M4B` in `docs/ROADMAP.md` for a durable trigger/orchestration plane:
  - schedule + event + manual trigger types
  - HA-safe scheduler dispatch, dedupe/idempotency, misfire handling, dead-lettering
  - trigger provenance in run/audit records
- New roadmap milestone `M6B` in `docs/ROADMAP.md` for provider-agnostic secrets:
  - Vault, AWS, Google Cloud, and Azure backends
  - reference-based secret config (no raw secret persistence)
  - rotation, TTL cache, and strict no-skill secret boundary

### Changed
- Updated architecture docs to include Trigger/Scheduler and Secrets Provider components:
  - `docs/ARCHITECTURE.md`
- Updated handoff priorities so new sessions can proceed directly on trigger + secrets implementation:
  - `docs/SESSION_HANDOFF.md`

## v0.0.42 — Add roadmap milestones for sats payments and memory plane

### Added
- New roadmap milestone `M5C` in `docs/ROADMAP.md` for agent-to-agent payments:
  - Nostr Wallet Connect (NIP-47) first rail
  - policy-gated `payment.send`
  - spend budgets, idempotency, and settlement/audit requirements
  - optional Cashu follow-on track (NIP-60/NIP-61)
- New roadmap milestone `M6A` in `docs/ROADMAP.md` for durable agent memory:
  - layered memory model (session, semantic, procedural)
  - redaction-aware indexing and retention controls
  - compaction/summarization and inter-agent handoff memory artifacts

### Changed
- Updated `docs/SESSION_HANDOFF.md` snapshot and prioritized next steps so new sessions can continue directly on payments + memory implementation.

## v0.0.41 — Add Slack retry/backoff and dead-letter delivery state

### Added
- Worker Slack runtime config in `worker/src/lib.rs`:
  - `SLACK_MAX_ATTEMPTS` (default `3`, minimum `1`)
  - `SLACK_RETRY_BACKOFF_MS` (base retry backoff)
- Worker integration coverage in `worker/tests/worker_integration.rs`:
  - retries Slack webhook after transient failures and succeeds
  - marks Slack delivery as dead-lettered after retry exhaustion

### Changed
- Slack `message.send` delivery now retries webhook sends with exponential backoff and records attempt metadata.
- Persistent Slack failures now use delivery state `dead_lettered_local_outbox` with structured retry/error context.
- Worker startup logs now include Slack retry configuration (`worker/src/main.rs`).
- Updated docs/handoff/roadmap for retry and dead-letter behavior:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.40 — Add role-aware API capability presets

### Added
- API role preset parsing in `api/src/lib.rs` via optional header `x-user-role`:
  - `owner` (default), `operator`, `viewer`
- API integration coverage in `api/tests/api_integration.rs`:
  - operator preset removes `local.exec` from recipe bundle grants
  - viewer preset narrows grants to `object.read` + local-route `llm.infer`
  - invalid `x-user-role` values return `400 BAD_REQUEST`

### Changed
- `POST /v1/runs` capability resolution now applies role presets before granting capabilities:
  - recipe bundle defaults + requested intersections remain intact
  - role presets further constrain both default bundle grants and request-based grants
- `run.created` audit payload now includes `role_preset`.
- Updated docs and handoff for role-aware preset behavior:
  - `docs/API.md`
  - `docs/POLICY.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.39 — Add remote LLM token budgets and cost accounting metadata

### Added
- Worker LLM config in `worker/src/llm.rs` now supports:
  - `LLM_REMOTE_TOKEN_BUDGET_PER_RUN` (optional per-run remote token cap)
  - `LLM_REMOTE_COST_PER_1K_TOKENS_USD` (optional estimated-cost rate)
- Worker integration coverage in `worker/tests/worker_integration.rs`:
  - remote `llm.infer` run fails when requested remote token estimate exceeds configured per-run budget
- Reference Python skill (`skills/python/summarize_transcript/main.py`) now forwards optional `llm_max_tokens` input into `llm.infer` action args.

### Changed
- `worker/src/lib.rs` `llm.infer` action execution now:
  - tracks per-run remote token budget state during action execution
  - performs preflight budget checks for remote route requests
  - emits `token_accounting` metadata in action results (`estimated_tokens`, `consumed_tokens`, `remote_token_budget_remaining`, `estimated_cost_usd`)
- Worker startup logs include remote budget/cost settings (`worker/src/main.rs`).
- Updated operational/development/handoff docs for new budget/cost controls:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.38 — Add Slack webhook delivery transport for `message.send`

### Added
- New Slack transport module `worker/src/slack.rs` with webhook delivery helper:
  - sends `message.send` payloads to configured webhook endpoint
  - records HTTP status and response body for delivery metadata
- Worker integration coverage in `worker/tests/worker_integration.rs`:
  - `slack:*` `message.send` delivery path against a local mock webhook endpoint

### Changed
- `worker/src/lib.rs` `message.send` execution now supports Slack transport behavior:
  - `slack:*` routes deliver immediately when `SLACK_WEBHOOK_URL` is configured
  - still writes local outbox artifact for traceability in all cases
  - persists normalized delivery metadata fields (`delivery_state`, `delivery_result`, `delivery_error`, `delivery_context`)
- Worker config now includes:
  - `SLACK_WEBHOOK_URL`
  - `SLACK_SEND_TIMEOUT_MS`
- Worker startup logs now include Slack transport configuration state (`worker/src/main.rs`).
- Updated docs/handoff/roadmap for Slack transport support:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.37 — Add API-managed recipe capability bundles

### Added
- API recipe capability bundle resolver in `api/src/lib.rs`:
  - known recipes now have policy-owned capability presets
  - empty `requested_capabilities` receives bundle defaults
  - non-empty requests are intersected with bundle scope (fail-closed filtering)
- API integration tests in `api/tests/api_integration.rs`:
  - bundle defaults applied when requested list is empty
  - requested capabilities are filtered when outside recipe bundle scope

### Changed
- `POST /v1/runs` now resolves grants using:
  - existing capability normalization + hard caps
  - recipe bundle intersection when recipe is known
- Updated docs and handoff state for bundle-based grant behavior:
  - `docs/API.md`
  - `docs/POLICY.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.36 — Add remote LLM egress guardrails (default deny)

### Added
- Remote egress policy controls for `llm.infer` in `worker/src/llm.rs`:
  - `LLM_REMOTE_EGRESS_ENABLED` (default `0` / blocked)
  - `LLM_REMOTE_HOST_ALLOWLIST` (required host allowlist for remote routes)
- Unit tests in `worker/src/llm.rs` for:
  - remote block when egress is disabled
  - remote block when host is not allowlisted
  - policy scope resolution remains deterministic for remote-preferred actions
- Worker integration test in `worker/tests/worker_integration.rs`:
  - verifies remote `llm.infer` is blocked when egress gate is off even with remote capability granted

### Changed
- Worker startup logs now include remote egress gate status and allowlist count (`worker/src/main.rs`).
- Updated operational/development/handoff docs with remote egress gate configuration:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.35 — Add sandboxed `local.exec` and local-first `llm.infer`

### Added
- New sandboxed local execution primitive `worker/src/local_exec.rs`:
  - template-only command registry (`file.head`, `file.word_count`, `file.touch`)
  - absolute-path root enforcement for read/write scopes
  - hard runtime controls (timeout/output + unix process/memory limits)
- New LLM routing/execution module `worker/src/llm.rs`:
  - configurable `LLM_MODE` (`local_only`, `local_first`, `remote_only`)
  - OpenAI-compatible chat completion requests for local/remote endpoints
  - route-specific policy scope resolution (`local:<model>` / `remote:<model>`)
- Expanded integration coverage:
  - `worker/tests/worker_integration.rs`:
    - local exec success and out-of-scope failure
    - local-first llm infer success using mock endpoint
    - policy denial when remote llm route is requested but only local scope is granted
- API capability resolver support for:
  - `local.exec` scopes
  - `llm.infer` local/remote scopes
  - hard payload limits for both

### Changed
- Core policy model now includes `local.exec` and `llm.infer` capability kinds with scope-based allow/deny tests.
- Worker action execution path now supports `local.exec` and `llm.infer`.
- Worker startup logging now reports LLM mode/local-remote config presence and local exec sandbox state.
- Reference Python skill can request both `llm.infer` and `local.exec` actions in addition to current actions.
- Updated docs and session handoff for new primitives and local-first defaults:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/SECURITY.md`
  - `docs/POLICY.md`
  - `docs/ARCHITECTURE.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/README.md`

## v0.0.34 — Start M6 hardening: env containment + redacted persistence

### Added
- New core redaction utilities in `core/src/redaction.rs`:
  - recursive JSON redaction for sensitive keys
  - token redaction helpers for `nsec1...` and `Bearer ...` patterns
  - unit tests for key-based and token-based redaction behavior
- Skill runner integration test coverage in `skillrunner/tests/runner_integration.rs`:
  - verifies skill subprocesses do not inherit parent env secrets by default
  - verifies explicit env allowlisting works when required
- Worker integration test coverage in `worker/tests/worker_integration.rs`:
  - validates sensitive message payloads are redacted in persisted action/audit records

### Changed
- `skillrunner/src/runner.rs` now launches skills with:
  - `env_clear` by default
  - fixed `AEGIS_SKILL_SANDBOXED=1` marker
  - optional env pass-through via `RunnerConfig.env_allowlist`
- `worker/src/lib.rs` now:
  - supports `WORKER_SKILL_ENV_ALLOWLIST`
  - passes allowlisted env keys into skill runner config
  - redacts action request args, action results, audit payloads, and error payloads before persistence
- Worker startup logging includes allowlisted skill-env count (`worker/src/main.rs`).
- Updated security/development/operations/roadmap/handoff docs for M6 baseline:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/SECURITY.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.33 — Add NIP-46 remote-sign publish for White Noise relay transport

### Added
- New NIP-46 signer transport module `worker/src/nip46_signer.rs`:
  - connects to bunker relay from `NOSTR_NIP46_BUNKER_URI`
  - performs `connect` + `sign_event` NIP-46 request flow
  - decrypts and validates NIP-46 responses
  - returns signed events for relay publish
- Worker signer config now supports `NOSTR_NIP46_CLIENT_SECRET_KEY` for stable client app-key identity in NIP-46 mode.
- Worker integration coverage for end-to-end NIP-46 publish path:
  - `worker/tests/worker_integration.rs` now includes mock bunker/relay flow validating `message.send` relay publish with `NOSTR_SIGNER_MODE=nip46_signer`.

### Changed
- White Noise relay publish in `worker/src/lib.rs` now signs via signer mode:
  - `local_key` mode uses local secret key material
  - `nip46_signer` mode signs remotely through bunker URI, then publishes signed event to configured relays
- `worker/src/nostr_transport.rs` now separates unsigned event building from relay publish of already-signed events.
- Updated docs and handoff state for implemented NIP-46 publish support:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/README.md`
  - `docs/ADR/ADR-0007-pluggable-nostr-signer-modes.md`

## v0.0.32 — Add White Noise relay publish path for `message.send`

### Added
- New Nostr relay transport module `worker/src/nostr_transport.rs`:
  - signs Nostr text-note events for White Noise messages
  - publishes events to configured relays over websocket
  - parses relay `OK` ACK responses and reports per-relay outcomes
- Worker config knobs in `worker/src/lib.rs`:
  - `NOSTR_RELAYS` (comma-separated relay URLs)
  - `NOSTR_PUBLISH_TIMEOUT_MS`
- Integration test coverage in `worker/tests/worker_integration.rs`:
  - successful publish flow against a local mock relay with ACK validation
- Unit test coverage in `worker/src/nostr_transport.rs` for ACK parsing.

### Changed
- `message.send` White Noise execution now:
  - attempts relay publish when relays are configured and local signing key material is available
  - continues writing outbox artifacts for traceability in all cases
  - stores publish metadata in action result payloads (`delivery_state`, `accepted_relays`, `published_event_id`, `publish_error`)
- Worker startup logs now include relay publish configuration summary.
- Updated docs for relay publish behavior and handoff state:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/README.md`

## v0.0.31 — Add API policy-authoritative capability grant resolution

### Added
- API grant resolver in `api/src/lib.rs` for `POST /v1/runs`:
  - validates `requested_capabilities` shape (must be array of capability objects)
  - normalizes capability aliases (`object_write` -> `object.write`, etc.)
  - applies allowlisted scope rules per capability
  - enforces MVP deny for `http.request` and `db.query`
  - applies hard payload cap limits to granted capabilities
- API integration test coverage in `api/tests/api_integration.rs`:
  - grants are resolved and returned (not mirrored)
  - disallowed capabilities/scopes are filtered out
  - invalid `requested_capabilities` payload shape returns `400`

### Changed
- `run.created` audit payload now includes requested/granted capability counts.
- Updated docs for new grant behavior and handoff state:
  - `docs/API.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.30 — Add `message.send` worker execution baseline with signer-aware White Noise gating

### Added
- Worker `message.send` execution path in `worker/src/lib.rs`:
  - supports provider-scoped destinations (`whitenoise:<target>`, `slack:<target>`)
  - requires configured Nostr signer identity for White Noise destinations
  - persists outbound connector envelopes to local outbox artifacts under `messages/...`
  - records artifact metadata for message outbox entries
- Worker action execution failure handling improvements:
  - failed action execution now updates `action_requests.status` to `failed`
  - persists `action_results` with `ACTION_EXECUTION_FAILED`
  - appends `action.failed` audit events
- Worker integration tests for messaging paths in `worker/tests/worker_integration.rs`:
  - successful White Noise message execution with local signer
  - White Noise message failure when signer is missing

### Changed
- Reference Python skill (`skills/python/summarize_transcript/main.py`) can now request `message.send` actions.
- Updated roadmap/operations/development/handoff docs for message connector baseline and next transport work:
  - `docs/ROADMAP.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.29 — Normalize signer docs terminology for self-hosted and enterprise audiences

### Changed
- Replaced informal wording in signer-related docs with neutral terminology:
  - `docs/DEVELOPMENT.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/ADR/ADR-0007-pluggable-nostr-signer-modes.md`

## v0.0.28 — Add pluggable Nostr signer modes (local default + optional NIP-46)

### Added
- Worker signer module `worker/src/signer.rs` with:
  - `NostrSignerMode` (`local_key`, `nip46_signer`)
  - startup-safe config parsing from env
  - local key identity derivation (nsec/hex secret -> normalized `npub`)
  - NIP-46 identity validation from bunker URI/public key
  - owner-only permission checks (`0600`) for file-based local key loading on Unix
- Unit tests for signer mode behavior and identity resolution paths.
- ADR `docs/ADR/ADR-0007-pluggable-nostr-signer-modes.md` formalizing self-hosted + enterprise signer strategy.

### Changed
- `worker/src/lib.rs` `WorkerConfig` now includes `nostr_signer` settings parsed from env.
- `worker/src/main.rs` now resolves/logs signer identity at startup and warns when local mode has no configured key.
- Added `nostr` workspace dependency for signer identity parsing.
- Updated docs for signer configuration and handoff continuity:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ARCHITECTURE.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/README.md`

## v0.0.27 — Complete worker vertical slice with skill invocation and policy-gated action execution

### Added
- Worker step execution path in `worker/src/lib.rs`:
  - creates/persists run step records
  - invokes Python reference skill through `skillrunner`
  - persists `action_requests` / `action_results`
  - evaluates policy decisions per action request
  - executes allowed `object.write` actions and persists artifact metadata
- New `core` DB APIs for step/action lifecycle persistence:
  - `mark_step_succeeded`
  - `mark_step_failed`
  - `create_action_request`
  - `update_action_request_status`
  - `create_action_result`
- Expanded integration coverage:
  - `worker/tests/worker_integration.rs` now validates successful action execution and policy-denied action failure paths
  - `core/tests/db_integration.rs` adds step/action persistence transition coverage

### Changed
- `claim_next_queued_run` now returns `input_json` and `granted_capabilities` to support in-worker execution decisions.
- `worker/src/main.rs` outcome logging now distinguishes succeeded vs failed processed runs.
- `api/src/lib.rs` now mirrors requested capabilities into granted capabilities in MVP mode to unblock end-to-end execution flow.
- Updated docs for current implementation status and next priorities:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/TESTING.md`

## v0.0.26 — Add API doc to main docs index

### Changed
- Updated `docs/README.md` docs list to include `docs/API.md` for easier session/bootstrap discovery.

## v0.0.25 — Implement M5 API create/status/audit endpoints with tenant-scoped DB reads

### Added
- `api/src/lib.rs`:
  - `POST /v1/runs` (creates queued run + appends `run.created` audit event)
  - `GET /v1/runs/{id}` (tenant-scoped run status/read model)
  - `GET /v1/runs/{id}/audit` (tenant-scoped ordered audit stream with `limit`)
- DB-backed API integration tests:
  - `api/tests/api_integration.rs`
  - covers create/status path, audit ordering, and required tenant header behavior
- New `make test-api-db` target for API DB integration test execution.

### Changed
- `api/src/main.rs` now starts a real Axum server using `DATABASE_URL` and `API_BIND`.
- Expanded `core` DB read APIs used by API layer:
  - `get_run_status`
  - `list_run_audit_events`
- Updated docs for API/runtime/test usage and roadmap status:
  - `docs/API.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/TESTING.md`

## v0.0.24 — Implement worker run-loop baseline with lease-backed lifecycle and tests

### Added
- `worker/src/lib.rs`:
  - `WorkerConfig` with env-driven lease/poll/requeue settings
  - `process_once` worker cycle using core lease APIs
  - run audit events for claim/start/complete and lease-renew failure paths
- `worker/tests/worker_integration.rs` DB integration coverage for:
  - queued run claim + completion
  - stale-running requeue + completion
  - idle cycle behavior when no work exists
- `make test-worker-db` target for DB-backed worker integration validation.

### Changed
- `worker/src/main.rs` now runs a real poll loop against Postgres instead of placeholder output.
- `core/src/db.rs` lease claim record now includes `triggered_by_user_id` so worker audits can preserve actor context.
- Updated docs to reflect current status and testing commands:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/TESTING.md`

## v0.0.23 — Add explicit new-session handoff doc and reading order

### Added
- `docs/SESSION_HANDOFF.md`:
  - current implementation snapshot
  - mandatory read order for new Codex sessions
  - critical ADR references
  - environment/runtime notes and verification commands
  - high-priority next steps and a copy/paste bootstrap prompt

### Changed
- Updated `AGENTS.md` to require `docs/SESSION_HANDOFF.md` in pre-change reading sequence.
- Updated `docs/README.md` docs index to include `docs/SESSION_HANDOFF.md`.

## v0.0.22 — Add run-lease queue claim primitives for worker reliability

### Added
- New migration `migrations/0002_run_leases.sql`:
  - adds `runs.attempts`, `runs.lease_owner`, `runs.lease_expires_at`
  - adds queue-claim/recovery indexes on `runs`
  - adds uniqueness on `action_results(action_request_id)` for idempotent result writes
- New `core` DB APIs for robust worker coordination:
  - `claim_next_queued_run` (queue claim with lease + `FOR UPDATE SKIP LOCKED`)
  - `renew_run_lease`
  - `mark_run_succeeded`
  - `mark_run_failed`
  - `requeue_expired_runs`
- Added integration test coverage for lease behavior in `core/tests/db_integration.rs`:
  - queue claim order + lease assignment
  - lease renewal + successful completion
  - stale running-run requeue

### Changed
- Updated `docs/SCHEMA.md` to include run-attempt and lease columns/indexes.
- Updated `docs/ROADMAP.md` M4 landmark to call out lease-based queue claims.

## v0.0.21 — Cleanup: remove obsolete repo skeleton archive

### Changed
- Removed `agent_platform_repo_skeleton.zip` from the repository root.
- The project now uses the live workspace/docs directly without bundled scaffold archive artifacts.

## v0.0.20 — Align DB integration test default with local Postgres DB name

### Changed
- Updated `make test-db` default `TEST_DATABASE_URL` from `agentdb_test` to `agentdb` to match the compose Postgres initialization.
- Updated DB test command/examples in `docs/TESTING.md` to use `agentdb` by default.

## v0.0.19 — Fix Postgres 18 data-volume layout for container startup

### Changed
- Updated `infra/containers/compose.yml` Postgres volume mount for 18+ images:
  - from `/var/lib/postgresql/data`
  - to `/var/lib/postgresql`
- Renamed compose volume to `agentdb-pg18-data` to avoid reuse of incompatible prior volume layout.

## v0.0.18 — Use fully qualified Postgres image for Podman compatibility

### Changed
- Updated compose image reference in `infra/containers/compose.yml`:
  - `postgres:18` -> `docker.io/library/postgres:18`
- Fixes Podman hosts configured with strict short-name resolution (no unqualified search registries).

## v0.0.17 — Fix Podman compose file path resolution

### Changed
- Updated `Makefile` compose invocation to pass an absolute compose file path (`COMPOSE_FILE_ABS`) for `db-up`/`db-down`.
- Added explicit compose-file existence checks before invoking compose commands.
- Expanded `make container-info` output with:
  - absolute compose file path
  - existence status

## v0.0.16 — Move container assets under `infra/` and bump Postgres to 18

### Changed
- Moved compose config from root to infrastructure layout:
  - `docker-compose.yml` -> `infra/containers/compose.yml`
- Updated Postgres image in compose to `postgres:18`.
- Updated `Makefile` DB runtime wiring:
  - added `COMPOSE_FILE` (default `infra/containers/compose.yml`)
  - `db-up` / `db-down` now run with `-f $(COMPOSE_FILE)`
  - `container-info` now reports active compose file
- Updated docs to match the new container layout and startup flow:
  - `docs/DEVELOPMENT.md`
  - `docs/TESTING.md`
  - `docs/RUNBOOK.md`
  - `docs/MVP_PLAN.md`
  - `docs/CONTRIBUTING.md`

## v0.0.15 — Add Podman-first local runtime support

### Changed
- Updated `Makefile` DB/runtime targets to support Podman and Docker compose auto-detection:
  - `db-up`/`db-down` now use detected compose runtime instead of hardcoded `docker compose`
  - added `container-info` target to show detected runtime and available versions
  - added `COMPOSE_CMD` override support for explicit runtime selection
- Updated docs for Podman-first local setup:
  - `docs/DEVELOPMENT.md`
  - `docs/TESTING.md`
  - `docs/RUNBOOK.md`

## v0.0.14 — Move root docs into `docs/` and update copyright attribution

### Changed
- Moved Markdown docs from repo root into `docs/` (keeping only `AGENTS.md` and `CHANGELOG.md` at root):
  - `ARCHITECTURE.md` -> `docs/ARCHITECTURE_BRIEF.md`
  - `README.md` -> `docs/README.md`
  - `CONTRIBUTING.md` -> `docs/CONTRIBUTING.md`
  - `SECURITY.md` -> `docs/SECURITY.md`
  - `TESTING.md` -> `docs/TESTING.md`
  - `DEVELOPMENT.md` -> `docs/DEVELOPMENT.md`
  - `OPERATIONS.md` -> `docs/OPERATIONS.md`
- Updated internal references to the new docs locations in:
  - `AGENTS.md`
  - `docs/ARCHITECTURE.md`
  - `docs/MVP_PLAN.md`
  - `docs/README.md`
  - `docs/CONTRIBUTING.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
- Updated `NOTICE` copyright attribution to:
  - `Copyright 2026 McIntosh`

## v0.0.13 — M3 skill protocol + runner + Python reference skill

### Added
- `skillrunner` protocol module (`skillrunner/src/protocol.rs`) with NDJSON message types and codecs for:
  - `describe` / `describe_result`
  - `invoke` / `invoke_result`
  - structured `error`
- `skillrunner` subprocess runner (`skillrunner/src/runner.rs`) with:
  - request/response correlation by `id`
  - timeout handling
  - crash/non-zero exit handling
  - max output byte enforcement
- Skill runner integration tests (`skillrunner/tests/runner_integration.rs`) covering:
  - successful invoke
  - timeout kill path
  - crash/non-zero exit path
  - oversized output rejection
- Reference Python skill:
  - `skills/python/summarize_transcript/main.py`
  - `skills/python/summarize_transcript/SKILL.md`
- `make test-db` target for explicit DB integration validation.

### Changed
- Updated `skillrunner/src/lib.rs` to export protocol and runner APIs.
- Expanded workspace Tokio features in `Cargo.toml` for process/time/io support used by runner.
- Updated developer/testing docs to include `make test-db`:
  - `DEVELOPMENT.md`
  - `TESTING.md`

## v0.0.12 — ADR for sandboxed local execution controls

### Added
- `docs/ADR/ADR-0006-sandboxed-local-exec-primitive.md`:
  - formalizes a constrained local-exec primitive model
  - prohibits arbitrary shell usage
  - defines allowlisted templates, scoped path access, strict limits, and auditing requirements

### Changed
- Updated `SECURITY.md` to explicitly forbid arbitrary shell command execution and reference ADR-0006.
- Updated `docs/ROADMAP.md` M6 hardening milestone to reference ADR-0006 for sandbox implementation details.

## v0.0.11 — M2 foundation: initial schema, DB layer, and integration tests

### Added
- Initial migration `migrations/0001_init.sql` for:
  - `agents`, `users`, `runs`, `steps`, `artifacts`, `action_requests`, `action_results`, `audit_events`
- `core` DB access module in `core/src/db.rs` with minimal persistence APIs:
  - `create_run`
  - `create_step`
  - `append_audit_event`
  - `persist_artifact_metadata`
- DB integration tests in `core/tests/db_integration.rs` covering:
  - migration application
  - run/step inserts
  - audit event append

### Changed
- Split `core/src/lib.rs` into `policy` and `db` modules and re-exported public APIs.
- Enabled Postgres-backed integration tests in CI by adding a Postgres service and test env vars in `.github/workflows/ci.yml`.
- Updated developer/testing docs for DB integration test execution:
  - `DEVELOPMENT.md`
  - `TESTING.md`
- Updated `docs/ROADMAP.md` with an explicit channel-communications milestone:
  - White Noise first-class messaging connector
  - Slack enterprise-secondary connector

## v0.0.10 — Nostr-first communications: White Noise first-class, Slack secondary

### Added
- `docs/ADR/ADR-0005-nostr-first-whitenoise.md` to formalize messaging priority and connector order.

### Changed
- Updated docs to make White Noise (Marmot over Nostr) the primary messaging path:
  - `README.md`
  - `ARCHITECTURE.md`
  - `docs/ARCHITECTURE.md`
  - `docs/agent_platform.md`
  - `docs/POLICY.md`
  - `docs/MVP_PLAN.md`
  - `docs/API.md`
  - `docs/ROADMAP.md`

## v0.0.9 — Add contributor and operator docs

### Added
- `DEVELOPMENT.md`:
  - local dev prerequisites and bootstrap
  - shared-Postgres local workflow
  - build/test/migration commands
  - contributor workflow expectations
- `OPERATIONS.md`:
  - deployment topology for shared Postgres per environment
  - runtime safety controls and incident actions
  - DB operations and observability guidance
  - release/change-management checkpoints

### Changed
- Updated docs index in `README.md` to include `DEVELOPMENT.md` and `OPERATIONS.md`.

## v0.0.8 — M1 core contracts: capability and policy engine with tests

### Changed
- Replaced `core` placeholder implementation with reusable policy contracts:
  - `CapabilityKind`, `CapabilityGrant`, `CapabilityLimits`, `GrantSet`
  - `ActionRequest`, `PolicyDecision`, `DenyReason`
  - `is_action_allowed` default-deny evaluator with scoped capability matching and payload limit checks
- Added required `core` policy unit tests for:
  - unknown action type deny
  - missing capability deny
  - scope mismatch deny
  - payload limit deny
  - exact capability+scope allow
  - stable deny reason strings

## v0.0.7 — Add delivery roadmap with milestones and exit criteria

### Added
- `docs/ROADMAP.md` with milestone-based delivery plan (M1-M9), landmarks, and explicit exit criteria.

### Changed
- Added `docs/ROADMAP.md` to documentation index in `README.md`.

## v0.0.6 — Commit Cargo.lock for reproducible workspace builds

### Changed
- Added `Cargo.lock` to version control for deterministic dependency resolution across local/CI builds.

## v0.0.5 — Shared schema topology documented across architecture and ops docs

### Changed
- Documented shared Postgres topology across docs:
  - One Postgres cluster per environment.
  - One standardized app schema per environment (not per-agent DB/schema).
  - Direct Postgres access limited to `api`/`worker`; agents/skills use platform APIs/protocols.
- Updated the following docs accordingly:
  - `README.md`
  - `ARCHITECTURE.md`
  - `docs/ARCHITECTURE.md`
  - `docs/MVP_PLAN.md`
  - `docs/RUNBOOK.md`
  - `docs/SCHEMA.md`

## v0.0.4 — Schema docs: first-class agent/user linkage

### Changed
- Updated `docs/SCHEMA.md` to model enterprise attribution explicitly:
  - Added `agents` and `users` tables.
  - Added `agent_id`/`user_id` linkage fields to `runs`, `steps`, and `audit_events`.
  - Added indexes for common tenant+agent and tenant+user query paths.

## v0.0.3 — Shared Postgres topology ADR + architecture doc link cleanup

### Added
- `docs/ADR/ADR-0004-shared-postgres-topology.md`:
  - One shared Postgres cluster per environment, not one instance per agent.
  - Standardized app schema for platform tables.
  - API/worker services are the only DB clients; agents/skills do not connect directly to Postgres.

### Changed
- Fixed stale protocol-spec references in `docs/ARCHITECTURE.md` to use `docs/agent_platform.md`.

## v0.0.2 — Repo skeleton + sqlx workspace scaffolding + testing standards

### Added
- Repository skeleton ZIP (ready-to-unzip into a new repo) containing:
  - Rust workspace directories: `api/`, `worker/`, `core/`, `skillrunner/`
  - Minimal crate stubs (`src/main.rs` / `src/lib.rs`) so `cargo test` can run immediately
  - Root `Cargo.toml` workspace with shared dependencies (Tokio, Axum, sqlx, serde, uuid, time, tracing)
  - Crate `Cargo.toml` files for `api`, `worker`, `core`, `skillrunner`
- SQLx-oriented developer tooling:
  - `Makefile` targets for `migrate` and `sqlx-prepare` (offline metadata workflow)
  - `rust-toolchain.toml` to standardize toolchain + fmt/clippy components
- Local development infrastructure:
  - `docker-compose.yml` for Postgres dev DB
  - `.gitignore` and `.editorconfig`
- CI defaults:
  - `.github/workflows/ci.yml` (fmt, clippy with `-D warnings`, test)
- Project governance/quality docs:
  - `TESTING.md` (tests-as-you-go rules, unit vs integration, DB isolation strategy, timeouts/limits)
  - `CHANGELOG.md` updated to track docs + scaffolding evolution

### Notes
- Clarified that multi-node/cluster deployments should use a shared Postgres service for durable state (runs/steps/audit), rather than per-node bundled databases.

## v0.0.1 — Initial docs + MVP scaffolding plan

### Added
- Core product/architecture documentation:
  - `docs/agent_platform.md` (platform brief + Skill Protocol v0)
  - `ARCHITECTURE.md` (system architecture + MVP definition)
- Codex guidance and guardrails:
  - `AGENTS.md` (repo instructions + non-negotiables)
- Security documentation:
  - `SECURITY.md` (security posture + forbidden patterns + deployment minimums)
  - `docs/THREAT_MODEL.md` (MVP-first threat model)
  - `docs/POLICY.md` (capability model + default-deny policy + example grants)
- Operational documentation:
  - `docs/RUNBOOK.md` (MVP run/ops notes)
  - `docs/API.md` (MVP API sketch)
- Decision records:
  - `docs/ADR/ADR-0001-out-of-process-skills.md`
  - `docs/ADR/ADR-0002-ndjson-protocol-v0.md`
  - `docs/ADR/ADR-0003-no-general-http-in-mvp.md`
- MVP implementation guidance:
  - `docs/MVP_PLAN.md` (vertical slice checklist + acceptance criteria + required tests)
  - `docs/SCHEMA.md` (MVP Postgres schema outline)
- Testing policy:
  - `TESTING.md` (unit vs integration test requirements, DB isolation strategy, timeouts/limits, “tests-as-you-go” rules)

### Notes
- MVP scope explicitly defers:
  - general `http.request` primitive (or requires strict single-host allowlist)
  - multi-tenancy
  - marketplace/signing beyond curated installs
  - microVM isolation (Firecracker/Kata)
  - UI
