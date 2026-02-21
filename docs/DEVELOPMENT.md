# Development Guide

This is a living document for contributors building SecureAgnt locally.

## Scope
- Local developer setup
- Build, lint, and test workflows
- Local Postgres workflow for migrations and integration testing

## Prerequisites
- Rust toolchain (stable) with `rustfmt` and `clippy`
- Podman (preferred) with compose support, or Docker with Compose
- `sqlx-cli` for migration commands
- Optional secret-provider CLIs when testing cloud secret references:
  - `vault` (HashiCorp Vault)
  - `aws` (AWS CLI)
  - `gcloud` (Google Cloud CLI)
  - `az` (Azure CLI)

Install `sqlx-cli`:

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

## Repository bootstrap

```bash
git clone <repo-url>
cd secureagnt
```

## Local database (shared service model, local instance)
SecureAgnt uses a shared Postgres service per environment. In local dev, run one local Postgres container and one standardized app schema.
Default compose file path: `infra/containers/compose.yml`.

Container profiles:
- default profile: `postgres` only (`make db-up`)
- `stack` profile: `postgres + secureagnt-api + secureagntd` (`make stack-up`)
  - API container auto-runs DB migrations on startup (`API_RUN_MIGRATIONS=1` in compose profile)
  - container builds are throttled via `SECUREAGNT_CARGO_BUILD_JOBS` (default `2`)

Service packaging templates:
- systemd unit files live in `infra/systemd/`:
  - `secureagnt.service`
  - `secureagnt-api.service`

Start/stop DB:

```bash
make container-info
make db-up
make db-down
```

Start/stop full containerized stack (DB + API + worker):

```bash
make stack-build
make stack-up
make stack-ps
make stack-logs
make stack-down
```

Optional deployment profile presets (export before `make stack-up*`):

```bash
set -a
source infra/config/profile.solo-dev.env
set +a
```

or:

```bash
set -a
source infra/config/profile.enterprise.env
set +a
```

Initialize per-agent context profile templates (optional):

```bash
TENANT_ID=single AGENT_ID=<agent-uuid> AGENT_NAME="show-producer" make agent-context-init
```

Build behavior:
- `make stack-up` reuses existing images (no rebuild).
- `make stack-up-build` rebuilds and starts the stack.
- `make stack-build` rebuilds only.

If auto-detection picks the wrong runtime, override it explicitly:

```bash
COMPOSE_CMD="podman compose" make db-up
```

If you keep an alternate compose file, override that too:

```bash
COMPOSE_FILE=infra/containers/compose.yml make db-up
```

Useful runtime checks:

```bash
make container-info
```
- Shows which compose command the Makefile detected and prints available runtime versions.

```bash
COMPOSE_CMD="podman compose" make db-up
COMPOSE_CMD="podman compose" make db-down
```
- Forces Podman compose regardless of auto-detection.

```bash
podman ps
```
- Confirms the Postgres container is running after `make db-up`.

Default connection:

```bash
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb
```

## Build and quality commands

```bash
make build
make fmt
make lint
make verify
make verify-db
make test
make test-db
make test-worker-db
make test-api-db
make check
make runbook-validate
make validation-gate
make governance-gate
make release-manifest
make release-manifest-verify
make deploy-preflight
```

Run services:

```bash
make api
make worker
make secureagnt-api
make secureagntd
make agntctl
make soak-gate
make perf-gate
make compliance-gate
make isolation-gate
make m5c-signoff
make m6-signoff
make m6a-signoff
make m7-signoff
make m8-signoff
make m8a-signoff
make m9-signoff
make m10-signoff
make governance-gate
make capture-perf-baseline
make security-gate
make validation-gate
make release-gate
make release-manifest
make release-manifest-verify
make deploy-preflight
make stack-build
make stack-up
make stack-up-build
make stack-ps
make stack-logs
make stack-down
```

`make soak-gate` runs `scripts/ops/soak_gate.sh`, which repeatedly evaluates `/v1/ops/summary` thresholds through:
- `agntctl ops soak-gate`
- optional summary fixture input via `SUMMARY_JSON` (`--summary-json`)
- optional action-latency threshold checks via `/v1/ops/action-latency`
- configurable thresholds via env vars (`MAX_QUEUED_RUNS`, `MAX_FAILED_RUNS_WINDOW`, `MAX_DEAD_LETTER_EVENTS_WINDOW`, `MAX_P95_RUN_DURATION_MS`, optional `MAX_AVG_RUN_DURATION_MS`, optional `MAX_ACTION_P95_MS`, optional `MAX_ACTION_FAILED_RATE_PCT`, optional `MAX_ACTION_DENIED_RATE_PCT`)

`make perf-gate` runs `scripts/ops/perf_gate.sh`, which compares candidate latency metrics against a baseline through:
- `agntctl ops perf-gate`
- required baseline fixture inputs (`BASELINE_SUMMARY_JSON`, `BASELINE_HISTOGRAM_JSON`, `BASELINE_TRACES_JSON`)
- configurable regression thresholds (`MAX_P95_REGRESSION_MS`, `MAX_AVG_REGRESSION_MS`, `TAIL_BUCKET_LOWER_MS`, `MAX_TAIL_REGRESSION_PCT`, `MAX_TRACE_P99_REGRESSION_MS`, `MAX_TRACE_MAX_REGRESSION_MS`, `MAX_TRACE_TOP5_AVG_REGRESSION_MS`)

`make capture-perf-baseline` runs `scripts/ops/capture_perf_baseline.sh`, which snapshots current API telemetry into local baseline files through:
- `agntctl ops capture-baseline`
- output controls:
  - `CAPTURE_BASELINE_OUTPUT_DIR` (default `agntctl/fixtures/generated`)
  - `CAPTURE_BASELINE_PREFIX` (default `ops_baseline_<utc_timestamp>`)
- API/tenant controls:
  - `AGNTCTL_API_BASE_URL`
  - `AGNTCTL_TENANT_ID`
  - `AGNTCTL_USER_ROLE`
  - `WINDOW_SECS`
  - `TRACE_LIMIT`

`make validation-gate` runs `scripts/ops/validation_gate.sh`, which executes the reusable validation sequence:
- `make runbook-validate`
- `make verify`
- `make security-gate`
- fixture-backed `make compliance-gate`
- `make governance-gate`
- fixture-backed `make perf-gate`
- optional explicit DB suite re-run (`VALIDATION_GATE_RUN_DB_SUITES=1`) via:
  - `make isolation-gate`
  - `make test-db`
  - `make test-api-db`
  - `make test-worker-db`
- optional coverage gate (`VALIDATION_GATE_RUN_COVERAGE=1`) via:
  - `make coverage`

`make release-gate` runs `scripts/ops/release_gate.sh`, which executes:
- `make validation-gate`
- optional `make soak-gate` (`RELEASE_GATE_SKIP_SOAK=0`)
- optional DB suite re-run pass-through (`RELEASE_GATE_RUN_DB_SUITES=1`)
- optional coverage pass-through (`RELEASE_GATE_RUN_COVERAGE=1`)
- optional security-gate DB worker pass-through (`RELEASE_GATE_RUN_DB_SECURITY=1`)
- optional governance-gate pass-through (`RELEASE_GATE_RUN_GOVERNANCE=0` to disable)

Deployment prep scaffolding:
- `make release-manifest` writes a SHA-256 manifest for deployment artifacts (default `dist/release-manifest.sha256`)
- `make release-manifest-verify` verifies a previously generated manifest
- `make deploy-preflight` validates required deployment templates and optionally verifies manifest integrity (`DEPLOY_PREFLIGHT_VERIFY_MANIFEST=1`)
- `make governance-gate` enforces manifest generate+verify and deploy preflight with manifest verification

`make compliance-gate` runs `scripts/ops/compliance_gate.sh`, which evaluates:
- compliance tamper-chain verification status
- SIEM delivery SLO thresholds (hard-failure/dead-letter rates, optional oldest-pending age)
- optional per-target SIEM thresholds (hard-failure/dead-letter/pending)
- optional fixture mode with:
  - `VERIFY_JSON`
  - `SLO_JSON`
  - `TARGETS_JSON`

Milestone closure helpers:
- `make m5c-signoff` runs `scripts/ops/m5c_signoff.sh` for payment milestone exit-criteria checks (allow/deny, budgets, idempotency replay, NWC relay path).
- `make m6-signoff` runs `scripts/ops/m6_signoff.sh` for security hardening exit-criteria checks (policy deny/allow invariants, skill containment, worker denial/redaction boundaries).
- `make m6a-signoff` runs `scripts/ops/m6a_signoff.sh` for memory-plane exit-criteria checks (memory isolation, retention enforcement, redaction-before-indexing, concurrent retrieval benchmark).
- `make m7-signoff` runs `scripts/ops/m7_signoff.sh` for tenant-isolation and tenant-capacity exit-criteria checks.
- `make m8-signoff` runs `scripts/ops/m8_signoff.sh` for production-readiness exit-criteria checks (ops endpoint coverage, runbook validation, fixture-backed perf/soak threshold gates).
- `make m8a-signoff` runs `scripts/ops/m8a_signoff.sh` for compliance routing/export/retention/tamper/runbook exit-criteria checks.
- `make m9-signoff` runs `scripts/ops/m9_signoff.sh` for governance exit-criteria checks (supply-chain gate, approval-gate enforcement, skill digest provenance checks).
- `make m10-signoff` runs `scripts/ops/m10_signoff.sh` for cross-platform packaging/docs baseline checks.

`make security-gate` runs `scripts/ops/security_gate.sh` and enforces security-critical checks:
- core policy deny/allow invariants
- core redaction behavior
- skillrunner containment (env scrubbing + timeout kill)
- optional DB-backed worker deny-by-default boundary tests (`local.exec`, `llm.infer`, `message.send`) and redaction persistence checks
  - enable with `RUN_DB_SECURITY=1 make security-gate` (or `RUN_DB_TESTS=1 make security-gate`)

Worker runtime knobs (optional):

```bash
export WORKER_SKILL_COMMAND=python3
export WORKER_SKILL_SCRIPT=skills/python/summarize_transcript/main.py
export WORKER_SKILL_TIMEOUT_MS=5000
export WORKER_SKILL_ENV_ALLOWLIST=LANG,LC_ALL
export WORKER_ARTIFACT_ROOT=artifacts
export WORKER_TRIGGER_SCHEDULER_ENABLED=1
export WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS=100
export WORKER_TRIGGER_SCHEDULER_LEASE_ENABLED=1
export WORKER_TRIGGER_SCHEDULER_LEASE_NAME=default
export WORKER_TRIGGER_SCHEDULER_LEASE_TTL_MS=3000
export WORKER_MEMORY_COMPACTION_ENABLED=1
export WORKER_MEMORY_COMPACTION_MIN_RECORDS=10
export WORKER_MEMORY_COMPACTION_MAX_GROUPS_PER_CYCLE=5
export WORKER_MEMORY_COMPACTION_MIN_AGE_SECS=300
export WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED=0
export WORKER_COMPLIANCE_SIEM_DELIVERY_BATCH_SIZE=10
export WORKER_COMPLIANCE_SIEM_DELIVERY_LEASE_MS=30000
export WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_BACKOFF_MS=5000
export WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_JITTER_MAX_MS=500
export WORKER_COMPLIANCE_SIEM_HTTP_ENABLED=0
export WORKER_COMPLIANCE_SIEM_HTTP_TIMEOUT_MS=5000
export WORKER_COMPLIANCE_SIEM_HTTP_AUTH_HEADER=authorization
export WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN=
export WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN_REF=
export API_TENANT_MAX_INFLIGHT_RUNS=
export API_TENANT_MAX_TRIGGERS=
export API_TENANT_MAX_MEMORY_RECORDS=
```

Artifact layout note:
- worker writes run side-effect artifacts under tenant-scoped directories:
  - `<WORKER_ARTIFACT_ROOT>/tenants/<tenant_id>/shownotes/...`
  - `<WORKER_ARTIFACT_ROOT>/tenants/<tenant_id>/messages/...`
  - `<WORKER_ARTIFACT_ROOT>/tenants/<tenant_id>/payments/...`

`WORKER_SKILL_ENV_ALLOWLIST` is optional. By default, skills run with a cleared environment (`env_clear`) plus `SECUREAGNT_SKILL_SANDBOXED=1`.
Add only the minimum env vars a specific skill runtime requires.

Local sandbox exec knobs (disabled by default):

```bash
export WORKER_LOCAL_EXEC_ENABLED=1
export WORKER_LOCAL_EXEC_READ_ROOTS=/path/to/secureagnt/docs
export WORKER_LOCAL_EXEC_WRITE_ROOTS=/path/to/secureagnt/artifacts
export WORKER_LOCAL_EXEC_TIMEOUT_MS=2000
export WORKER_LOCAL_EXEC_MAX_OUTPUT_BYTES=16384
export WORKER_LOCAL_EXEC_MAX_MEMORY_BYTES=268435456
export WORKER_LOCAL_EXEC_MAX_PROCESSES=32
```

The local exec primitive is template-only (`file.head`, `file.word_count`, `file.touch`) and capability-scoped by template id (`local.exec:<template_id>`).

LLM runtime knobs (local-first default):

```bash
# Routing mode: local_only | local_first | remote_only
export LLM_MODE=local_first
export LLM_MAX_INPUT_BYTES=262144
export LLM_LARGE_INPUT_THRESHOLD_BYTES=12000
export LLM_LARGE_INPUT_POLICY=summarize_first
export LLM_LARGE_INPUT_SUMMARY_TARGET_BYTES=8000
export LLM_CONTEXT_RETRIEVAL_TOP_K=6
export LLM_CONTEXT_RETRIEVAL_MAX_BYTES=32000
export LLM_CONTEXT_RETRIEVAL_CHUNK_BYTES=2048
export LLM_ADMISSION_ENABLED=1
export LLM_ADMISSION_INTERACTIVE_MAX_INFLIGHT=8
export LLM_ADMISSION_BATCH_MAX_INFLIGHT=2
export LLM_CACHE_ENABLED=0
export LLM_CACHE_TTL_SECS=300
export LLM_CACHE_MAX_ENTRIES=1024
export LLM_VERIFIER_ENABLED=0
export LLM_VERIFIER_MIN_SCORE_PCT=65
export LLM_VERIFIER_ESCALATE_REMOTE=1
export LLM_VERIFIER_MIN_RESPONSE_CHARS=48

# Local OpenAI-compatible endpoint (default values shown)
export LLM_LOCAL_BASE_URL=http://127.0.0.1:11434/v1
export LLM_LOCAL_MODEL=qwen2.5:7b-instruct
# Optional local endpoint auth
export LLM_LOCAL_API_KEY=
export LLM_LOCAL_API_KEY_REF=

# Optional remote endpoint (only used when configured + mode/route selects remote)
export LLM_REMOTE_BASE_URL=https://api.openai.com/v1
export LLM_REMOTE_MODEL=gpt-4o-mini
export LLM_REMOTE_API_KEY=<secret>
export LLM_REMOTE_API_KEY_REF=
export LLM_REMOTE_EGRESS_ENABLED=0
export LLM_REMOTE_EGRESS_CLASS=cloud_allowed
export LLM_REMOTE_HOST_ALLOWLIST=api.openai.com
export LLM_REMOTE_TOKEN_BUDGET_PER_RUN=
export LLM_REMOTE_TOKEN_BUDGET_PER_TENANT=
export LLM_REMOTE_TOKEN_BUDGET_PER_AGENT=
export LLM_REMOTE_TOKEN_BUDGET_PER_MODEL=
export LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS=86400
export LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT=
export LLM_REMOTE_COST_PER_1K_TOKENS_USD=0.0

export LLM_TIMEOUT_MS=12000
export LLM_MAX_PROMPT_BYTES=32000
export LLM_MAX_OUTPUT_BYTES=64000
```

Replay package manifest signing (optional):

```bash
export COMPLIANCE_REPLAY_SIGNING_KEY=
export COMPLIANCE_REPLAY_SIGNING_KEY_REF=
```

If a signing key is configured, replay package responses include `manifest.signing_mode=hmac-sha256` and a `manifest.signature`.

`llm.infer` scope convention:
- local route: `local:*` or `local:<model>`
- remote route: `remote:*` or `remote:<model>`

Nostr signer runtime knobs:

```bash
# Default mode if unset:
export NOSTR_SIGNER_MODE=local_key
```

Local key mode (self-hosted / smaller deployment friendly):

```bash
# Option A: direct env secret (nsec or hex)
export NOSTR_SECRET_KEY=<nsec_or_hex_secret>

# Option B: file-based secret (preferred vs shell history leakage)
chmod 600 .secrets/nostr.key
export NOSTR_SECRET_KEY_FILE=.secrets/nostr.key
```

NIP-46 mode (enterprise/hardened option, private key stays off worker host):

```bash
export NOSTR_SIGNER_MODE=nip46_signer
export NOSTR_NIP46_BUNKER_URI='bunker://<npub>?relay=wss://relay.example'
# Optional if bunker URI already contains npub:
export NOSTR_NIP46_PUBLIC_KEY=<npub_or_hex_pubkey>
# Optional client app key used for NIP-46 handshake/session continuity:
export NOSTR_NIP46_CLIENT_SECRET_KEY=<nsec_or_hex_secret>
```

Relay publish knobs:

```bash
# Comma-separated relay URLs for White Noise transport publish
export NOSTR_RELAYS='wss://relay1.example,wss://relay2.example'
export NOSTR_PUBLISH_TIMEOUT_MS=4000
```

Slack delivery knobs (enterprise-secondary path):

```bash
export SLACK_WEBHOOK_URL=https://hooks.slack.com/services/xxx/yyy/zzz
export SLACK_WEBHOOK_URL_REF=
export SLACK_SEND_TIMEOUT_MS=4000
export SLACK_MAX_ATTEMPTS=3
export SLACK_RETRY_BACKOFF_MS=500
export WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST=
export WORKER_MESSAGE_SLACK_DEST_ALLOWLIST=
```

Payment rail knobs (M5C baseline, NWC-first):

```bash
export PAYMENT_NWC_ENABLED=1
export PAYMENT_NWC_URI=
export PAYMENT_NWC_URI_REF=
export PAYMENT_NWC_WALLET_URIS=
export PAYMENT_NWC_WALLET_URIS_REF=
export PAYMENT_NWC_TIMEOUT_MS=5000
export PAYMENT_NWC_ROUTE_STRATEGY=ordered
export PAYMENT_NWC_ROUTE_FALLBACK_ENABLED=1
export PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT=100
export PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD=3
export PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS=60
export PAYMENT_NWC_MOCK_BALANCE_MSAT=1000000
export PAYMENT_MAX_SPEND_MSAT_PER_RUN=50000
export PAYMENT_APPROVAL_THRESHOLD_MSAT=10000
export PAYMENT_MAX_SPEND_MSAT_PER_TENANT=500000
export PAYMENT_MAX_SPEND_MSAT_PER_AGENT=100000
```

Cashu scaffold knobs (routing/config scaffold is active; default remains fail-closed):

```bash
export PAYMENT_CASHU_ENABLED=0
export PAYMENT_CASHU_MINT_URIS=
export PAYMENT_CASHU_MINT_URIS_REF=
export PAYMENT_CASHU_DEFAULT_MINT=
export PAYMENT_CASHU_TIMEOUT_MS=5000
export PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN=
export PAYMENT_CASHU_MOCK_ENABLED=0
export PAYMENT_CASHU_MOCK_BALANCE_MSAT=1000000
export PAYMENT_CASHU_HTTP_ENABLED=0
export PAYMENT_CASHU_HTTP_ALLOW_INSECURE=0
export PAYMENT_CASHU_AUTH_HEADER=authorization
export PAYMENT_CASHU_AUTH_TOKEN=
export PAYMENT_CASHU_AUTH_TOKEN_REF=
```

For payment rail behavior and phased Cashu plan details, see `docs/PAYMENTS.md`.

Memory write note:
- API memory writes apply server-side redaction before persistence/indexing.
- `redaction_applied` is set when redaction occurs or when explicitly flagged by caller.

Secret reference format (shared resolver):
- `env:VAR_NAME`
- `file:/path/to/secret.txt`
- cloud provider schemes (enabled only when `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI=1`): `vault:...`, `aws-sm:...`, `gcp-sm:...`, `azure-kv:...`
- optional version pin query parameters are supported per backend:
  - Vault: `vault:kv/data/app/slack#token?version=3`
  - AWS SM: `aws-sm:prod/secureagnt/slack?version_id=<id>` or `?version_stage=AWSCURRENT`
  - GCP SM: `gcp-sm:project:secret?version=42`
  - Azure KV: `azure-kv:https://vault/secrets/name?version=<id>`

Cloud secret adapter gate:

```bash
export SECUREAGNT_SECRET_ENABLE_CLOUD_CLI=1
```

When this gate is off (default), cloud secret references fail closed.

Secret cache controls (API/worker shared resolver):

```bash
export SECUREAGNT_SECRET_CACHE_TTL_SECS=30
export SECUREAGNT_SECRET_CACHE_MAX_ENTRIES=1024
```

- `SECUREAGNT_SECRET_CACHE_TTL_SECS=0` disables caching.
- Rotation-friendly default: keep TTL short in dev/staging, then tune for production backend load.

Behavior notes:
- `local_key` is default and optional; if no local key is configured, worker starts with Nostr signing disabled.
- `nip46_signer` is strict; missing/invalid bunker configuration fails worker startup.
- `message.send` always writes connector envelopes to local outbox artifacts under `WORKER_ARTIFACT_ROOT/messages/...`.
- Cashu destinations (`cashu:<mint_id>`) fail closed by default; enable either:
  - deterministic local/dev mock mode (`PAYMENT_CASHU_MOCK_ENABLED=1`), or
  - live HTTP settlement mode (`PAYMENT_CASHU_HTTP_ENABLED=1`) with mint allowlist configuration.
- If `NOSTR_RELAYS` is configured, White Noise `message.send` publishes signed Nostr events to relays and records ACK outcomes.
- Optional destination allowlists can harden `message.send` routing:
  - `WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST` (comma-separated White Noise targets)
  - `WORKER_MESSAGE_SLACK_DEST_ALLOWLIST` (comma-separated Slack channel ids)
  - when set, destinations outside the allowlist fail closed.
- Signing source depends on signer mode:
  - `local_key`: signs with local secret key material.
  - `nip46_signer`: signs remotely through the configured bunker (`NOSTR_NIP46_BUNKER_URI`), with optional app key from `NOSTR_NIP46_CLIENT_SECRET_KEY`.
- Worker stores redacted values for sensitive action/audit payload fields (`token`, `secret`, `password`, `authorization`, `nsec` patterns).
- Secrets resolved by reference use TTL caching to reduce repeated backend calls while still refreshing rotated values after cache expiry.
- `llm.infer` defaults to local route in `local_first` mode and only uses remote endpoints when explicitly preferred and allowed by policy/grants.
- Agent-context profile loading (M12 runtime baseline):
  - enable with `WORKER_AGENT_CONTEXT_ENABLED=1`
  - force fail-closed when profile missing/invalid with `WORKER_AGENT_CONTEXT_REQUIRED=1`
  - configure root with `WORKER_AGENT_CONTEXT_ROOT` (default `agent_context`)
  - optional required file override via `WORKER_AGENT_CONTEXT_REQUIRED_FILES` (CSV)
  - size controls:
    - `WORKER_AGENT_CONTEXT_MAX_FILE_BYTES`
    - `WORKER_AGENT_CONTEXT_MAX_TOTAL_BYTES`
    - `WORKER_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR`
  - directory resolution order:
    - `<root>/<tenant_id>/<agent_id>/`
    - `<root>/<agent_id>/`
  - loaded context is injected into skill input as `agent_context`
  - audit events emitted:
    - `agent.context.loaded`
    - `agent.context.not_found`
    - `agent.context.error`
- Remote `llm.infer` is blocked unless both are set:
  - `LLM_REMOTE_EGRESS_ENABLED=1`
  - remote host included in `LLM_REMOTE_HOST_ALLOWLIST`
- `LLM_REMOTE_EGRESS_CLASS` controls remote egress posture:
  - `cloud_allowed` (default): remote allowed when egress gate + allowlist pass.
  - `redacted_only`: remote allowed only when action args include `redacted=true`.
  - `never_leaves_prem`: all remote routes fail closed.
- `llm.infer` action results now include `gateway` metadata for deterministic routing audits:
  - `gateway.selected_route`
  - `gateway.reason_code`
  - `gateway.remote_egress_class`
  - `gateway.remote_host` (when remote selected)
  - `gateway.request_class` / `gateway.queue_lane`
  - `gateway.large_input_policy`
  - `gateway.large_input_applied`
  - `gateway.large_input_reason_code`
  - `gateway.prompt_bytes_original` / `gateway.prompt_bytes_effective`
  - `gateway.retrieval_candidate_documents` / `gateway.retrieval_selected_documents`
- `llm.infer` large-input control path:
  - default behavior from `LLM_LARGE_INPUT_POLICY`
  - per-action override via `args.large_input_policy`
  - supported request classes via `args.request_class` (`interactive`, `batch`)
  - optional context retrieval payload:
    - `args.context_documents` (array of `{id|path|source, text}`)
    - `args.context_query`
    - optional `args.context_top_k`
  - optional `args.context_max_bytes`
- `llm.infer` admission/cache/verifier controls:
  - admission gates:
    - `LLM_ADMISSION_ENABLED`
    - `LLM_ADMISSION_INTERACTIVE_MAX_INFLIGHT`
    - `LLM_ADMISSION_BATCH_MAX_INFLIGHT`
  - optional distributed admission/cache (recommended only for multi-worker deployments):
    - `LLM_DISTRIBUTED_ENABLED`
    - `LLM_DISTRIBUTED_FAIL_OPEN`
    - `LLM_DISTRIBUTED_OWNER`
    - `LLM_DISTRIBUTED_ADMISSION_ENABLED`
    - `LLM_DISTRIBUTED_ADMISSION_LEASE_MS`
    - `LLM_DISTRIBUTED_CACHE_ENABLED`
    - `LLM_DISTRIBUTED_CACHE_NAMESPACE_MAX_ENTRIES`
    - uses Postgres tables:
      - `llm_gateway_admission_leases`
      - `llm_gateway_cache_entries`
  - response cache:
    - `LLM_CACHE_ENABLED`
    - `LLM_CACHE_TTL_SECS`
    - `LLM_CACHE_MAX_ENTRIES`
  - verifier escalation:
    - `LLM_VERIFIER_ENABLED`
    - `LLM_VERIFIER_MODE` (`heuristic`, `deterministic`, `model_judge`, `hybrid`)
    - `LLM_VERIFIER_MIN_SCORE_PCT`
    - `LLM_VERIFIER_ESCALATE_REMOTE`
    - `LLM_VERIFIER_MIN_RESPONSE_CHARS`
    - optional judge endpoint:
      - `LLM_VERIFIER_JUDGE_BASE_URL`
      - `LLM_VERIFIER_JUDGE_MODEL`
      - `LLM_VERIFIER_JUDGE_API_KEY` / `LLM_VERIFIER_JUDGE_API_KEY_REF`
      - `LLM_VERIFIER_JUDGE_TIMEOUT_MS`
      - `LLM_VERIFIER_JUDGE_FAIL_OPEN`
- Optional remote-spend controls:
  - `LLM_REMOTE_TOKEN_BUDGET_PER_RUN` enforces a per-run remote token cap (preflight check from action `max_tokens`, default estimate `512`).
  - `LLM_REMOTE_TOKEN_BUDGET_PER_TENANT` enforces a rolling-window tenant remote token cap.
  - `LLM_REMOTE_TOKEN_BUDGET_PER_AGENT` enforces a rolling-window agent remote token cap.
  - `LLM_REMOTE_TOKEN_BUDGET_PER_MODEL` enforces a rolling-window remote model cap (`remote:<model>` key).
  - `LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS` controls the shared rolling window for tenant/agent/model budgets (default `86400`).
  - `LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT` emits soft-alert audit events when usage reaches threshold percent (`1..100`) without hard-stopping execution.
  - `LLM_REMOTE_COST_PER_1K_TOKENS_USD` adds estimated USD cost metadata to `llm.infer` action results.
  - Remote usage accounting is persisted to `llm_token_usage` for deterministic budget enforcement across runs.

For backend auth strategy and full reference syntax, see `docs/SECRETS.md`.
- `message.send` to `slack:*` delivers via webhook when `SLACK_WEBHOOK_URL` is configured; otherwise it remains queued in local outbox artifacts.
- Slack webhook delivery retries with exponential backoff (`SLACK_MAX_ATTEMPTS`, `SLACK_RETRY_BACKOFF_MS`) and transitions to `dead_lettered_local_outbox` when attempts are exhausted.
- API run creation supports optional role preset header for capability narrowing during local testing:
  - `x-user-role: owner` (default), `operator`, `viewer`
- Optional trusted proxy auth guardrail for role/user headers:
  - `API_TRUSTED_PROXY_AUTH_ENABLED=1`
  - configure shared token via:
    - `API_TRUSTED_PROXY_SHARED_SECRET`, or
    - `API_TRUSTED_PROXY_SHARED_SECRET_REF`
  - when enabled, role-scoped calls require `x-auth-proxy-token`
- Compliance alert acknowledgement workflow:
  - `POST /v1/audit/compliance/siem/deliveries/alerts/ack`
  - requires `x-user-id` header (`owner`/`operator`) for audit attribution
  - supports optional `run_id` scoping and optional free-text note
- Optional API tenant capacity guardrails:
  - `API_TENANT_MAX_INFLIGHT_RUNS` limits queued/running runs for `POST /v1/runs`
  - `API_TENANT_MAX_TRIGGERS` limits total trigger definitions for `POST /v1/triggers*`
  - `API_TENANT_MAX_MEMORY_RECORDS` limits active memory rows for `POST /v1/memory/records` and `POST /v1/memory/handoff-packets`
- Agent-context API controls:
  - loader root/config controls:
    - `API_AGENT_CONTEXT_ROOT` (default `agent_context`)
    - `API_AGENT_CONTEXT_REQUIRED_FILES` (CSV)
    - `API_AGENT_CONTEXT_MAX_FILE_BYTES`
    - `API_AGENT_CONTEXT_MAX_TOTAL_BYTES`
    - `API_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR`
  - operator inspect endpoint:
    - `GET /v1/agents/{agent_id}/context` (`owner`/`operator`, `viewer` denied)
  - heartbeat compile endpoint:
    - `POST /v1/agents/{agent_id}/heartbeat/compile`
    - compiles `HEARTBEAT.md` or inline markdown into trigger candidates with issue reporting
  - mutation endpoint (disabled by default):
    - `POST /v1/agents/{agent_id}/context`
    - enable with `API_AGENT_CONTEXT_MUTATION_ENABLED=1`
    - mutability enforcement:
      - immutable: `AGENTS.md`, `TOOLS.md`, `IDENTITY.md`, `SOUL.md` (always denied)
      - human-primary: `USER.md`, `HEARTBEAT.md` (owner only)
      - agent-managed: `MEMORY.md`, `memory/*.md`, `sessions/*.jsonl` (owner/operator)
      - `sessions/*.jsonl` is append-only
- Worker can auto-dispatch due triggers when `WORKER_TRIGGER_SCHEDULER_ENABLED=1`:
  - interval triggers (`POST /v1/triggers`)
  - cron triggers (`POST /v1/triggers/cron`)
  - queued webhook events (`POST /v1/triggers/webhook` + `POST /v1/triggers/{id}/events`)
- Trigger enqueue semantics:
  - webhook event ingestion distinguishes `duplicate` from trigger-unavailable outcomes.
  - API returns `409 CONFLICT` when trigger exists but is unavailable (`disabled` or schedule-broken state).
- Scheduler tenant guardrail:
  - `WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS` limits queued/running runs per tenant for trigger dispatch.
- Scheduler lease guardrail (HA-safe dispatch coordination):
  - `WORKER_TRIGGER_SCHEDULER_LEASE_ENABLED` gates lease acquisition before dispatch
  - `WORKER_TRIGGER_SCHEDULER_LEASE_NAME` chooses the shared lease key
  - `WORKER_TRIGGER_SCHEDULER_LEASE_TTL_MS` sets lease lifetime
- Trigger mutation ownership tests with `x-user-role=operator` should include `x-user-id` header.
- `payment.send` baseline uses `destination` scoped as `nwc:<wallet_target>` and supports:
  - `operation`: `pay_invoice`, `make_invoice`, `get_balance`
  - `idempotency_key`: required for settlement idempotency
  - live NIP-47 request/response path when either is configured:
    - per-wallet map: `PAYMENT_NWC_WALLET_URIS` / `PAYMENT_NWC_WALLET_URIS_REF`
    - single default fallback: `PAYMENT_NWC_URI` / `PAYMENT_NWC_URI_REF`
  - wallet map format:
    - CSV/newline entries: `wallet-main=nostr+walletconnect://...`
    - optional wildcard default entry: `*=nostr+walletconnect://...`
    - JSON object form is also accepted (`{"wallet-main":"nostr+walletconnect://..."}`)
    - multi-route value is supported with `|` separators: `wallet-main=uri_a|uri_b`
  - route strategy:
    - `PAYMENT_NWC_ROUTE_STRATEGY=ordered` (default): attempt routes in listed order
    - `PAYMENT_NWC_ROUTE_STRATEGY=deterministic_hash`: stable per-wallet/per-idempotency route selection
  - controlled rollout:
    - `PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT=100` (default): all requests use full multi-route candidates
    - lower values enable gradual canary rollout by deterministic wallet/idempotency bucket
    - `0` forces primary-route-only behavior even when fallback is enabled
  - failover control:
    - `PAYMENT_NWC_ROUTE_FALLBACK_ENABLED=1` (default): try additional routes on failure
    - `PAYMENT_NWC_ROUTE_FALLBACK_ENABLED=0`: fail fast on first route failure
  - route health quarantine:
    - `PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD` consecutive failures mark route unhealthy
    - `PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS` controls unhealthy cooldown window
    - unhealthy routes are skipped while cooling down, with skip metadata in `result.nwc.route`
  - `PAYMENT_NWC_TIMEOUT_MS` sets relay request timeout budget
  - destination should remain a logical wallet id; do not pass full `nostr+walletconnect://...` URIs in action args
  - if wallet mapping is configured but requested wallet id is missing (and no wildcard/default exists), payment fails closed
  - if no NWC URI is configured, worker uses the local mock rail (`nwc_mock`) for dev/test
  - optional run spend budget guardrail via `PAYMENT_MAX_SPEND_MSAT_PER_RUN`
  - optional tenant/agent spend budget guardrails via `PAYMENT_MAX_SPEND_MSAT_PER_TENANT` and `PAYMENT_MAX_SPEND_MSAT_PER_AGENT`
  - optional approval gate for high-value payouts via `PAYMENT_APPROVAL_THRESHOLD_MSAT`
    - if `amount_msat >= PAYMENT_APPROVAL_THRESHOLD_MSAT`, action args must include `"payment_approved": true`

## Migrations
Run migrations:

```bash
make migrate
```

Prepare sqlx offline metadata (when needed):

```bash
make sqlx-prepare
```

## Integration test notes
- Integration tests should use isolated test schemas per test run.
- Keep DB tests deterministic.
- Always cap loops/timeouts to avoid hanging CI.
- DB integration tests are enabled when `RUN_DB_TESTS=1`.

Run all tests with DB integration enabled:

```bash
RUN_DB_TESTS=1 TEST_DATABASE_URL=$DATABASE_URL cargo test
```

Run measured coverage locally:

```bash
make coverage
make coverage-db
```

See `docs/TESTING.md` for mandatory test coverage expectations.

## Workflow expectations
- Follow `AGENTS.md` non-negotiables.
- Keep trusted code paths small (`core` policy + primitives + dispatcher).
- Add or update tests in the same change as feature work.
- Update `CHANGELOG.md` for every meaningful repository change.
