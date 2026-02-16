# CHANGELOG

All notable changes to this project will be documented in this file.

This project follows a lightweight, practical changelog format. Versions are early and pre-stable.

---

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
