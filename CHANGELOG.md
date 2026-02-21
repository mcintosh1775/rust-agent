# CHANGELOG

All notable changes to this project will be documented in this file.

This project follows a lightweight, practical changelog format. Versions are early and pre-stable.

---

## v0.1.32 — Expand M10 matrix gating and execution checklist baseline

### Added
- New portability matrix gate script:
  - `scripts/ops/m10_matrix_gate.sh`
  - verifies M10 signoff + deploy preflight + checklist/docs/CI wiring
- New M10 execution evidence template:
  - `docs/M10_EXECUTION_CHECKLIST.md`
  - captures per-OS pass/fail evidence across target families
- New Makefile target:
  - `make m10-matrix-gate`
- CI portability matrix job:
  - `.github/workflows/ci.yml`
  - runs on `ubuntu-latest` and `macos-latest`
  - executes `make m10-matrix-gate`

### Changed
- `docs/CROSS_PLATFORM.md` now includes matrix-gate/checklist usage in signoff flow.
- M10 status documentation synchronized:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`

### Tests
- Verified:
  - `make m10-signoff`
  - `make deploy-preflight`
  - `make m10-matrix-gate`

## v0.1.31 — Harden M10 portability preflight/signoff workflow

### Added
- `deploy_preflight` optional compose profile validation:
  - `DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1`
  - validates compose config with detected runtime (`podman compose`, `podman-compose`, or `docker compose`)
- M10 signoff script now verifies:
  - portability checklist markers in `docs/CROSS_PLATFORM.md`
  - Makefile targets for `m10-signoff` and `deploy-preflight`
  - preflight compose-validation support wiring
- Cross-platform guide now includes a concrete portability signoff checklist:
  - baseline `m10-signoff`
  - deploy preflight
  - optional compose + manifest validation sequence

### Changed
- `scripts/ops/deploy_preflight.sh` now validates compose file presence and supports compose syntax/profile checks when enabled.
- `scripts/ops/m10_signoff.sh` now enforces stronger M10 readiness gates beyond file existence checks.
- Documentation synchronized:
  - `docs/CROSS_PLATFORM.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make m10-signoff`
  - `make deploy-preflight`
  - `DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 make deploy-preflight`

## v0.1.30 — Add LLM lane-SLO controls and ops lane telemetry (M14H + M11G baseline)

### Added
- Lane-specific LLM SLO controls:
  - `LLM_SLO_INTERACTIVE_MAX_LATENCY_MS`
  - `LLM_SLO_BATCH_MAX_LATENCY_MS`
  - `LLM_SLO_ALERT_THRESHOLD_PCT`
  - `LLM_SLO_BREACH_ESCALATE_REMOTE`
- `llm.infer` gateway metadata now includes SLO fields:
  - `gateway.slo_threshold_ms`
  - `gateway.slo_latency_ms`
  - `gateway.slo_status`
  - `gateway.slo_reason_code`
- Worker audit emission for SLO alerts:
  - `llm.slo.alert`
- New tenant ops endpoint for LLM gateway lane visibility:
  - `GET /v1/ops/llm-gateway` (owner/operator)
  - includes lane aggregates for latency, cache hit rates, verifier escalation, SLO warn/breach counts, and distributed fail-open counts
- Console panel wiring for LLM gateway lanes:
  - `/console` now includes “LLM Gateway Lanes” panel backed by `/v1/ops/llm-gateway`
  - threshold posture now factors in LLM SLO warn/breach and verifier escalation pressure
- API integration coverage for new ops endpoint:
  - `get_ops_llm_gateway_returns_lane_metrics_and_enforces_role`
- Worker unit coverage for SLO evaluation helpers.

### Changed
- Worker startup telemetry now logs lane-SLO config posture.
- Worker startup logging was split into multiple structured log records to avoid `tracing` macro recursion overflow while preserving full config visibility.
- Deployment profile/env passthrough updated for lane-SLO knobs:
  - `infra/config/profile.solo-dev.env`
  - `infra/config/profile.enterprise.env`
  - `infra/containers/compose.yml`
- Documentation synchronized:
  - `docs/API.md`
  - `QUICKSTART.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo fmt`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker llm -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p api -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p api get_ops_llm_gateway_returns_lane_metrics_and_enforces_role -- --nocapture`

## v0.1.29 — Add verifier mode framework with optional model-judge path (M14G baseline)

### Added
- Verifier mode controls for `llm.infer`:
  - `LLM_VERIFIER_MODE=heuristic|deterministic|model_judge|hybrid`
- Optional verifier model-judge endpoint controls:
  - `LLM_VERIFIER_JUDGE_BASE_URL`
  - `LLM_VERIFIER_JUDGE_MODEL`
  - `LLM_VERIFIER_JUDGE_API_KEY` / `LLM_VERIFIER_JUDGE_API_KEY_REF`
  - `LLM_VERIFIER_JUDGE_TIMEOUT_MS`
  - `LLM_VERIFIER_JUDGE_FAIL_OPEN`
- Deterministic verifier reason-code path for maintainable verifier behavior.
- Gateway metadata expansion:
  - `gateway.verifier_mode`
  - `gateway.verifier_judge_score_pct`
- Unit tests for deterministic verifier reasons and judge-response parsing.

### Changed
- `llm.infer` verifier flow now supports deterministic-only, judge-only, and hybrid score decisions.
- Gateway decision version bumped to `m14g.v1`.
- Deployment profile env templates and compose passthrough updated for new verifier controls:
  - `infra/config/profile.solo-dev.env`
  - `infra/config/profile.enterprise.env`
  - `infra/containers/compose.yml`
- Worker startup telemetry now logs verifier mode and judge configuration posture.
- Documentation synchronized:
  - `QUICKSTART.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo fmt`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker llm -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker --test worker_integration worker_process_once_executes_llm_infer_with_local_first_route -- --nocapture`
  - `make runbook-validate`

## v0.1.28 — Add optional distributed LLM gateway admission/cache controls (M14F baseline)

### Added
- Postgres-backed distributed gateway control tables:
  - `llm_gateway_admission_leases`
  - `llm_gateway_cache_entries`
- Core DB helpers for shared gateway controls:
  - distributed admission lease acquire/release
  - distributed cache upsert/get/prune
- Optional distributed LLM gateway env controls:
  - `LLM_DISTRIBUTED_ENABLED`
  - `LLM_DISTRIBUTED_FAIL_OPEN`
  - `LLM_DISTRIBUTED_OWNER`
  - `LLM_DISTRIBUTED_ADMISSION_ENABLED`
  - `LLM_DISTRIBUTED_ADMISSION_LEASE_MS`
  - `LLM_DISTRIBUTED_CACHE_ENABLED`
  - `LLM_DISTRIBUTED_CACHE_NAMESPACE_MAX_ENTRIES`

### Changed
- `llm.infer` can now use Postgres-backed shared admission and cache when distributed mode is enabled; default behavior remains local/in-process for solo/small setups.
- Worker now passes DB context into LLM execution to support shared controls.
- Gateway decision version bumped to `m14f.v1`.
- Gateway status fields now distinguish distributed paths:
  - admission: `distributed_admitted`, `distributed_fail_open_local`
  - cache: `distributed_hit`, `distributed_miss`
- Deployment profiles and compose env passthrough updated:
  - `infra/config/profile.solo-dev.env` (distributed disabled)
  - `infra/config/profile.enterprise.env` (distributed enabled baseline)
  - `infra/containers/compose.yml`
- Documentation synchronized:
  - `QUICKSTART.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo fmt`
  - `CARGO_BUILD_JOBS=2 cargo test -p core --test db_integration llm_gateway_admission_leases_enforce_lane_capacity -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker llm -- --nocapture`
  - `make runbook-validate`

## v0.1.27 — Add gateway admission, cache, and verifier-escalation controls

### Added
- Gateway admission controls in worker LLM path:
  - `LLM_ADMISSION_ENABLED`
  - `LLM_ADMISSION_INTERACTIVE_MAX_INFLIGHT`
  - `LLM_ADMISSION_BATCH_MAX_INFLIGHT`
- Gateway response cache controls:
  - `LLM_CACHE_ENABLED`
  - `LLM_CACHE_TTL_SECS`
  - `LLM_CACHE_MAX_ENTRIES`
- Verifier-based escalation controls:
  - `LLM_VERIFIER_ENABLED`
  - `LLM_VERIFIER_MIN_SCORE_PCT`
  - `LLM_VERIFIER_ESCALATE_REMOTE`
  - `LLM_VERIFIER_MIN_RESPONSE_CHARS`
- Namespace-scoped cache keying for `llm.infer` via worker-provided tenant/agent scope.

### Changed
- `llm.infer` gateway metadata expanded with admission/cache/verifier fields:
  - `admission_status`
  - `cache_status`
  - `cache_key_sha256`
  - `verifier_enabled`
  - `verifier_score_pct`
  - `verifier_threshold_pct`
  - `verifier_escalated`
  - `verifier_reason_code`
- Worker now injects run-level queue class hint into `llm.infer` args when absent.
- Deployment profile envs and compose stack passthrough updated for new gateway controls:
  - `infra/config/profile.solo-dev.env`
  - `infra/config/profile.enterprise.env`
  - `infra/containers/compose.yml`
- Documentation synchronized:
  - `QUICKSTART.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo fmt`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker llm -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker --test worker_integration -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p core --test db_integration claim_next_queued_run_prioritizes_interactive_over_batch -- --nocapture`
  - `make runbook-validate`

## v0.1.26 — Complete M14B/M14C/M14D gateway controls baseline

### Added
- M14B queue-lane baseline:
  - run-claim prioritization for `interactive` vs `batch` lanes in `claim_next_queued_run`
  - anti-starvation aging for old `batch` runs
  - optional run input lane hints:
    - `input.queue_class`
    - `input.llm_queue_class`
- M14C large-input policy engine in `llm.infer`:
  - `LLM_MAX_INPUT_BYTES`
  - `LLM_LARGE_INPUT_THRESHOLD_BYTES`
  - `LLM_LARGE_INPUT_POLICY=direct|summarize_first|chunk_and_retrieve|escalate_remote`
  - `LLM_LARGE_INPUT_SUMMARY_TARGET_BYTES`
- M14D retrieval guardrails in `llm.infer`:
  - optional action args:
    - `context_documents`
    - `context_query`
    - `context_top_k`
    - `context_max_bytes`
  - retrieval tuning env:
    - `LLM_CONTEXT_RETRIEVAL_TOP_K`
    - `LLM_CONTEXT_RETRIEVAL_MAX_BYTES`
    - `LLM_CONTEXT_RETRIEVAL_CHUNK_BYTES`

### Changed
- `llm.infer` gateway metadata expanded:
  - `gateway.request_class`
  - `gateway.queue_lane`
  - `gateway.large_input_policy`
  - `gateway.large_input_applied`
  - `gateway.large_input_reason_code`
  - `gateway.prompt_bytes_original`
  - `gateway.prompt_bytes_effective`
  - `gateway.retrieval_candidate_documents`
  - `gateway.retrieval_selected_documents`
- Policy-scope and execution now use the same prompt-planning path to keep route/reason decisions deterministic.
- Worker startup telemetry now includes large-input/retrieval gateway control settings.
- Container stack env passthrough and deployment profile presets now include M14B/C/D control knobs.
- Documentation synchronized:
  - `docs/API.md`
  - `QUICKSTART.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo fmt`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker llm -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker --test worker_integration -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p core --test db_integration claim_next_queued_run_prioritizes_interactive_over_batch -- --nocapture`

## v0.1.25 — Implement M14A gateway baseline + dual profile wiring

### Added
- LLM gateway decision metadata in `llm.infer` action results:
  - `gateway.version`
  - `gateway.mode`
  - `gateway.selected_route`
  - `gateway.reason_code`
  - `gateway.remote_egress_class`
  - `gateway.remote_host`
- New remote egress classification control:
  - `LLM_REMOTE_EGRESS_CLASS=cloud_allowed|redacted_only|never_leaves_prem`
  - `redacted_only` requires `llm.infer` action args `redacted=true`
- New deployment profile presets:
  - `infra/config/profile.solo-dev.env`
  - `infra/config/profile.enterprise.env`

### Changed
- `llm.infer` routing now produces deterministic gateway reason codes for route selection/fallback paths.
- Container stack now consumes deployment-profile env posture through compose substitution:
  - `infra/containers/compose.yml`
- Worker startup telemetry now includes `llm_remote_egress_class`.
- Worker integration/unit coverage expanded for egress-class deny/allow behavior.
- Documentation synchronized for profile usage and gateway/egress operations:
  - `QUICKSTART.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo fmt`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker llm -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p worker --test worker_integration -- --nocapture`
  - `make runbook-validate`

## v0.1.24 — Refine M14 with dual deployment profiles (solo/dev + enterprise)

### Changed
- M14 now explicitly requires two supported deployment profiles with one shared product/API surface:
  - `solo/dev` profile for non-enterprise users (minimal setup, remote-only friendly)
  - `enterprise` profile for hardened/compliance-heavy environments
- M14 now codifies that enterprise controls are additive via configuration, not mandatory for basic agent usage.
- M14 exit criteria now include profile-compatibility validation and profile-specific runbook coverage.
- Session handoff updated to call out dual-profile M14 requirements:
  - `docs/SESSION_HANDOFF.md`

### Tests
- Not run (documentation-only update).

## v0.1.23 — Draft M14 LLM gateway milestone (remote-first now, hybrid-ready)

### Added
- New roadmap milestone:
  - `M14 — LLM Gateway and Tiered Model Routing (Post-MVP)` in `docs/ROADMAP.md`
- M14 defines:
  - centralized LLM gateway contract
  - tiered routing model (Tier 0/1/2)
  - remote-only-first operation profile with future on-prem local-tier activation
  - escalation policy triggers and reason-code requirements
  - egress classification policy (`never_leaves_prem`, `redacted_only`, `cloud_allowed`)
  - caching/admission-control/observability expectations

### Changed
- Session handoff now tracks M14 draft status and prioritizes gateway implementation sequencing:
  - `docs/SESSION_HANDOFF.md`

### Tests
- Not run (documentation-only update).

## v0.1.22 — Complete M12C agent-context control-plane baseline and M13B docs sync

### Added
- Core agent-context helpers:
  - mutability classifier for canonical profile paths
  - heartbeat intent compiler with typed candidates/issues and cron/timezone validation
  - canonical summary digest helper (`summary_digest_sha256`)
- API operator endpoints:
  - `GET /v1/agents/{id}/context`
  - `POST /v1/agents/{id}/heartbeat/compile`
- API mutation endpoint (opt-in):
  - `POST /v1/agents/{id}/context`
  - guarded by `API_AGENT_CONTEXT_MUTATION_ENABLED=1`
- API integration coverage:
  - `agent_context_inspect_and_heartbeat_compile_endpoints_work`
  - `agent_context_mutation_enforces_mutability_boundaries`

### Changed
- Agent-context mutation now enforces mutability boundaries:
  - immutable files denied
  - human-primary files owner-only
  - agent-managed files owner/operator
  - `sessions/*.jsonl` append-only
- Agent-context inspect/compile responses include checksum provenance (`aggregate_sha256`, summary digest).
- Documentation synchronization for M12/M13:
  - `docs/API.md`
  - `docs/AGENT_FILES.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `QUICKSTART.md`

### Tests
- Verified:
  - `CARGO_BUILD_JOBS=2 cargo test -p core agent_context -- --nocapture`
  - `CARGO_BUILD_JOBS=2 cargo test -p api --test api_integration -- --nocapture`

## v0.1.21 — Implement M12B runtime agent-context loader and worker profile injection

### Added
- New typed agent-context module in core:
  - `core/src/agent_context.rs`
  - loader/validator for per-agent profile files
  - canonical/default required file support
  - tenant-aware source resolution + flat fallback
  - bounded file/total size controls
  - dynamic `memory/*.md` and `sessions/*.jsonl` loading (capped)
- New worker context bootstrap script:
  - `scripts/ops/init_agent_context.sh`
- New Make target:
  - `make agent-context-init`
- Worker integration coverage:
  - `worker_process_once_loads_agent_context_profile_and_audits_manifest`
  - `worker_process_once_fails_when_required_agent_context_missing`

### Changed
- Worker runtime now supports agent-context profile loading and skill-input injection under `agent_context`.
- Worker emits context lifecycle audit events:
  - `agent.context.loaded`
  - `agent.context.not_found`
  - `agent.context.error`
- Worker config/env expanded with context controls:
  - `WORKER_AGENT_CONTEXT_ENABLED`
  - `WORKER_AGENT_CONTEXT_REQUIRED`
  - `WORKER_AGENT_CONTEXT_ROOT`
  - `WORKER_AGENT_CONTEXT_REQUIRED_FILES`
  - `WORKER_AGENT_CONTEXT_MAX_FILE_BYTES`
  - `WORKER_AGENT_CONTEXT_MAX_TOTAL_BYTES`
  - `WORKER_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR`
- Worker startup telemetry now reports context-loader config posture.
- Container stack worker service now supports context env passthrough and read-only context mount:
  - `../../agent_context:/var/lib/secureagnt/agent-context:ro`
- Docs updated:
  - `docs/AGENT_FILES.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/OPERATIONS_MANUAL.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `QUICKSTART.md`

### Tests
- Verified:
  - `CARGO_BUILD_JOBS=2 cargo test -p core agent_context -- --nocapture`
  - `make test-worker-db`

## v0.1.20 — Add enterprise-depth operations manual baseline

### Added
- New comprehensive operations manual:
  - `docs/OPERATIONS_MANUAL.md`
  - covers topology, day-0/day-1/day-2 operations, SLOs, incident playbooks, DR, security, release/change control, and tenant lifecycle workflows

### Changed
- Existing concise ops docs now reference the full manual:
  - `docs/OPERATIONS.md`
  - `docs/RUNBOOK.md`
- Documentation index now includes operations manual:
  - `docs/README.md`
- Roadmap now tracks operations-documentation milestone:
  - `M13 — Operations Excellence Documentation`
- Session handoff now includes M13A status and next-session prompt reads `docs/OPERATIONS_MANUAL.md`.

### Tests
- Not run (documentation-only update).

## v0.1.19 — Add M12A agent context files profile and precedence ADR baseline

### Added
- New agent-context profile doc:
  - `docs/AGENT_FILES.md`
- New architectural decision record:
  - `docs/ADR/ADR-0009-agent-context-files-profile.md`

### Changed
- `AGENTS.md` preload list now includes:
  - `docs/AGENT_FILES.md`
- Roadmap now includes M12 planning baseline for agent context profile:
  - precedence model
  - mutability boundaries
  - heartbeat intent-to-trigger posture
- Session handoff now captures M12A status and updates next-session bootstrap prompt to include `docs/AGENT_FILES.md`.
- Docs index now includes `docs/AGENT_FILES.md`.

### Tests
- Not run (documentation/ADR updates only).

## v0.1.18 — Complete M11F compliance alert acknowledgment workflow baseline

### Added
- New migration:
  - `migrations/0017_compliance_siem_alert_acks.sql`
- New core DB alert-ack persistence API:
  - `upsert_tenant_compliance_siem_delivery_alert_ack(...)`
  - `list_tenant_compliance_siem_delivery_alert_acks(...)`
- New API endpoint:
  - `POST /v1/audit/compliance/siem/deliveries/alerts/ack`
- New API integration coverage:
  - `compliance_alert_acknowledge_marks_alert_and_enforces_user_header`

### Changed
- `GET /v1/audit/compliance/siem/deliveries/alerts` now returns acknowledgment metadata per alert row:
  - `acknowledged`
  - `acknowledged_at`
  - `acknowledged_by_user_id`
  - `acknowledged_by_role`
  - `acknowledgement_note`
- `/console` now supports alert workflow actions:
  - optional `x-user-id` control
  - `Acknowledge Alert` action bound to SIEM delivery alert ack endpoint
  - optional run-scoped acknowledgement support
- Roadmap/operations/development/session docs updated for M11F status and operator workflow guidance.

### Tests
- Verified:
  - `make test-api-db`

## v0.1.17 — Complete M11E trusted-proxy auth boundary hardening baseline

### Added
- API trusted-proxy auth enforcement controls:
  - `API_TRUSTED_PROXY_AUTH_ENABLED`
  - `API_TRUSTED_PROXY_SHARED_SECRET`
  - `API_TRUSTED_PROXY_SHARED_SECRET_REF`
- API test wiring helper:
  - `app_router_with_trusted_proxy_auth(...)`
- New API integration coverage:
  - `trusted_proxy_auth_enforces_proxy_token_on_role_scoped_endpoints`

### Changed
- Role/user-header API flows now enforce trusted proxy token validation when enabled.
- Requests missing or using invalid `x-auth-proxy-token` now fail with `401 UNAUTHORIZED`.
- `/console` now supports an optional `Auth Proxy Token` control and forwards `x-auth-proxy-token` on panel fetches when set.
- API integration console shell assertions now include trusted-proxy auth markers.
- Docs updated:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-api-db`

## v0.1.16 — Add M10 cross-platform signoff scaffold and portability baseline doc

### Added
- New M10 signoff script:
  - `scripts/ops/m10_signoff.sh`
- New Make target:
  - `make m10-signoff`
- New portability baseline doc:
  - `docs/CROSS_PLATFORM.md`
  - includes OS-family notes (Ubuntu/Debian, Fedora/RHEL, Arch, openSUSE, macOS)
  - includes systemd/launchd and container baseline references

### Changed
- Roadmap and session handoff now track M10 signoff scaffold progress.
- Development/operations docs now include `make m10-signoff` in operator signoff workflows.

### Tests
- Verified:
  - `make m10-signoff`

## v0.1.15 — Harden trigger semantics, status typing, and availability signaling

### Added
- New trigger enqueue availability outcome model in `core/src/db.rs`:
  - `TriggerEventEnqueueOutcome::TriggerUnavailable { reason }`
  - reasons include:
    - `TriggerNotFound`
    - `TriggerDisabled`
    - `TriggerTypeMismatch`
    - `TriggerScheduleBroken`
- New core integration coverage:
  - `enqueue_trigger_event_returns_unavailable_reasons_for_non_dispatchable_triggers`
- New API integration coverage:
  - `webhook_trigger_event_ingest_returns_conflict_when_trigger_is_disabled`
- Trigger error payload normalization helper:
  - trigger failure metadata now consistently includes `code`, `message`, and `reason_class`

### Changed
- Trigger enqueue now distinguishes unavailable trigger state from duplicate event-id idempotency.
- API webhook event ingest now maps trigger-unavailable state to explicit API errors (including `409 CONFLICT` for unavailable trigger state).
- Trigger replay requeue path removes redundant status re-fetch on locked rows and uses stricter invariant handling.
- Trigger mutation/ingest/replay unavailable-state responses now use `409 CONFLICT` where appropriate instead of generic `400`.
- Trigger failure metadata for misfire/cron/event-size paths now uses consistent reason-classed payloads.

### Documentation
- Updated:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `RUN_DB_TESTS=1 cargo test -p core --test db_integration enqueue_trigger_event_returns_unavailable_reasons_for_non_dispatchable_triggers -- --nocapture`
  - `RUN_DB_TESTS=1 cargo test -p core --test db_integration enqueue_and_dispatch_webhook_trigger_event_creates_run -- --nocapture`
  - `RUN_DB_TESTS=1 cargo test -p api --test api_integration webhook_trigger_event_ingest_returns_conflict_when_trigger_is_disabled -- --nocapture`
  - `RUN_DB_TESTS=1 cargo test -p api --test api_integration replay_dead_lettered_trigger_event_requeues_event -- --nocapture`

## v0.1.14 — Complete M11D console thresholds/export and regression marker baseline

### Added
- M11D threshold posture chips in `api/static/console.html`:
  - run failures and run p95 latency
  - remote token burn
  - payment failures
  - SIEM dead-letter and hard-failure rates
- Console export actions:
  - `Export Snapshot JSON` (controls + thresholds + all panel payloads)
  - `Export Health JSON` (focused operational summary payload)
- Console now stores panel payload metadata in-memory for export and threshold evaluation.
- API integration assertion updates in `api/tests/api_integration.rs`:
  - console shell includes export controls and threshold marker IDs
  - console shell includes role/error marker strings (`ROLE_FORBIDDEN`, `FORBIDDEN`, `FETCH_FAILED`, `INPUT_REQUIRED`)

### Changed
- M11 roadmap/session status now tracks M11 as baseline-complete (M11A through M11D).
- Quickstart/Operations/API docs now include console threshold and export workflow notes.

### Tests
- Verified:
  - `RUN_DB_TESTS=1 cargo test -p api --test api_integration console_index_route_serves_html_shell -- --nocapture`

## v0.1.13 — Complete M11C console drill-down and persisted-operator-controls baseline

### Added
- M11C console drill-down panels in `api/static/console.html`:
  - run latency traces (`/v1/ops/latency-traces`)
  - action latency traces (`/v1/ops/action-latency-traces`)
  - run detail (`/v1/runs/:id`)
  - run audit (`/v1/runs/:id/audit`)
  - payments ledger (`/v1/payments`)
  - compliance delivery alerts (`/v1/audit/compliance/siem/deliveries/alerts`)
- New `Load Run Context` control to refresh run detail/audit panels for selected `run-id`.
- Console filter persistence (tenant/role/window/run/agent/action/limits) via local storage key:
  - `secureagnt_console_controls_v1`
- Run trace refresh can auto-select `run-id` from latest trace entry when none is set.
- API integration assertion updates in `api/tests/api_integration.rs`:
  - confirms drill-down shell markers and local-storage key marker are present.

### Changed
- M11 roadmap/session status now tracks M11C as complete and M11D as in progress.
- Quickstart/Operations/API docs now describe run-context drill-down workflow and persisted filters.

### Tests
- Verified:
  - `RUN_DB_TESTS=1 cargo test -p api --test api_integration console_index_route_serves_html_shell -- --nocapture`

## v0.1.12 — Complete M11B console RBAC/error-state hardening baseline

### Added
- M11B console shell behavior in `api/static/console.html`:
  - role selector now supports `viewer`
  - per-panel role restriction rendering (`ROLE_FORBIDDEN`) for insufficient role
  - explicit panel rendering for API `403` fetch failures (`FORBIDDEN`)
  - inline RBAC header guidance in console controls
- API integration assertion updates in `api/tests/api_integration.rs`:
  - console shell includes `viewer` role option
  - console shell includes RBAC marker strings

### Changed
- M11 roadmap/session status now tracks M11A + M11B as complete and M11C as in progress.
- Quickstart/Operations/API docs now describe viewer behavior on console reporting panels.

### Tests
- Verified:
  - `RUN_DB_TESTS=1 cargo test -p api --test api_integration console_index_route_serves_html_shell -- --nocapture`

## v0.1.11 — Start M11A web operations console baseline

### Added
- New M11A implementation plan:
  - `docs/M11A_PLAN.md`
- API-served web console shell:
  - `GET /console`
  - static console asset: `api/static/console.html`
- New API integration coverage:
  - `console_index_route_serves_html_shell`

### Changed
- Quickstart now includes console access (`http://localhost:8080/console`) for stack workflows.
- API docs now include console shell endpoint behavior and header notes.
- Roadmap/session handoff now track M11A as in-progress baseline.

### Tests
- Verified:
  - `RUN_DB_TESTS=1 cargo test -p api --test api_integration console_index_route_serves_html_shell -- --nocapture`

## v0.1.10 — Close M9 governance milestone with enforcement and sign-off gate

### Added
- New M9 sign-off script:
  - `scripts/ops/m9_signoff.sh`
- New make target:
  - `make m9-signoff`
- Worker governance controls:
  - `WORKER_APPROVAL_REQUIRED_ACTION_TYPES` for explicit approval-gated action enforcement
  - `WORKER_SKILL_SCRIPT_SHA256` for skill script provenance digest verification
- New worker integration coverage:
  - deny path when governance approval is required and absent
  - allow path when governance approval is provided
  - skill invoke failure path when configured script digest mismatches

### Changed
- Roadmap milestone status updated:
  - `M9` marked completed with explicit sign-off automation.
- Worker startup telemetry now reports governance gate configuration state.

### Documentation
- Updated:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/POLICY.md`
  - `Makefile` target surface

### Tests
- Verified:
  - `make m9-signoff`

## v0.1.9 — Close M8 production-readiness milestone with sign-off gate

### Added
- New M8 sign-off script:
  - `scripts/ops/m8_signoff.sh`
- New make target:
  - `make m8-signoff`
- New ops fixture:
  - `agntctl/fixtures/ops_action_latency_candidate_ok.json`

### Changed
- Soak gate automation now supports fixture-backed summary input:
  - `scripts/ops/soak_gate.sh`
  - new env passthrough `SUMMARY_JSON` (`--summary-json`)
- Roadmap milestone status updated:
  - `M8` marked completed with explicit sign-off automation.
- Session handoff updated to remove M8 from pending priorities.

### Documentation
- Updated:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `Makefile` target surface

### Tests
- Verified:
  - `make m8-signoff`

## v0.1.8 — Close M6A durable memory milestone with sign-off gate

### Added
- New M6A sign-off script:
  - `scripts/ops/m6a_signoff.sh`
- New make target:
  - `make m6a-signoff`

### Changed
- Roadmap milestone status updated:
  - `M6A` marked completed with explicit sign-off automation.
- Session handoff updated to remove M6A from pending priorities.

### Documentation
- Updated:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `Makefile` target surface

### Tests
- Verified:
  - `make m6a-signoff`

## v0.1.7 — Close M6 security hardening milestone with sign-off gate

### Added
- New M6 sign-off script:
  - `scripts/ops/m6_signoff.sh`
- New make target:
  - `make m6-signoff`

### Changed
- Roadmap milestone status updated:
  - `M6` marked completed with explicit sign-off automation.
- Session handoff updated to remove M6 from pending priorities.

### Documentation
- Updated:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `Makefile` target surface

### Tests
- Verified:
  - `make m6-signoff`

## v0.1.6 — Close M5C payments milestone with sign-off gate

### Added
- New payment milestone sign-off script:
  - `scripts/ops/m5c_signoff.sh`
- New make target:
  - `make m5c-signoff`
- New worker integration test:
  - `worker_process_once_denies_payment_send_without_capability`

### Changed
- Roadmap milestone status updated:
  - `M5C` marked completed with explicit sign-off automation.
- Session handoff updated to reflect completed M5C status and refreshed next-step priorities.

### Documentation
- Updated:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/PAYMENTS.md`
  - `Makefile` target surface

### Tests
- Verified:
  - `make m5c-signoff`

## v0.1.5 — Close M7 and M8A with explicit sign-off automation

### Added
- Milestone sign-off scripts:
  - `scripts/ops/m7_signoff.sh`
  - `scripts/ops/m8a_signoff.sh`
- New make targets:
  - `make m7-signoff`
  - `make m8a-signoff`

### Changed
- Roadmap milestone status updates:
  - `M7` marked completed (tenant hardening sign-off)
  - `M8A` marked completed (compliance-plane sign-off)
- Session handoff updated to reflect completed M7/M8A status and refreshed next-step priorities.

### Documentation
- Updated:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `Makefile` target listing/automation surface

### Tests
- Verified:
  - `make m7-signoff`
  - `make m8a-signoff`

## v0.1.4 — Add tenant memory capacity guardrail to API

### Added
- New API tenant quota control:
  - `API_TENANT_MAX_MEMORY_RECORDS`
  - enforced on memory write endpoints:
    - `POST /v1/memory/records`
    - `POST /v1/memory/handoff-packets`
- New API integration coverage:
  - `create_memory_record_enforces_tenant_memory_capacity_limit`

### Changed
- API returns `429 TENANT_MEMORY_LIMITED` when tenant active memory row count is at/above configured capacity.
- Active memory capacity counting excludes compacted/expired rows.

### Documentation
- Updated:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `CARGO_BUILD_JOBS=2 RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p api --test api_integration create_memory_record_enforces_tenant_memory_capacity_limit -- --nocapture`

## v0.1.3 — SIEM delivery non-retryable dead-letter hardening

### Added
- New core DB helper:
  - `mark_compliance_siem_delivery_record_dead_lettered(...)`
  - supports immediate dead-letter transitions for permanent delivery failures.
- New integration coverage:
  - `core`: `compliance_siem_delivery_outbox_can_be_force_dead_lettered`
  - `worker`: `worker_process_once_dead_letters_siem_http_non_retryable_failure`

### Changed
- Worker SIEM delivery now classifies failures as retryable vs non-retryable.
- Non-retryable SIEM failures now dead-letter immediately (single attempt), including:
  - HTTP `400`, `401`, `403`, `404`, `405`, `410`, `422`
  - unsupported target/configuration failures.

### Documentation
- Updated:
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `CARGO_BUILD_JOBS=2 RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p core --test db_integration compliance_siem_delivery_outbox_can_be_force_dead_lettered -- --nocapture`
  - `CARGO_BUILD_JOBS=2 RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p worker --test worker_integration dead_letters_siem_http_non_retryable_failure -- --nocapture`

## v0.1.2 — Add Cashu route orchestration parity controls

### Added
- Cashu routing controls now match NWC orchestration behavior:
  - `PAYMENT_CASHU_ROUTE_STRATEGY` (`ordered`/`deterministic_hash`)
  - `PAYMENT_CASHU_ROUTE_FALLBACK_ENABLED`
  - `PAYMENT_CASHU_ROUTE_ROLLOUT_PERCENT`
  - `PAYMENT_CASHU_ROUTE_HEALTH_FAIL_THRESHOLD`
  - `PAYMENT_CASHU_ROUTE_HEALTH_COOLDOWN_SECS`
- Cashu payment results now include route metadata under `result.route` for reconciliation/debug.
- New worker integration tests:
  - `worker_process_once_executes_payment_send_with_cashu_http_route_failover`
  - `worker_process_once_does_not_fail_over_between_cashu_routes_when_disabled`

### Changed
- Cashu mint route values now support multi-route entries (`uri_a|uri_b`) with deterministic selection/failover behavior.
- Worker startup telemetry now logs Cashu route-control configuration.

### Documentation
- Updated:
  - `docs/PAYMENTS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `CARGO_BUILD_JOBS=2 RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p worker --test worker_integration cashu_http_route -- --nocapture`
  - `CARGO_BUILD_JOBS=2 RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p worker --test worker_integration cashu_routes_when_disabled -- --nocapture`

## v0.1.1 — Add one-command quickstart seeding

### Added
- New helper target:
  - `make quickstart-seed`
- New helper script:
  - `scripts/ops/quickstart_seed.sh`
  - generates `AGENT_ID` + `USER_ID` (unless provided)
  - inserts agent/user rows for quickstart use
  - supports local `psql` path or compose-exec fallback (`podman`/`podman-compose`/`docker`)
  - prints export lines for immediate shell use

### Documentation
- Updated:
  - `QUICKSTART.md` (now uses `make quickstart-seed` as default seed path)
  - `docs/SESSION_HANDOFF.md` (local verification commands include `make quickstart-seed`)

## v0.1.0 — Add container-first quickstart guide and promote new release series

### Added
- New root quickstart guide:
  - `QUICKSTART.md`
  - covers Podman/Docker stack bootstrap (`postgres` + `secureagnt-api` + `secureagntd`)
  - covers first API interactions (seed IDs, create run, status, audit, ops checks)
  - covers `agntctl` usage against container API (`AGNTCTL_API_BASE_URL=http://localhost:8080`)
  - includes current web-console status note (M11 pending)

### Documentation
- Updated:
  - `docs/README.md` (quickstart linked in docs list)
  - `docs/SESSION_HANDOFF.md` (read order + new-session prompt include quickstart)

## v0.0.111 — Add SIEM delivery alerts endpoint for compliance observability

### Added
- New compliance observability endpoint:
  - `GET /v1/audit/compliance/siem/deliveries/alerts`
  - computes threshold-based target alert rows from SIEM delivery counters/rates.
- Alert response includes:
  - threshold echo (`max_hard_failure_rate_pct`, `max_dead_letter_rate_pct`, `max_pending_count`)
  - per-target rates (`hard_failure_rate_pct`, `dead_letter_rate_pct`)
  - `triggered_rules` and `severity` (`warning`/`critical`)
- New API integration coverage:
  - `get_compliance_audit_siem_delivery_alerts_returns_breaches_and_enforces_role`

### Changed
- `GET /v1/audit/compliance/siem/deliveries/targets` now honors `window_secs` filtering.

### Documentation
- Updated:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p api --test api_integration get_compliance_audit_siem_delivery_alerts_returns_breaches_and_enforces_role -- --nocapture`

## v0.0.110 — Add action-latency traces endpoint and soak action-rate thresholds

### Added
- New tenant ops endpoint:
  - `GET /v1/ops/action-latency-traces`
  - returns per-action trace samples (`action_request_id`, `run_id`, `step_id`, `action_type`, `status`, `duration_ms`, `created_at`, `executed_at`).
- New core DB query:
  - `get_tenant_action_latency_traces(...)`
- `agntctl ops soak-gate` now supports action-rate threshold tuning:
  - `--max-action-failed-rate-pct`
  - `--max-action-denied-rate-pct`
- `scripts/ops/soak_gate.sh` supports:
  - `MAX_ACTION_FAILED_RATE_PCT`
  - `MAX_ACTION_DENIED_RATE_PCT`

### Changed
- `agntctl ops soak-gate` action-latency evaluation now supports combined threshold checks:
  - p95 duration (existing)
  - failed-rate percentage
  - denied-rate percentage

### Documentation
- Updated:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `CARGO_BUILD_JOBS=2 cargo test -p agntctl`
  - `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p core --test db_integration tenant_action_latency_traces_are_filtered_and_tenant_scoped -- --nocapture`
  - `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p api --test api_integration get_ops_action_latency_traces_returns_recent_actions_and_enforces_role -- --nocapture`

## v0.0.109 — Add memory retrieval quality controls (scored query/filter retrieval)

### Added
- `GET /v1/memory/retrieve` now supports optional retrieval-quality controls:
  - `query_text`
  - `min_score`
  - `source_prefix`
  - `require_summary`
- Retrieval response payload now includes:
  - per-item `score` (`0.0..2.0`)
  - echoed retrieval controls (`query_text`, `min_score`, `source_prefix`, `require_summary`)
- API integration coverage for:
  - scored/source-filtered retrieval behavior
  - invalid `min_score` rejection

### Changed
- Retrieval scoring path now pre-tokenizes query terms once per request.
- `require_summary=true` now requires a non-empty summary value.

### Documentation
- Updated:
  - `docs/API.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p api --test api_integration memory_retrieve -- --nocapture`

## v0.0.108 — Add action-latency ops endpoint and soak-gate action thresholds

### Added
- New tenant ops endpoint:
  - `GET /v1/ops/action-latency`
  - returns action-type aggregates (`total_count`, `avg/p95/max duration`, `failed_count`, `denied_count`).
- New core DB query:
  - `get_tenant_action_latency_summary(...)`
- `agntctl ops soak-gate` now supports action-path thresholding:
  - `--max-action-p95-ms`
  - `--action-latency-json`
- `scripts/ops/soak_gate.sh` supports:
  - `MAX_ACTION_P95_MS`
  - `ACTION_LATENCY_JSON`
- New integration/unit coverage:
  - `core`: `tenant_action_latency_summary_is_tenant_scoped_and_reports_status_mix`
  - `api`: `get_ops_action_latency_returns_action_metrics_and_enforces_role`
  - `agntctl`: `action_latency_eval_collects_threshold_failures`

### Documentation
- Updated:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p core --test db_integration tenant_action_latency_summary_is_tenant_scoped_and_reports_status_mix -- --nocapture`
  - `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p api --test api_integration get_ops_action_latency_returns_action_metrics_and_enforces_role -- --nocapture`
  - `cargo test -p agntctl`

## v0.0.107 — Add per-target SIEM compliance thresholds to operator gate

### Added
- `agntctl ops compliance-gate` now supports target-level SIEM checks:
  - `--targets-json`
  - `--max-target-hard-failure-rate-pct`
  - `--max-target-dead-letter-rate-pct`
  - `--max-target-pending-count`
- Compliance-gate API fetch path for target summaries:
  - `GET /v1/audit/compliance/siem/deliveries/targets`
- New `agntctl` unit coverage for target-threshold evaluation.

### Changed
- `scripts/ops/compliance_gate.sh` now accepts/pass-through env knobs:
  - `TARGETS_JSON`
  - `MAX_TARGET_HARD_FAILURE_RATE_PCT`
  - `MAX_TARGET_DEAD_LETTER_RATE_PCT`
  - `MAX_TARGET_PENDING_COUNT`

### Documentation
- Updated:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test -p agntctl`

## v0.0.106 — Harden Cashu live transport semantics and normalized settlement results

### Changed
- Cashu live HTTP transport now uses explicit method semantics:
  - `pay_invoice` -> `POST /v1/pay_invoice`
  - `make_invoice` -> `POST /v1/make_invoice`
  - `get_balance` -> `GET /v1/balance`
- Cashu live payment results now include normalized reconciliation fields under `result`:
  - `pay_invoice`: `settlement_status`, `payment_hash`, `payment_preimage`, `fee_msat`
  - `make_invoice`: `invoice`, `payment_hash`, `amount_msat`
  - `get_balance`: `balance_msat`

### Added
- Worker integration tests for Cashu live HTTP operation coverage:
  - `worker_process_once_executes_payment_send_with_cashu_http_make_invoice`
  - `worker_process_once_executes_payment_send_with_cashu_http_get_balance`

### Documentation
- Updated:
  - `docs/PAYMENTS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `RUN_DB_TESTS=1 TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb cargo test -p worker --test worker_integration worker_process_once_executes_payment_send_with_cashu_http -- --nocapture`

## v0.0.105 — Add full containerized runtime stack (API + worker + Postgres)

### Added
- Container runtime images:
  - `infra/containers/Dockerfile.api`
  - `infra/containers/Dockerfile.worker`
- Compose `stack` profile service wiring in `infra/containers/compose.yml`:
  - `postgres` (default profile)
  - `api` and `worker` (`stack` profile)
- New Makefile targets:
  - `make stack-build`
  - `make stack-up`
  - `make stack-up-build`
  - `make stack-down`
  - `make stack-ps`
  - `make stack-logs`
- Repository `.dockerignore` for faster/cleaner container builds.

### Changed
- Compose Postgres service now includes healthchecks.
- API supports optional startup migration execution via `API_RUN_MIGRATIONS=1`; compose `stack` profile enables it by default.
- Container image builds now pass through a throttled cargo job cap via `SECUREAGNT_CARGO_BUILD_JOBS` (default `2`).
- `make stack-up` now starts the stack without forcing rebuild; rebuild is explicit via `make stack-build` or `make stack-up-build`.
- Local cargo-heavy Makefile targets now default to `CARGO_BUILD_JOBS=2` (overrideable per invocation).
- Container docs now explicitly support two run modes:
  - host binaries (`make secureagnt-api`, `make secureagntd`)
  - full containerized stack (`make stack-up`).

### Documentation
- Updated:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/ROADMAP.md`

### Tests
- Verified:
  - `make container-info`
  - `make governance-gate`

## v0.0.104 — Advance M9 with governance gate enforcement wiring

### Added
- New governance supply-chain gate script:
  - `scripts/ops/governance_gate.sh`
  - enforces release manifest generation + verification and deploy preflight with manifest verification enabled.
- New Makefile target:
  - `make governance-gate`

### Changed
- Validation/release gate integration now supports governance enforcement:
  - `scripts/ops/validation_gate.sh` adds `VALIDATION_GATE_RUN_GOVERNANCE` (default enabled).
  - `scripts/ops/release_gate.sh` adds `RELEASE_GATE_RUN_GOVERNANCE` pass-through.
- M9 roadmap status expanded from scaffold-only to enforced gate workflow.

### Documentation
- Updated:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/ROADMAP.md`

### Tests
- Verified:
  - `make governance-gate`
  - `make validation-gate`

## v0.0.103 — Harden M5C idempotent payment replay semantics

### Changed
- Worker idempotent replay behavior for `payment.send` now preserves original settlement status:
  - duplicate requests no longer overwrite `payment_requests.status`.
  - duplicate action results now include `prior_request_status` metadata.

### Added
- Worker integration coverage:
  - `worker_process_once_reuses_payment_result_on_idempotent_replay`
  - validates replayed idempotency keys do not create duplicate ledger settlements.
- Core DB integration coverage:
  - `payment_request_idempotency_key_is_scoped_by_tenant`
  - validates same idempotency key can be used across tenants without cross-tenant dedupe.

### Documentation
- M5C roadmap status updated with idempotent replay hardening and tenant-scoped replay coverage.

### Tests
- Verified:
  - `make test-db`
  - `make test-worker-db`

## v0.0.102 — Advance M7 with tenant isolation regression gate coverage

### Added
- New worker integration test:
  - `worker_process_once_isolates_message_outbox_by_tenant`
  - validates cross-tenant `message.send` outbox artifacts stay isolated on shared artifact roots.
- New isolation gate automation script:
  - `scripts/ops/isolation_gate.sh`
- New Makefile target:
  - `make isolation-gate`

### Changed
- `scripts/ops/validation_gate.sh` now runs `make isolation-gate` whenever DB validation suites are enabled (`VALIDATION_GATE_RUN_DB_SUITES=1`).
- M7 docs/handoff updates in:
  - `docs/ROADMAP.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make isolation-gate`
  - `make validation-gate VALIDATION_GATE_RUN_DB_SUITES=1`

## v0.0.101 — Advance M8A with compliance durability gate automation

### Added
- New `agntctl` operator command:
  - `agntctl ops compliance-gate`
  - validates compliance tamper-chain verification state and SIEM delivery SLO thresholds.
- New compliance gate automation script:
  - `scripts/ops/compliance_gate.sh`
- New Makefile target:
  - `make compliance-gate`
- New fixture inputs for local/offline compliance gate runs:
  - `agntctl/fixtures/compliance_verify_ok.json`
  - `agntctl/fixtures/compliance_slo_ok.json`

### Changed
- Validation/release gate composition now includes compliance durability checks by default:
  - `scripts/ops/validation_gate.sh`
  - `scripts/ops/release_gate.sh`
- Added compliance gate controls:
  - `VALIDATION_GATE_RUN_COMPLIANCE`
  - `RELEASE_GATE_RUN_COMPLIANCE`
- M8A ops docs/handoff updates in:
  - `docs/ROADMAP.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test -p agntctl`
  - `make compliance-gate VERIFY_JSON=agntctl/fixtures/compliance_verify_ok.json SLO_JSON=agntctl/fixtures/compliance_slo_ok.json`
  - `make validation-gate`

## v0.0.100 — Advance M6A with expiration-safe memory retrieval and load benchmark coverage

### Added
- New DB integration benchmark coverage:
  - `memory_retrieval_under_concurrent_load_is_tenant_isolated_and_bounded`
  - validates tenant isolation under concurrent retrieval load and enforces elapsed-time threshold (`MEMORY_RETRIEVAL_BENCH_MAX_MS`, default `15000`).

### Changed
- Memory retrieval/list query paths now exclude expired records immediately (before purge) for:
  - `list_tenant_memory_records(...)`
  - `list_tenant_handoff_memory_records(...)`
- Memory compaction candidate selection now excludes expired source records.
- API memory list expectations updated to reflect immediate expired-record exclusion.
- M6A docs/handoff updates:
  - `docs/ROADMAP.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`

## v0.0.99 — Advance M7 with tenant-scoped worker artifact isolation

### Added
- New worker integration coverage:
  - `worker_process_once_isolates_artifacts_by_tenant`
  - validates identical relative artifact paths across different tenants do not collide on shared filesystem roots.

### Changed
- Worker side-effect artifact writes are now tenant-scoped on disk for:
  - `object.write`
  - `message.send` outbox
  - `payment.send` outbox
- Artifacts now write under:
  - `<WORKER_ARTIFACT_ROOT>/tenants/<tenant_id>/...`
- Updated docs/handoff to reflect tenant artifact isolation:
  - `docs/ROADMAP.md`
  - `docs/OPERATIONS.md`
  - `docs/DEVELOPMENT.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-worker-db`

## v0.0.98 — Advance M9/M10 deployment preparation and provenance scaffold

### Added
- New deployment integrity/provenance scripts:
  - `scripts/ops/generate_release_manifest.sh`
  - `scripts/ops/verify_release_manifest.sh`
- New deployment preflight script:
  - `scripts/ops/deploy_preflight.sh`
- New Makefile targets:
  - `make release-manifest`
  - `make release-manifest-verify`
  - `make deploy-preflight`
- New cross-platform packaging scaffolds:
  - `infra/launchd/secureagnt.plist`
  - `infra/launchd/secureagnt-api.plist`
  - `infra/config/secureagnt.yaml`

### Changed
- M9/M10 roadmap status now tracks deployment/provenance scaffolds and launchd template prep.
- Development/operations/testing/handoff docs updated with:
  - manifest generation/verification workflows
  - deployment preflight workflow
  - macOS launchd template references

### Tests
- Verified:
  - `make release-manifest RELEASE_MANIFEST_OUTPUT=/tmp/secureagnt-manifest.sha256`
  - `make release-manifest-verify RELEASE_MANIFEST_INPUT=/tmp/secureagnt-manifest.sha256`
  - `make deploy-preflight`

## v0.0.97 — Advance M8 with reusable validation-gate workflow

### Added
- New validation gate automation script:
  - `scripts/ops/validation_gate.sh`
  - executes runbook validation, workspace verify, security gate, and fixture-backed perf gate.
- New Makefile target:
  - `make validation-gate`
- Validation gate runtime controls:
  - `VALIDATION_GATE_RUN_DB_SUITES=1` (optional DB integration suite pass)
  - `VALIDATION_GATE_RUN_COVERAGE=1` (optional coverage gate pass)

### Changed
- `scripts/ops/release_gate.sh` now delegates core checks to `make validation-gate`, then optionally runs soak checks.
- Release-gate pass-through controls added:
  - `RELEASE_GATE_RUN_DB_SUITES`
  - `RELEASE_GATE_RUN_COVERAGE`
  - `RELEASE_GATE_RUN_DB_SECURITY`
- M8 docs/handoff updated for validation-gate usage and release-gate composition:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make validation-gate`
  - `make release-gate`

## v0.0.96 — Advance M6 with deterministic security-gate DB opt-in profile

### Added
- `scripts/ops/security_gate.sh` now supports deterministic DB test mode selection:
  - defaults to non-DB security checks when neither `RUN_DB_SECURITY` nor `RUN_DB_TESTS` enables DB mode
  - enables DB-backed worker security checks when `RUN_DB_SECURITY=1` or `RUN_DB_TESTS=1`

### Changed
- Security gate messaging now explicitly documents DB worker checks as opt-in for restricted/sandbox environments.
- M6/M8 docs and handoff updates now describe the security-gate profile and explicit DB opt-in commands in:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/TESTING.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make security-gate`

## v0.0.95 — Advance M8 with latency-trace regression capture and thresholds

### Added
- New tenant ops endpoint:
  - `GET /v1/ops/latency-traces`
  - returns rolling-window per-run latency samples (`duration_ms`) with role guardrails (`owner`/`operator` only).
- New core DB primitive:
  - `get_tenant_run_latency_traces(...)`
- `agntctl ops perf-gate` trace-regression support:
  - baseline/candidate trace fixtures (`--baseline-traces-json`, `--candidate-traces-json`)
  - API trace fetch path (`/v1/ops/latency-traces`) when candidate traces are not provided
  - new regression thresholds:
    - `--max-trace-p99-regression-ms`
    - `--max-trace-max-regression-ms`
    - `--max-trace-top5-avg-regression-ms`
  - trace sample size control: `--trace-limit`
- `agntctl ops capture-baseline` now captures latency traces:
  - writes `<prefix>_latency_traces.json`
  - supports `--traces-json` and `--trace-limit`
- New perf fixture inputs:
  - `agntctl/fixtures/ops_latency_traces_baseline.json`
  - `agntctl/fixtures/ops_latency_traces_candidate_ok.json`

### Changed
- Perf automation scripts now include trace regression/capture paths:
  - `scripts/ops/perf_gate.sh`
  - `scripts/ops/release_gate.sh`
  - `scripts/ops/capture_perf_baseline.sh`
- M8 docs updated for latency traces in:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/DEVELOPMENT.md`
  - `docs/RUNBOOK.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test -p agntctl`
  - `make test-db`
  - `make test-api-db`
  - `make release-gate`

## v0.0.94 — Advance M8/M8A runbook coverage for baseline capture and signing-key rotation

### Added
- `docs/RUNBOOK.md` now includes:
  - `Perf baseline capture` workflow (`make capture-perf-baseline` + `agntctl ops perf-gate`)
  - `Compliance replay signing-key rotation` workflow with version-pinned key cutover + rollback validation checks
- Runbook validation gate now enforces the new required sections:
  - `scripts/ops/validate_runbook.sh`

### Changed
- M8/M8A docs now explicitly track runbook coverage updates in:
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make runbook-validate`
  - `make release-gate`

## v0.0.93 — Advance M8 with staging perf baseline capture tooling

### Added
- New `agntctl` operator command:
  - `agntctl ops capture-baseline`
  - captures `/v1/ops/summary` + `/v1/ops/latency-histogram` payloads and writes baseline JSON files.
  - supports offline fixture inputs via `--summary-json` / `--histogram-json`.
- New automation script:
  - `scripts/ops/capture_perf_baseline.sh`
  - wraps `agntctl ops capture-baseline` for staging operators.
- New Makefile target:
  - `make capture-perf-baseline`
- New CLI integration coverage:
  - capture-baseline happy-path fixture capture/write path
  - capture-baseline required argument validation

### Changed
- M8 docs now include baseline capture workflow and controls in:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test -p agntctl`
  - `make release-gate`

## v0.0.92 — Advance M8 with consolidated release-gate workflow

### Added
- New release-gate automation script:
  - `scripts/ops/release_gate.sh`
  - executes:
    - `make runbook-validate`
    - `make verify`
    - fixture-backed `make perf-gate`
    - optional `make soak-gate` (`RELEASE_GATE_SKIP_SOAK=0`)
    - optional explicit DB suite re-run (`RELEASE_GATE_RUN_DB_SUITES=1`):
      - `make test-db`
      - `make test-api-db`
      - `make test-worker-db`
- New Makefile target:
  - `make release-gate`

### Changed
- M8 operations/development docs now describe release-gate as the default pre-release operator path.
- Roadmap and session handoff are updated to track the new release-gate entry point.
- CI now runs `RELEASE_GATE_SKIP_SOAK=0 make release-gate` (instead of separate verify/soak steps), so runbook validation + perf/soak regression checks are enforced through one operator entrypoint.

### Tests
- Verified:
  - `make runbook-validate`
  - `make release-gate`

## v0.0.91 — Advance M5C reconciliation metadata normalization

### Added
- New payment reconciliation normalization fields on `GET /v1/payments`:
  - `settlement_rail`
  - `normalized_outcome`
  - `normalized_error_code`
  - `normalized_error_class`
- New API normalization helpers:
  - stable outcome normalization from request/result status
  - stable error-class mapping from payment error-code families
- New API integration coverage:
  - payment ledger endpoint asserts normalized reconciliation fields

### Changed
- `GET /v1/payments` and replay package payment ledger rows now include normalized reconciliation metadata for downstream reporting/alerting workflows.
- M5C docs now include reconciliation normalization behavior in:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-api-db`
  - `make verify`

## v0.0.90 — Advance M8A with SIEM SLO metrics and compliance correlation enrichment

### Added
- New SIEM delivery SLO endpoint:
  - `GET /v1/audit/compliance/siem/deliveries/slo`
  - returns rolling-window counters and rate metrics:
    - `delivery_success_rate_pct`
    - `hard_failure_rate_pct`
    - `dead_letter_rate_pct`
- New core DB SIEM SLO query helper:
  - `get_tenant_compliance_siem_delivery_slo(...)`
- New compliance correlation enrichment fields in API responses:
  - `request_id`
  - `session_id`
  - `action_request_id`
  - `payment_request_id`
  - values are derived from routed compliance event payloads when present.
- New integration coverage:
  - core DB SIEM SLO rate/counter query behavior
  - API SIEM SLO endpoint role/rate behavior
  - compliance audit correlation field exposure in API responses

### Changed
- `list_tenant_compliance_audit_events(...)` now enriches returned records with correlation fields extracted from payload metadata.
- M8A docs now include SIEM SLO endpoint usage and correlation-field behavior:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`

## v0.0.89 — Advance M5C/M8A/M8 with Cashu live HTTP path, SIEM replay tooling, and perf regression gate

### Added
- New SIEM delivery operator endpoints:
  - `GET /v1/audit/compliance/siem/deliveries/targets`
  - `POST /v1/audit/compliance/siem/deliveries/{id}/replay`
- New core DB SIEM operator helpers:
  - `list_tenant_compliance_siem_delivery_target_summaries(...)`
  - `requeue_dead_letter_compliance_siem_delivery_record(...)`
- New worker SIEM hardening controls:
  - `WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_JITTER_MAX_MS`
  - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_HEADER`
  - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN`
  - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN_REF`
- New worker Cashu live transport controls:
  - `PAYMENT_CASHU_HTTP_ENABLED`
  - `PAYMENT_CASHU_HTTP_ALLOW_INSECURE`
  - `PAYMENT_CASHU_AUTH_HEADER`
  - `PAYMENT_CASHU_AUTH_TOKEN`
  - `PAYMENT_CASHU_AUTH_TOKEN_REF`
- New operator performance-gate tooling:
  - `agntctl ops perf-gate`
  - fixture inputs:
    - `agntctl/fixtures/ops_summary_candidate_ok.json`
    - `agntctl/fixtures/ops_latency_histogram_baseline.json`
    - `agntctl/fixtures/ops_latency_histogram_candidate_ok.json`
  - automation script:
    - `scripts/ops/perf_gate.sh`
  - Makefile target:
    - `make perf-gate`

### Changed
- Worker Cashu `payment.send` path now supports live HTTP settlement execution when enabled, with HTTPS-by-default validation and fail-closed behavior when both mock/live modes are disabled.
- Worker SIEM outbox retry now applies deterministic jitter to backoff scheduling and supports optional per-target auth headers for HTTP delivery.
- SIEM delivery target summaries now preserve latest non-null error and latest attempt timestamps for operator triage.
- `agntctl` help surface now includes `ops perf-gate` and regression threshold flags.
- Development/operations/API/payments/roadmap/session handoff docs are updated for:
  - Cashu live HTTP knobs and execution mode
  - SIEM targets/replay endpoints and auth/jitter controls
  - perf-gate usage and automation

### Tests
- Verified:
  - `cargo test -p agntctl`
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.88 — Advance M5C/M8A/M8 with Cashu mock execution, SIEM summary, and ops latency histogram

### Added
- New Cashu provider migration:
  - `migrations/0016_payment_provider_cashu.sql`
  - expands `payment_requests.provider` check constraint to allow `cashu`
- New worker Cashu mock execution controls:
  - `PAYMENT_CASHU_MOCK_ENABLED`
  - `PAYMENT_CASHU_MOCK_BALANCE_MSAT`
- New API observability endpoints:
  - `GET /v1/ops/latency-histogram`
  - `GET /v1/audit/compliance/siem/deliveries/summary`
- New core DB query helpers:
  - `get_tenant_run_latency_histogram(...)`
  - `get_tenant_compliance_siem_delivery_summary(...)`
- New build reliability hooks:
  - `api/build.rs`
  - `core/build.rs`
  - `worker/build.rs`
  - these force recompilation when `migrations/` changes so `sqlx::migrate!` tests stay in sync
- New integration coverage:
  - worker Cashu mock payment execution (`payment.send` `cashu:*`)
  - API ops latency histogram role/bucket behavior
  - API SIEM delivery summary role/tenant behavior
  - core DB latency histogram and SIEM summary queries

### Changed
- Worker `payment.send` Cashu path now records executed ledger outcomes and payment outbox artifacts when mock mode is enabled.
- Cashu path remains fail-closed by default when mock mode is disabled.
- API/docs/runbooks now include:
  - SIEM delivery summary endpoint
  - ops latency histogram endpoint
  - Cashu mock runtime controls and behavior

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.87 — Complete M6A handoff packet APIs and enforce build-then-test verification gate

### Added
- New inter-agent handoff packet API endpoints:
  - `POST /v1/memory/handoff-packets`
  - `GET /v1/memory/handoff-packets`
- New core memory query helper:
  - `list_tenant_handoff_memory_records(...)`
- New API integration coverage:
  - handoff packet create/list behavior
  - `to_agent_id` / `from_agent_id` filter behavior
  - tenant and role guardrails on handoff endpoints
- New core DB integration coverage:
  - handoff memory listing filter behavior (`to_agent_id`, `from_agent_id`)
- New Makefile verification targets:
  - `make build`
  - `make verify` (build, then tests)
  - `make verify-db` (build + DB integration suites)

### Changed
- CI now runs `make verify` so tests are always executed after a workspace build.
- Handoff packet persistence uses structured `memory_kind=handoff` records in `memory_records`.
- Worker relay socket unit tests now gracefully skip only when local TCP bind is denied by the host sandbox/OS policy.
- Worker integration test config helper now tracks the full `WorkerConfig` field set, including Cashu/memory-compaction/SIEM outbox controls.
- Development/operations/API/roadmap/session-handoff docs updated for:
  - handoff packet endpoints and filters
  - new verify/verify-db build-test workflow
  - current M6A status/next-step focus

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`
  - `make verify`

## v0.0.86 — Advance M6A/M8A/M5C with memory redaction, SIEM observability, and Cashu scaffold controls

### Added
- New SIEM delivery observability endpoint:
  - `GET /v1/audit/compliance/siem/deliveries` (`owner`/`operator`)
  - supports tenant-scoped filters: `run_id`, `status`, `limit`
- New core DB query primitive:
  - `list_tenant_compliance_siem_delivery_records(...)`
- New redaction helper:
  - `redact_memory_content(...)` in `core/src/redaction.rs`
- New integration coverage:
  - API SIEM delivery list endpoint role/tenant/status guardrails
  - API memory auto-redaction behavior on create/list flow
  - API `payments_cashu_v1` recipe capability grant path
  - core DB SIEM delivery listing path and status filtering
- New payment recipe bundle:
  - `payments_cashu_v1` (`payment.send` with `cashu:*` scope)
- New worker Cashu scaffold controls:
  - `PAYMENT_CASHU_ENABLED`
  - `PAYMENT_CASHU_MINT_URIS` / `PAYMENT_CASHU_MINT_URIS_REF`
  - `PAYMENT_CASHU_DEFAULT_MINT`
  - `PAYMENT_CASHU_TIMEOUT_MS`
  - `PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN`

### Changed
- API `payment.send` scope normalization now accepts both:
  - `nwc:*`
  - `cashu:*`
- API memory writes now apply redaction before persistence/indexing and set `redaction_applied` automatically when changes occur.
- Worker payment destination parser now recognizes `cashu:<mint_id>`.
- Cashu runtime remains fail-closed with deterministic `payment_results` failure records until full settlement transport is implemented.
- SIEM delivery observability docs expanded in:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`
  - `cargo test -p core --lib`
  - `cargo test -p worker --lib`

## v0.0.85 — Advance M8A with replay manifest signing and SIEM delivery outbox scaffold

### Added
- New compliance SIEM delivery outbox migration:
  - `migrations/0015_compliance_siem_delivery_outbox.sql`
  - table: `compliance_siem_delivery_outbox`
  - status lifecycle: `pending -> processing -> delivered|failed|dead_lettered`
- New core SIEM outbox DB APIs:
  - `create_compliance_siem_delivery_record(...)`
  - `claim_pending_compliance_siem_delivery_records(...)`
  - `mark_compliance_siem_delivery_record_delivered(...)`
  - `mark_compliance_siem_delivery_record_failed(...)`
- New API endpoint:
  - `POST /v1/audit/compliance/siem/deliveries` (owner/operator)
  - queues adapter-formatted SIEM payloads into delivery outbox
- Replay package manifest baseline:
  - `GET /v1/audit/compliance/replay-package` now includes `manifest`
  - manifest fields:
    - `version`
    - `digest_sha256`
    - `signing_mode` (`unsigned` or `hmac-sha256`)
    - `signature` (when signing key is configured)
- New worker SIEM outbox scaffold:
  - claim/process outbox rows per cycle
  - mock targets for local validation:
    - `mock://success`
    - `mock://fail`
  - optional HTTP delivery path (fail-closed unless enabled)
  - controls:
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED`
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_BATCH_SIZE`
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_LEASE_MS`
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_BACKOFF_MS`
    - `WORKER_COMPLIANCE_SIEM_HTTP_ENABLED`
    - `WORKER_COMPLIANCE_SIEM_HTTP_TIMEOUT_MS`

### Changed
- SIEM adapter payload serialization is now shared between export and queued-delivery flows.
- Replay package generation now computes deterministic manifest digest and optional HMAC signature key resolution via:
  - `COMPLIANCE_REPLAY_SIGNING_KEY`
  - `COMPLIANCE_REPLAY_SIGNING_KEY_REF`

### Tests
- Added coverage for:
  - SIEM outbox claim/deliver and dead-letter transitions (`core` DB integration)
  - SIEM delivery queue API role/failure guardrails (`api` integration)
  - replay package manifest payload fields (`api` integration)
- Verified:
  - `make test-db`
  - `make test-api-db`
  - `cargo test -p worker --lib`

## v0.0.84 — Advance M6A with worker memory compaction and stats visibility

### Added
- New memory compaction migration:
  - `migrations/0014_memory_compaction_controls.sql`
  - `memory_records.compacted_at` marker column
  - tenant compaction index for active/pending scans
- New core compaction APIs:
  - `compact_memory_records(...)`
  - `get_tenant_memory_compaction_stats(...)`
- New worker background compaction pass:
  - runs each worker cycle before run claim
  - emits run-linked `memory.compacted` audit events
  - controlled by:
    - `WORKER_MEMORY_COMPACTION_ENABLED`
    - `WORKER_MEMORY_COMPACTION_MIN_RECORDS`
    - `WORKER_MEMORY_COMPACTION_MAX_GROUPS_PER_CYCLE`
    - `WORKER_MEMORY_COMPACTION_MIN_AGE_SECS`
- New API endpoint:
  - `GET /v1/memory/compactions/stats` (`owner`/`operator`)
- New integration coverage:
  - compaction group-limit and under-load DB behavior
  - compaction stats API role guardrails

### Changed
- Memory listing/retrieval now read only active rows (`compacted_at IS NULL`).
- `POST /v1/memory/records/purge-expired` now appends run-linked `memory.purged` audit events.
- M6A docs expanded in:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`
  - `cargo test -p worker --lib`

## v0.0.83 — Advance M6A with memory retrieval API and citation metadata

### Added
- New memory retrieval endpoint:
  - `GET /v1/memory/retrieve`
  - deterministic ranked retrieval results with citation metadata:
    - `memory_id`
    - `created_at`
    - `source`
    - `memory_kind`
    - `scope`
- New API integration coverage:
  - retrieval ranking + citation payload validation
  - retrieval scope guardrails (`memory:` prefix)
  - retrieval role and tenant isolation checks

### Changed
- M6A docs expanded in:
  - `docs/API.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-api-db`

## v0.0.82 — Start M6A with durable memory schema and API baseline

### Added
- New memory-plane migration:
  - `migrations/0013_memory_plane.sql`
  - tables:
    - `memory_records`
    - `memory_compactions`
  - retention function:
    - `purge_expired_memory_records(tenant_id, as_of)`
- New core DB APIs:
  - `create_memory_record(...)`
  - `list_tenant_memory_records(...)`
  - `create_memory_compaction_record(...)`
  - `purge_expired_tenant_memory_records(...)`
- New API endpoints:
  - `POST /v1/memory/records`
  - `GET /v1/memory/records`
  - `POST /v1/memory/records/purge-expired` (owner only)
- New policy capabilities:
  - `memory.read`
  - `memory.write`
- New recipe capability bundle:
  - `memory_v1`
- New integration coverage:
  - API memory create/list/purge behavior and role guardrails
  - core memory persistence, purge, compaction, and tenant scoping

### Changed
- Worker capability normalization now recognizes memory capability kinds for policy parsing.
- M6A docs expanded in:
  - `docs/API.md`
  - `docs/ARCHITECTURE.md`
  - `docs/OPERATIONS.md`
  - `docs/POLICY.md`
  - `docs/ROADMAP.md`
  - `docs/SCHEMA.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`
  - `cargo test -p worker --lib`

## v0.0.81 — Advance M8A with SIEM export adapters and incident replay package API

### Added
- New compliance SIEM export endpoint:
  - `GET /v1/audit/compliance/siem/export`
  - supports adapter-formatted output:
    - `secureagnt_ndjson`
    - `splunk_hec`
    - `elastic_bulk`
- New deterministic replay package endpoint:
  - `GET /v1/audit/compliance/replay-package`
  - returns tenant-scoped run status, run audit events, compliance events, optional payment ledger, and correlation summary
- New API integration coverage:
  - SIEM adapter export format validation (`splunk_hec`, `elastic_bulk`)
  - replay package correlation payload validation
  - viewer-role denial + tenant isolation checks for new M8A endpoints

### Changed
- Compliance export now uses shared serializer helpers for stable event field mapping.
- M8A docs expanded in:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-api-db`

## v0.0.80 — Advance M8 with soak/perf gate automation and runbook validation checks

### Added
- New operator threshold-gate command:
  - `agntctl ops soak-gate`
  - evaluates `/v1/ops/summary` against configurable thresholds
  - supports API-backed checks and fixture/file-backed regression checks (`--summary-json`)
- New soak automation script:
  - `scripts/ops/soak_gate.sh`
  - repeated threshold checks for staging soak windows
- New runbook validation script:
  - `scripts/ops/validate_runbook.sh`
  - enforces required incident/backup/rollback/soak sections
- New fixture:
  - `agntctl/fixtures/ops_summary_ok.json`

### Changed
- CI now includes:
  - `make runbook-validate`
  - fixture-backed `agntctl ops soak-gate` regression gate
- New Make targets:
  - `make soak-gate`
  - `make runbook-validate`
- M8 docs expanded in:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/RUNBOOK.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test -p agntctl`
  - `make test-db`
  - `make test-api-db`

## v0.0.79 — Start M8 with tenant ops summary endpoint and runbook baseline

### Added
- New tenant operations summary API endpoint:
  - `GET /v1/ops/summary` (owner/operator only)
  - rolling-window counters for:
    - `queued_runs`
    - `running_runs`
    - `succeeded_runs_window`
    - `failed_runs_window`
    - `dead_letter_trigger_events_window`
  - rolling-window run duration telemetry:
    - `avg_run_duration_ms`
    - `p95_run_duration_ms`
- New core DB helper and model:
  - `get_tenant_ops_summary(...)`
  - `TenantOpsSummaryRecord`
- New API integration coverage:
  - validates ops summary counters and role guardrail enforcement (`viewer` denied)

### Changed
- M8 docs baseline expanded in:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/RUNBOOK.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`

## v0.0.78 — Advance M7 with tenant trigger-capacity guardrail and index tuning

### Added
- New API tenant trigger-capacity guardrail:
  - `API_TENANT_MAX_TRIGGERS`
  - trigger creation endpoints return `429 TENANT_TRIGGER_LIMITED` when tenant trigger capacity is exhausted
- New core DB helper:
  - `count_tenant_triggers(...)`
- New tenant index tuning migration:
  - `migrations/0012_tenant_isolation_indexes.sql`
  - indexes for tenant-scoped run, trigger-event, and payment-ledger query paths
- New integration coverage:
  - cross-tenant trigger mutation isolation (`PATCH`, `disable`, `fire` -> `404`)
  - trigger-capacity limit enforcement on create path

### Changed
- API builder now supports explicit guardrail composition in tests:
  - `app_router_with_limits(pool, tenant_max_inflight_runs, tenant_max_triggers)`
- M7 docs updated in:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`

## v0.0.77 — Advance M8A with compliance retention/legal-hold controls

### Added
- New compliance policy migration and purge function:
  - `migrations/0011_compliance_retention_legal_hold.sql`
  - table `compliance_audit_policies`
  - function `purge_expired_compliance_audit_events(tenant_id, as_of)`
- New core DB compliance policy APIs:
  - `get_tenant_compliance_audit_policy(...)`
  - `upsert_tenant_compliance_audit_policy(...)`
  - `purge_expired_tenant_compliance_audit_events(...)`
- New API endpoints:
  - `GET /v1/audit/compliance/policy` (owner/operator)
  - `PUT /v1/audit/compliance/policy` (owner only)
  - `POST /v1/audit/compliance/purge` (owner only)

### Changed
- M8A docs expanded for retention/legal-hold operations:
  - `docs/API.md`
  - `docs/SCHEMA.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`

## v0.0.76 — Advance M5C with Cashu planning scaffold and ADR

### Added
- New payment rail planning documentation:
  - `docs/PAYMENTS.md`
  - defines current NWC runtime baseline and phased Cashu implementation targets
- New ADR for Cashu rail planning:
  - `docs/ADR/ADR-0008-cashu-rail-planning.md`

### Changed
- Payment docs now consistently call out:
  - NWC-only runtime enforcement today
  - Cashu as optional future rail (planning scaffold only)
- Updated references and handoff context:
  - `docs/README.md`
  - `docs/API.md`
  - `docs/POLICY.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.75 — Advance M7 with cross-tenant isolation integration tests

### Added
- New API integration isolation coverage:
  - `run_and_audit_endpoints_are_tenant_isolated`
    - cross-tenant `GET /v1/runs/{id}` returns `404`
    - cross-tenant `GET /v1/runs/{id}/audit` returns `404`
  - `compliance_endpoints_are_tenant_isolated`
    - cross-tenant `GET /v1/audit/compliance` returns empty result set
    - cross-tenant `GET /v1/audit/compliance/export` returns empty NDJSON body
    - cross-tenant `GET /v1/audit/compliance/verify` reports `checked_events=0`

### Changed
- M7 roadmap/handoff docs now explicitly track expanded isolation-test coverage:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-api-db`

## v0.0.74 — Advance M8A with compliance tamper-evidence hash chain and verification API

### Added
- New compliance tamper-evidence migration:
  - `migrations/0010_compliance_tamper_chain.sql`
  - adds per-tenant chain fields on `compliance_audit_events`:
    - `tamper_chain_seq`
    - `tamper_prev_hash`
    - `tamper_hash`
  - adds chain verification SQL function:
    - `verify_compliance_audit_chain(tenant_id)`
- New core DB verification API:
  - `verify_tenant_compliance_audit_chain(...)`
- New API verification endpoint:
  - `GET /v1/audit/compliance/verify`
  - tenant-scoped, owner/operator-only
- Compliance event responses and NDJSON exports now include tamper-evidence fields.

### Changed
- Compliance event query order now follows deterministic chain order (`tamper_chain_seq`).
- M8A docs expanded with tamper-evidence schema/ops/API details:
  - `docs/API.md`
  - `docs/SCHEMA.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`

## v0.0.73 — Complete M0N by removing legacy AEGIS runtime compatibility

### Changed
- Removed legacy `AEGIS_*` runtime compatibility paths:
  - worker/skill runtime no longer emits `AEGIS_SKILL_SANDBOXED`
  - worker config no longer supports `WORKER_SKILL_EMIT_LEGACY_AEGIS_MARKER`
  - secret resolver now reads only `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`
- Updated migration docs/handoff to the finalized SecureAgnt env baseline:
  - `docs/DEVELOPMENT.md`
  - `docs/NAMING.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test -p core`
  - `cargo test -p skillrunner`
  - `cargo test -p worker --lib`

## v0.0.72 — Advance M8A with compliance NDJSON export endpoint

### Added
- New compliance audit export endpoint:
  - `GET /v1/audit/compliance/export`
  - tenant-scoped, owner/operator-only
  - NDJSON output (`application/x-ndjson`) for batch export/SIEM ingestion workflows
- New API integration coverage:
  - validates NDJSON export response and schema fields
  - validates viewer-role denial on compliance export path

### Changed
- Compliance docs expanded with export workflow details:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.71 — Advance M7 with tenant in-flight run capacity guardrail

### Added
- New core DB helper:
  - `count_tenant_inflight_runs(...)` for tenant queued+running run counts
- New API runtime guardrail:
  - `API_TENANT_MAX_INFLIGHT_RUNS` (optional positive integer)
  - when configured, `POST /v1/runs` returns `429` with `TENANT_INFLIGHT_LIMITED` at/above tenant inflight capacity
- New API builder for deterministic config in tests:
  - `app_router_with_tenant_limit(...)`
- New API integration coverage:
  - verifies tenant inflight cap denial behavior on run creation

### Changed
- Multi-tenant capacity docs updated:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.70 — Advance M6 with message destination allowlist hardening

### Added
- New worker security controls for `message.send` destination hardening:
  - `WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST`
  - `WORKER_MESSAGE_SLACK_DEST_ALLOWLIST`
- New fail-closed destination enforcement in worker action execution:
  - non-allowlisted destination targets now fail `message.send` when an allowlist is configured
- New worker integration coverage:
  - White Noise destination allowlist denial
  - Slack destination allowlist denial

### Changed
- Worker startup logs now include configured allowlist counts for White Noise and Slack destinations.
- Security/operations/dev/handoff docs updated with new destination allowlist controls:
  - `docs/SECURITY.md`
  - `docs/OPERATIONS.md`
  - `docs/DEVELOPMENT.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.69 — Advance M5C with payment summary reporting endpoint

### Added
- New tenant payment summary API endpoint:
  - `GET /v1/payments/summary`
  - supports optional filters: `window_secs`, `agent_id`, `operation`
- New core DB summary query:
  - `get_tenant_payment_summary(...)`
  - returns request status counters and executed spend totals
- New API integration coverage:
  - validates payment summary counters/spend output
  - validates operation filter behavior
  - validates invalid operation rejection (`400`)

### Changed
- Payment reconciliation docs now include summary reporting:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.68 — Advance M0N with SecureAgnt systemd packaging templates

### Added
- New SecureAgnt-first systemd unit templates:
  - `infra/systemd/secureagnt.service` (worker daemon)
  - `infra/systemd/secureagnt-api.service` (API daemon)
- Hardened baseline unit settings included:
  - non-root runtime user/group (`secureagnt`)
  - `NoNewPrivileges=true`
  - `ProtectSystem=strict`
  - explicit writable paths for `/var/lib/secureagnt` and `/var/log/secureagnt`

### Changed
- Naming/operations docs now reference systemd templates as part of packaging migration:
  - `docs/NAMING.md`
  - `docs/OPERATIONS.md`
  - `docs/DEVELOPMENT.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.67 — Start M8A with DB-routed compliance audit plane and query API

### Added
- New compliance audit-plane persistence layer:
  - migration `migrations/0009_compliance_audit_plane.sql`
  - table `compliance_audit_events`
  - trigger-based routing from `audit_events` into compliance plane
- Baseline compliance routing classes:
  - `action.denied`
  - `action.failed`
  - `action.requested|action.allowed|action.executed` where `payload_json.action_type` is `payment.send` or `message.send`
  - `run.failed`
- New core query API:
  - `list_tenant_compliance_audit_events(...)`
- New API endpoint:
  - `GET /v1/audit/compliance` (tenant-scoped, owner/operator only, optional `run_id`, `event_type`, `limit`)
- New integration coverage:
  - `core/tests/db_integration.rs`: compliance routing assertion
  - `api/tests/api_integration.rs`: compliance endpoint retrieval and viewer-role denial

### Changed
- Migration expected table set now includes `compliance_audit_events`.
- Docs updated for M8A baseline implementation:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/SCHEMA.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.66 — Advance M5C with tenant payment reconciliation/reporting API

### Added
- New tenant payment ledger query endpoint:
  - `GET /v1/payments`
  - filters: `run_id`, `agent_id`, `status`, `destination`, `idempotency_key`, `limit`
- New core DB query path:
  - `list_tenant_payment_ledger(...)` for tenant-scoped payment requests with latest result join
- New API integration coverage:
  - verifies tenant-scoped payment ledger response includes latest settlement result
  - verifies viewer-role denial (`403`) for payment ledger queries

### Changed
- `core` now exports `PaymentLedgerRecord` and `list_tenant_payment_ledger`.
- API docs and operations/handoff/roadmap notes now include payment reporting baseline:
  - `docs/API.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-api-db`
  - `make test-db`
  - `make test-worker-db`

## v0.0.65 — Advance M0N with configurable legacy skill marker emission

### Added
- New worker runtime control for naming-migration compatibility:
  - `WORKER_SKILL_EMIT_LEGACY_AEGIS_MARKER` (`1` default)
  - set to `0` to stop emitting `AEGIS_SKILL_SANDBOXED` to skill subprocesses
- New `skillrunner` integration coverage:
  - verifies legacy marker can be disabled while preserving `SECUREAGNT_SKILL_SANDBOXED=1`

### Changed
- `skillrunner::RunnerConfig` now exposes explicit legacy marker emission behavior:
  - `emit_legacy_aegis_skill_sandbox_marker`
- Worker startup logs now include `skill_emit_legacy_aegis_marker`.
- Naming/operations/dev docs now include migration control details:
  - `docs/NAMING.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test -p skillrunner runner_integration`
  - `make test-worker-db`

## v0.0.64 — Advance M5C with payment route health quarantine and rollout controls

### Added
- New payment route rollout control:
  - `PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT` (`0..100`, default `100`)
  - enables deterministic canary rollout of multi-route behavior by wallet/idempotency bucket
- New payment route health controls:
  - `PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD` (default `3`)
  - `PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS` (default `60`)
  - failing routes are temporarily quarantined and skipped during cooldown
- Expanded route metadata in `payment.send` results:
  - rollout posture (`rollout_percent`, `rollout_limited`)
  - health posture (`skipped_unhealthy_count`, `health_fail_threshold`, `health_cooldown_secs`)
  - attempt counts (`attempted_count`)
- New worker integration tests:
  - skip unhealthy route on subsequent run after health threshold is reached
  - `rollout_percent=0` forces primary-route-only behavior even with fallback enabled

### Changed
- Worker startup logs now include payment rollout and route-health controls.
- `spawn_mock_nwc_wallet_relay` test helper now supports multiple relay connections to validate multi-run route behavior.
- Payment roadmap/handoff/dev-ops docs updated for rollout and route-health controls:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`

### Tests
- Verified:
  - `make test-worker-db`
  - `make test-api-db`
  - `make test-db`

## v0.0.63 — Complete M6B provider-adapter integration coverage

### Added
- New `core` secrets integration-style tests with mocked CLI backends:
  - Vault adapter test for version-pin flag + field selection behavior.
  - AWS adapter test for provider error propagation.
  - Azure adapter test for version-pin command argument path.
  - Cached CLI resolver rollover test validating new secret value pickup after TTL expiry.

### Changed
- M6B roadmap and handoff status advanced to completed expanded baseline with provider-adapter coverage documented.
- Session handoff next-steps list updated to remove remaining M6B work item.

### Tests
- Verified:
  - `cargo test -p core secrets`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.62 — Complete M6C with remote LLM soft-alert thresholds and audit emission

### Added
- New remote LLM soft-alert config:
  - `LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT` (`1..100`, optional)
- `llm.infer` now produces soft-alert metadata in token accounting when configured thresholds are reached.
- Worker emits dedicated audit events for near-budget conditions:
  - `event_type = "llm.budget.soft_alert"`
- New worker integration coverage:
  - verifies soft-alert audit emission on successful remote `llm.infer` execution near budget threshold.

### Changed
- Worker startup logs now include `llm_remote_token_budget_soft_alert_threshold_pct`.
- M6C roadmap/handoff/docs updated to mark soft-alert coverage as implemented:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`

### Tests
- Verified:
  - `make test-worker-db`
  - `cargo test -p worker llm::tests`

## v0.0.61 — Expand M6B with secret cache + version pin support

### Added
- New shared secret resolver cache wrapper in `core/src/secrets.rs`:
  - `CachedSecretResolver<R>`
  - env controls:
    - `SECUREAGNT_SECRET_CACHE_TTL_SECS` (default `30`, `0` disables cache)
    - `SECUREAGNT_SECRET_CACHE_MAX_ENTRIES` (default `1024`)
- Cloud secret reference query-parameter support for version pinning:
  - Vault: `?version=`
  - AWS Secrets Manager: `?version_id=` or `?version_stage=`
  - GCP Secret Manager: `?version=`
  - Azure Key Vault: `?version=`
- New documentation: `docs/SECRETS.md` (provider auth strategy, cache, version pins, rotation guidance).

### Changed
- API and worker secret-consuming paths now use cached shared resolvers:
  - `api/src/lib.rs`
  - `worker/src/lib.rs`
  - `worker/src/llm.rs`
- Core secret parsing now supports backend query params with validation for conflicting AWS version selectors.
- Session handoff/read-order updated to include secrets operations guidance.

### Tests
- Added core resolver tests for:
  - query-param parsing and GCP query-version support
  - cache hit behavior before TTL expiry
  - cache refresh behavior after TTL expiry (rotation path)
- Verified:
  - `cargo test -p core secrets`
  - `make test-worker-db`
  - `make test-api-db`

## v0.0.60 — Expand M6C with DB-backed LLM token governance and usage API

### Added
- New migration `migrations/0008_llm_token_usage.sql`:
  - adds `llm_token_usage` ledger table for remote LLM token/cost accounting
  - adds tenant/agent/model time-window indexes for budget enforcement and usage queries
- New core DB APIs for remote LLM usage accounting:
  - `create_llm_token_usage_record(...)`
  - `sum_llm_consumed_tokens_for_tenant_since(...)`
  - `sum_llm_consumed_tokens_for_agent_since(...)`
  - `sum_llm_consumed_tokens_for_model_since(...)`
  - `get_llm_usage_totals_since(...)`
- New API endpoint:
  - `GET /v1/usage/llm/tokens`
  - supports `window_secs`, optional `agent_id`, optional `model_key`
  - role guard: `viewer` denied, `owner`/`operator` allowed
- New runtime controls for remote LLM budget windows:
  - `LLM_REMOTE_TOKEN_BUDGET_PER_TENANT`
  - `LLM_REMOTE_TOKEN_BUDGET_PER_AGENT`
  - `LLM_REMOTE_TOKEN_BUDGET_PER_MODEL`
  - `LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS`

### Changed
- Worker `llm.infer` now enforces fail-closed remote token budgets at multiple levels:
  - per-run (existing)
  - per-tenant (new)
  - per-agent (new)
  - per-model (new)
- Remote `llm.infer` executions now persist token usage to `llm_token_usage` for deterministic budget accounting.
- `llm.infer` action result `token_accounting` now includes:
  - budget window metadata
  - remaining tenant/agent/model budget values when configured
- Worker startup logs now include remote tenant/agent/model budget settings and budget window size.
- Updated docs for new budget knobs and usage-query behavior:
  - `docs/API.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `make test-db`
  - `make test-worker-db`
  - `make test-api-db`

## v0.0.59 — Harden M5C wallet routing with deterministic strategy and failover controls

### Added
- New payment routing controls in worker config:
  - `PAYMENT_NWC_ROUTE_STRATEGY` (`ordered` default, `deterministic_hash`)
  - `PAYMENT_NWC_ROUTE_FALLBACK_ENABLED` (`1` default)
- Multi-route wallet entries for `PAYMENT_NWC_WALLET_URIS` values using `|` separators (`wallet=uri_a|uri_b`).
- Route-attempt metadata in `payment.send` NWC execution results (`result.nwc.route`).
- New worker integration tests:
  - failover succeeds when first wallet route fails and fallback is enabled
  - fail-fast behavior when fallback is disabled

### Changed
- `payment.send` NWC execution now selects route candidates by strategy and attempts fallback routes when enabled.
- Worker startup logs now include payment route strategy and fallback posture fields.
- Updated M5C docs/handoff for route orchestration controls:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.58 — Extend M5C with multi-wallet NWC routing and fail-closed wallet targeting

### Added
- Worker multi-wallet NWC route configuration:
  - `PAYMENT_NWC_WALLET_URIS`
  - `PAYMENT_NWC_WALLET_URIS_REF`
  - accepts `wallet_id=nwc_uri` entries (comma/newline) or JSON object form
  - supports optional wildcard default route (`*`)
- Worker runtime wallet route resolver with precedence:
  - exact `wallet_id` route
  - wildcard (`*`) route
  - legacy single default (`PAYMENT_NWC_URI` / `PAYMENT_NWC_URI_REF`)
- New worker integration coverage:
  - wallet-map route overrides single default URI
  - fail-closed behavior when map mode is configured but destination wallet id is missing

### Changed
- `payment.send` execution path now fails closed with `PAYMENT_WALLET_NOT_CONFIGURED` when wallet-map mode is configured and target wallet id is unresolved.
- Worker startup logs now include wallet-route posture metadata:
  - configured wallet route count
  - wildcard/default route configured-state
- M5C docs and handoff updated for wallet-id routing controls and operational behavior:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.57 — Add M4B dead-letter webhook event replay path

### Added
- New trigger-event replay DB primitive in `core/src/db.rs`:
  - `requeue_dead_letter_trigger_event(...)`
  - `TriggerEventReplayOutcome` (`Requeued`, `NotFound`, `NotDeadLettered`)
- New API endpoint:
  - `POST /v1/triggers/{id}/events/{event_id}/replay`
  - owner/operator only, webhook-only, dead-letter status required
  - returns `202` with `status=queued_for_replay` when replay is accepted
- New test coverage:
  - `core/tests/db_integration.rs` replay reset behavior + state transitions
  - `api/tests/api_integration.rs` replay endpoint coverage (`conflict -> requeue -> conflict`)

### Changed
- Core exports now include replay primitives via `core/src/lib.rs`.
- API trigger docs now document replay semantics and mutation auth scope (`docs/API.md`).
- Roadmap/session handoff updated to reflect replay capability in M4B:
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.56 — Advance M0N env alias migration with dated compatibility window

### Added
- Explicit alias retirement window in naming and ops docs:
  - `AEGIS_SECRET_ENABLE_CLOUD_CLI` accepted through `2026-06-30`
  - planned removal date `2026-07-01`
- New core test coverage for legacy env-gate fallback:
  - `legacy_cloud_gate_env_is_respected_when_secure_unset`

### Changed
- `core/src/secrets.rs` now uses explicit SecureAgnt-first env precedence for cloud secret CLI gate resolution:
  - primary: `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`
  - fallback: `AEGIS_SECRET_ENABLE_CLOUD_CLI`
- Runtime warning is now emitted when the legacy `AEGIS_SECRET_ENABLE_CLOUD_CLI` alias is used without the SecureAgnt primary variable.
- Updated migration/deprecation references in:
  - `docs/NAMING.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.55 — Define two-plane enterprise audit model in roadmap/ops docs

### Added
- M8A now explicitly defines two audit planes in `docs/ROADMAP.md`:
  - `Operational Audit` (high-volume support/troubleshooting)
  - `Compliance Audit` (high-trust governance/forensics)
- Event class baselines added for both planes (lifecycle, policy, approvals, payments, external side effects, control-plane mutations).
- Retention default baselines added:
  - operational: `30` days hot + `180` days archive
  - compliance: `180` days hot + `2555` days (`7` years) archive
  - legal-hold override prevents purge

### Changed
- `docs/OPERATIONS.md` now documents the two-plane audit operating model and retention targets.
- `docs/SESSION_HANDOFF.md` now captures the two-plane audit plan and retention defaults for fast session continuity.

## v0.0.54 — Add explicit enterprise audit/compliance milestone planning

### Added
- New roadmap milestone `M8A — Enterprise Audit and Compliance Plane` in `docs/ROADMAP.md`:
  - immutable/WORM-capable audit export planning
  - tamper-evidence planning (hash/signature verification path)
  - SIEM export adapter planning
  - retention and legal-hold control planning
- Session handoff now tracks M8A and calls it out in high-priority next steps (`docs/SESSION_HANDOFF.md`).

### Changed
- Roadmap sequencing now makes enterprise audit/compliance a first-class deliverable before post-MVP governance packaging.

## v0.0.53 — Start SecureAgnt naming migration with `agntctl` CLI scaffold

### Added
- New CLI crate `agntctl` with initial command surface scaffolding:
  - `status`
  - `config validate`
  - `skills list|info|install`
  - `policy allow|deny`
  - `audit tail`
- New naming spec doc: `docs/NAMING.md`.
- New roadmap milestone `M0N — Naming and Packaging Migration (SecureAgnt)`.
- New core secret resolver coverage for SecureAgnt cloud-gate env var:
  - `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`.

### Changed
- Brand/docs migration from `Aegis` to `SecureAgnt` across primary operational docs:
  - `docs/SESSION_HANDOFF.md`
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/ARCHITECTURE.md`
  - `docs/README.md`
  - `docs/RUNBOOK.md`
  - `docs/SCHEMA.md`
- Worker/API binary aliasing:
  - `secureagntd` alias for worker runtime
  - `secureagnt-api` alias for API runtime
  - legacy `worker`/`api` binaries kept for compatibility
- Skillrunner sandbox marker now sets both:
  - `SECUREAGNT_SKILL_SANDBOXED=1` (primary)
  - `AEGIS_SKILL_SANDBOXED=1` (legacy compatibility)
- Secret CLI gate now uses SecureAgnt-first env var with compatibility fallback:
  - primary: `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI`
  - fallback: `AEGIS_SECRET_ENABLE_CLOUD_CLI`
- Make targets expanded:
  - `make agntctl`
  - `make secureagntd`
  - `make secureagnt-api`
- Updated test fixture naming prefixes from `aegis_*` to `secureagnt_*` in DB/integration tests.

### Tests
- Verified:
  - `cargo test`
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.52 — Add live NIP-47 payment relay path with fail-closed ledgering

### Added
- New NIP-47 wallet transport module in `worker/src/nip47_wallet.rs`:
  - encrypted NWC request/response over relay websockets
  - per-request timeout and multi-relay attempt behavior
  - relay/request/response event correlation metadata
- Worker payment config knobs:
  - `PAYMENT_NWC_URI` / `PAYMENT_NWC_URI_REF` (live NIP-47 wallet URI)
  - `PAYMENT_NWC_TIMEOUT_MS` (NIP-47 timeout budget)
- New tests:
  - `worker/src/nip47_wallet.rs` relay round-trip and wallet-error surfacing tests
  - `worker/tests/worker_integration.rs` live NIP-47 `payment.send` execution test

### Changed
- `payment.send` execution in `worker/src/lib.rs` now:
  - uses live NIP-47 flow when `PAYMENT_NWC_URI` is configured
  - keeps mock fallback (`nwc_mock`) when no NWC URI is configured
  - fails closed with persisted `payment_results`/`payment_requests.status='failed'` for NIP-47 transport or wallet-response errors
  - rejects inline `nostr+walletconnect://...` destinations to avoid credential leakage in run payloads/artifacts
- Worker startup logs now include NWC URI configured-state and timeout fields.
- Enabled `nostr` crate `nip47` feature in workspace dependencies.
- Updated docs:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/ROADMAP.md`
  - `docs/SESSION_HANDOFF.md`

## v0.0.51 — Add M5C tenant/agent spend caps and roadmap token-budget milestone

### Added
- Payment spend guardrails in worker (`worker/src/lib.rs`):
  - `PAYMENT_MAX_SPEND_MSAT_PER_TENANT`
  - `PAYMENT_MAX_SPEND_MSAT_PER_AGENT`
- Core payment spend aggregation DB APIs (`core/src/db.rs`):
  - `sum_executed_payment_amount_msat_for_tenant(...)`
  - `sum_executed_payment_amount_msat_for_agent(...)`
- New worker integration coverage for payment cap enforcement:
  - tenant spend cap denial
  - agent spend cap denial
- Roadmap milestone `M6C — Token Budget Governance` added in `docs/ROADMAP.md`.

### Changed
- `payment.send` now records a payment request before budget checks and returns deterministic duplicate outcomes by idempotency key.
- Budget/approval failures for `payment.send` now persist failed payment ledger records (`payment_results`, `payment_requests.status='failed'`) for auditability.
- Worker startup logs include tenant/agent payment budget configuration.
- Updated operational and developer docs for new payment cap knobs:
  - `docs/DEVELOPMENT.md`
  - `docs/OPERATIONS.md`
  - `docs/SESSION_HANDOFF.md`

### Tests
- Verified:
  - `cargo test`
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.50 — Add enforced coverage gate and M5C approval-threshold guardrail

### Added
- Coverage automation:
  - `make coverage` (workspace coverage with line-threshold gate)
  - `make coverage-db` (coverage including DB integration suites)
  - `COVERAGE_MIN_LINES` make variable (default `70`)
- CI coverage enforcement in `.github/workflows/ci.yml`:
  - installs `cargo-llvm-cov`
  - runs `make coverage-db`
  - Postgres CI service updated to `postgres:18`
- M5C payment safety control in worker:
  - `PAYMENT_APPROVAL_THRESHOLD_MSAT` runtime config
  - `payment.send` now requires action arg `"payment_approved": true` when `amount_msat` meets/exceeds threshold
- New worker integration coverage for payment approval gates:
  - denied when threshold requires approval and approval flag is absent
  - allowed when approval flag is present

### Changed
- Worker startup logs now include payment approval threshold config.
- Python reference skill now supports optional `payment_approved` input passthrough for `payment.send`.

### Tests
- Verified:
  - `cargo test`
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.49 — Start M5C payments baseline (NWC-first) with ledger persistence and worker execution

### Added
- New payment migration `migrations/0007_payment_ledger.sql`:
  - `payment_requests` table with tenant-scoped idempotency key uniqueness
  - `payment_results` table for execution outcomes/errors
- Core payment DB APIs in `core/src/db.rs`:
  - `create_or_get_payment_request(...)`
  - `create_payment_result(...)`
  - `get_latest_payment_result(...)`
  - `update_payment_request_status(...)`
- Worker `payment.send` execution baseline:
  - NWC-first destination parsing (`nwc:<target>`)
  - operations: `pay_invoice`, `make_invoice`, `get_balance`
  - required `idempotency_key`
  - outbox artifact persistence under `payments/...`
- New worker runtime knobs:
  - `PAYMENT_NWC_ENABLED`
  - `PAYMENT_NWC_MOCK_BALANCE_MSAT`
  - `PAYMENT_MAX_SPEND_MSAT_PER_RUN`

### Changed
- Policy model now recognizes `payment.send` capability/action type (`core/src/policy.rs`).
- API capability normalization now supports `payment.send` with `nwc:*` scope only.
- Added `payments_v1` recipe bundle in API grant resolver for default `payment.send` grants.
- Python reference skill can now emit `payment.send` action requests for integration testing.

### Tests
- Added policy unit coverage for `payment.send` scope gating.
- Added core DB integration coverage for payment request idempotency and payment result persistence.
- Added API integration coverage for `payments_v1` grant behavior.
- Added worker integration coverage for:
  - successful `payment.send` execution
  - run-level payment budget enforcement failures
- Verified DB-backed suites:
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

## v0.0.48 — Complete M4B scheduler hardening with lease coordination, jitter, and trigger ownership guards

### Added
- New migration `migrations/0006_trigger_jitter_and_scheduler_leases.sql`:
  - `triggers.jitter_seconds` with bounded check (`0..=3600`)
  - `scheduler_leases` table for HA-safe scheduler lease coordination
- Scheduler lease acquisition primitive in `core/src/db.rs`:
  - `try_acquire_scheduler_lease(...)`
  - exported `SchedulerLeaseParams`
- Trigger jitter support across interval and cron trigger scheduling paths:
  - create/update validation + persistence + dispatch application

### Changed
- Worker trigger scheduler now supports lease-gated dispatch controls:
  - `WORKER_TRIGGER_SCHEDULER_LEASE_ENABLED` (default `1`)
  - `WORKER_TRIGGER_SCHEDULER_LEASE_NAME` (default `default`)
  - `WORKER_TRIGGER_SCHEDULER_LEASE_TTL_MS` (default `3000`)
- API trigger mutation ownership controls were tightened:
  - operator-trigger mutations now require `x-user-id`
  - operators can only create/mutate triggers for their own user id
- Trigger APIs now support `jitter_seconds` on create/update and in trigger responses.
- Upgrade safety fix:
  - moved new schema changes from edited `0005` into additive `0006` migration so existing environments that already applied `0005` can upgrade cleanly.

### Fixed
- Corrected webhook trigger insert placeholder mismatch in `core/src/db.rs` that caused SQL error `INSERT has more target columns than expressions` during DB-backed tests.
- API test request helper now sends JSON bodies for `PATCH` requests (previously caused `415` in DB-backed lifecycle tests).

### Tests
- Added API integration coverage for operator trigger mutation without `x-user-id` (`403`).
- DB-backed suites validated:
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`

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
