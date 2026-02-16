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
  - M5A messaging baseline with `message.send` execution, local connector outbox persistence, and White Noise relay publish support (`NOSTR_RELAYS`)
  - M5B signer baseline with pluggable Nostr identity modes (`local_key` default, optional `nip46_signer`) and NIP-46-backed relay publish signing

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
- Reference Python skill: `skills/python/summarize_transcript/main.py`

## High-Priority Next Steps
1. Add structured redaction for logs/audit payloads.
2. Add API-managed capability bundle presets per recipe/role (instead of caller-defined free-form requests).
3. Add Slack delivery transport execution path behind policy and destination allowlists.

## New Session Prompt (copy/paste)
```text
Read AGENTS.md and docs/SESSION_HANDOFF.md first, then docs/agent_platform.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/POLICY.md, docs/ROADMAP.md, and recent CHANGELOG entries. Summarize current implemented state vs remaining roadmap, then continue with the next unfinished milestone.
```
