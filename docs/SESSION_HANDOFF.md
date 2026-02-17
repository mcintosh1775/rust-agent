# SESSION_HANDOFF

Use this file to bootstrap a new Codex session quickly and consistently.

## Project Identity
- Name: `Aegis`
- Goal: secure, high-performance Rust agent runtime replacing OpenClaw-style architecture
- Messaging direction: Nostr-first, White Noise first-class, Slack enterprise-secondary

## Current State Snapshot
- Milestones completed:
  - M1 policy contracts and tests (`core/policy`)
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
  - M5C/M6A planning captured: Nostr-first sats payments rail and durable memory-plane milestone definitions
  - M4B/M6B planning captured: durable trigger plane and provider-agnostic secrets interface (Vault + cloud backends)
  - M4B baseline implemented: interval trigger creation (`POST /v1/triggers`) + worker due-trigger dispatch + `trigger_runs` ledger
  - M4B expanded baseline implemented:
    - webhook trigger creation (`POST /v1/triggers/webhook`)
    - webhook event ingestion (`POST /v1/triggers/{id}/events`) with idempotent `event_id` dedupe
    - trigger event queue (`trigger_events`) with pending/processed/dead-lettered states
    - interval misfire skip handling and persisted trigger-run failure ledger entries
    - trigger provenance now includes `trigger_type` and optional `trigger_event_id` in worker `run.created` audits
  - M6B expanded baseline implemented:
    - shared secret reference abstraction with `env:`/`file:` runtime resolution
    - CLI-backed Vault/AWS/GCP/Azure resolver adapters behind fail-closed gate `AEGIS_SECRET_ENABLE_CLOUD_CLI`
    - worker/API secret-consuming paths now use `CliSecretResolver`

## Mandatory Read Order (for new sessions)
1. `AGENTS.md`
2. `docs/SESSION_HANDOFF.md` (this file)
3. `docs/agent_platform.md`
4. `docs/ARCHITECTURE.md`
5. `docs/SECURITY.md`
6. `docs/POLICY.md`
7. `docs/ROADMAP.md`
8. `CHANGELOG.md` (latest entries first)

## Critical ADRs
- `docs/ADR/ADR-0004-shared-postgres-topology.md` (shared DB topology)
- `docs/ADR/ADR-0005-nostr-first-whitenoise.md` (messaging priority)
- `docs/ADR/ADR-0006-sandboxed-local-exec-primitive.md` (sandbox boundary)
- `docs/ADR/ADR-0007-pluggable-nostr-signer-modes.md` (self-hosted + enterprise signer modes)

## Environment + Runtime Notes
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
  - gate: `AEGIS_SECRET_ENABLE_CLOUD_CLI=1`
- Webhook trigger knobs/behavior:
  - API create endpoint: `POST /v1/triggers/webhook`
  - API event ingest endpoint: `POST /v1/triggers/{id}/events`
  - optional `x-trigger-secret` header validation when trigger has `webhook_secret_ref`
  - trigger event payload guardrail: events above 64KB are rejected into retry/dead-letter flow
- API role preset knob:
  - optional request header `x-user-role` (`owner` default, `operator`, `viewer`)
- Skill runtime env control:
  - optional `WORKER_SKILL_ENV_ALLOWLIST` (comma-separated env vars passed through to skill process)
- Trigger scheduler control:
  - `WORKER_TRIGGER_SCHEDULER_ENABLED` (default on)
- Local exec sandbox control:
  - `WORKER_LOCAL_EXEC_ENABLED` plus path roots (`WORKER_LOCAL_EXEC_READ_ROOTS`, `WORKER_LOCAL_EXEC_WRITE_ROOTS`)
- LLM routing control:
  - `LLM_MODE` (`local_only`, `local_first`, `remote_only`)
  - local endpoint: `LLM_LOCAL_BASE_URL`, `LLM_LOCAL_MODEL`
  - optional remote endpoint: `LLM_REMOTE_BASE_URL`, `LLM_REMOTE_MODEL`, `LLM_REMOTE_API_KEY`
  - remote egress gate: `LLM_REMOTE_EGRESS_ENABLED` + `LLM_REMOTE_HOST_ALLOWLIST`
  - optional remote spend controls: `LLM_REMOTE_TOKEN_BUDGET_PER_RUN`, `LLM_REMOTE_COST_PER_1K_TOKENS_USD`

## Local Verification Commands
```bash
make container-info
make db-up
make test-db
make test-worker-db
make test-api-db
make test
```

## Key Code Areas
- Policy engine: `core/src/policy.rs`
- DB primitives and run-lease APIs: `core/src/db.rs`
- DB integration tests: `core/tests/db_integration.rs`
- Skill protocol: `skillrunner/src/protocol.rs`
- Skill runner: `skillrunner/src/runner.rs`
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
1. Complete remaining M4B scope: cron/timezone trigger specs, manual/idempotent trigger runs, scheduler HA/backpressure controls, and trigger RBAC controls.
2. Complete remaining M6B scope: provider auth strategy docs (Vault/AppRole/K8s, cloud workload identity), TTL caching/version pinning, and rotation-focused integration coverage.
3. Implement M5C payment baseline (`payment.send`) with NWC (NIP-47), spend budgets, and idempotent settlement records.

## New Session Prompt (copy/paste)
```text
Read AGENTS.md and docs/SESSION_HANDOFF.md first, then docs/agent_platform.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/POLICY.md, docs/ROADMAP.md, and recent CHANGELOG entries. Summarize current implemented state vs remaining roadmap, then continue with the next unfinished milestone.
```
