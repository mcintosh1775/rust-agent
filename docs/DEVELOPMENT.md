# Development Guide

This is a living document for contributors building Aegis locally.

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
cd rust-agent
```

## Local database (shared service model, local instance)
Aegis uses a shared Postgres service per environment. In local dev, run one local Postgres container and one standardized app schema.
Default compose file path: `infra/containers/compose.yml`.

Start/stop DB:

```bash
make container-info
make db-up
make db-down
```

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
make fmt
make lint
make test
make test-db
make test-worker-db
make test-api-db
make check
```

Run services:

```bash
make api
make worker
```

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
```

`WORKER_SKILL_ENV_ALLOWLIST` is optional. By default, skills run with a cleared environment (`env_clear`) plus `AEGIS_SKILL_SANDBOXED=1`. Add only the minimum env vars a specific skill runtime requires.

Local sandbox exec knobs (disabled by default):

```bash
export WORKER_LOCAL_EXEC_ENABLED=1
export WORKER_LOCAL_EXEC_READ_ROOTS=/home/mcintosh/repos/rust-agent/docs
export WORKER_LOCAL_EXEC_WRITE_ROOTS=/home/mcintosh/repos/rust-agent/artifacts
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
export LLM_REMOTE_HOST_ALLOWLIST=api.openai.com
export LLM_REMOTE_TOKEN_BUDGET_PER_RUN=
export LLM_REMOTE_COST_PER_1K_TOKENS_USD=0.0

export LLM_TIMEOUT_MS=12000
export LLM_MAX_PROMPT_BYTES=32000
export LLM_MAX_OUTPUT_BYTES=64000
```

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
```

Payment rail knobs (M5C baseline, NWC-first):

```bash
export PAYMENT_NWC_ENABLED=1
export PAYMENT_NWC_URI=
export PAYMENT_NWC_URI_REF=
export PAYMENT_NWC_TIMEOUT_MS=5000
export PAYMENT_NWC_MOCK_BALANCE_MSAT=1000000
export PAYMENT_MAX_SPEND_MSAT_PER_RUN=50000
export PAYMENT_APPROVAL_THRESHOLD_MSAT=10000
export PAYMENT_MAX_SPEND_MSAT_PER_TENANT=500000
export PAYMENT_MAX_SPEND_MSAT_PER_AGENT=100000
```

Secret reference format (shared resolver):
- `env:VAR_NAME`
- `file:/path/to/secret.txt`
- cloud provider schemes (enabled only when `AEGIS_SECRET_ENABLE_CLOUD_CLI=1`): `vault:...`, `aws-sm:...`, `gcp-sm:...`, `azure-kv:...`

Cloud secret adapter gate:

```bash
export AEGIS_SECRET_ENABLE_CLOUD_CLI=1
```

When this gate is off (default), cloud secret references fail closed.

Behavior notes:
- `local_key` is default and optional; if no local key is configured, worker starts with Nostr signing disabled.
- `nip46_signer` is strict; missing/invalid bunker configuration fails worker startup.
- `message.send` always writes connector envelopes to local outbox artifacts under `WORKER_ARTIFACT_ROOT/messages/...`.
- If `NOSTR_RELAYS` is configured, White Noise `message.send` publishes signed Nostr events to relays and records ACK outcomes.
- Signing source depends on signer mode:
  - `local_key`: signs with local secret key material.
  - `nip46_signer`: signs remotely through the configured bunker (`NOSTR_NIP46_BUNKER_URI`), with optional app key from `NOSTR_NIP46_CLIENT_SECRET_KEY`.
- Worker stores redacted values for sensitive action/audit payload fields (`token`, `secret`, `password`, `authorization`, `nsec` patterns).
- `llm.infer` defaults to local route in `local_first` mode and only uses remote endpoints when explicitly preferred and allowed by policy/grants.
- Remote `llm.infer` is blocked unless both are set:
  - `LLM_REMOTE_EGRESS_ENABLED=1`
  - remote host included in `LLM_REMOTE_HOST_ALLOWLIST`
- Optional remote-spend controls:
  - `LLM_REMOTE_TOKEN_BUDGET_PER_RUN` enforces a per-run remote token cap (preflight check from action `max_tokens`, default estimate `512`).
  - `LLM_REMOTE_COST_PER_1K_TOKENS_USD` adds estimated USD cost metadata to `llm.infer` action results.
- `message.send` to `slack:*` delivers via webhook when `SLACK_WEBHOOK_URL` is configured; otherwise it remains queued in local outbox artifacts.
- Slack webhook delivery retries with exponential backoff (`SLACK_MAX_ATTEMPTS`, `SLACK_RETRY_BACKOFF_MS`) and transitions to `dead_lettered_local_outbox` when attempts are exhausted.
- API run creation supports optional role preset header for capability narrowing during local testing:
  - `x-user-role: owner` (default), `operator`, `viewer`
- Worker can auto-dispatch due triggers when `WORKER_TRIGGER_SCHEDULER_ENABLED=1`:
  - interval triggers (`POST /v1/triggers`)
  - cron triggers (`POST /v1/triggers/cron`)
  - queued webhook events (`POST /v1/triggers/webhook` + `POST /v1/triggers/{id}/events`)
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
  - live NIP-47 request/response path when `PAYMENT_NWC_URI` (or `PAYMENT_NWC_URI_REF`) is configured
  - `PAYMENT_NWC_TIMEOUT_MS` sets relay request timeout budget
  - destination should remain a logical wallet id; do not pass full `nostr+walletconnect://...` URIs in action args
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
