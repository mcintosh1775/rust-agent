# Skills Done Right (Agent Platform)

A capability-secured, Nostr-first agent runtime where **skills request actions** and the **platform executes them** under policy, with full auditability.

## What this is
- Workflow runtime (runs/steps) with durable state in a shared Postgres service per environment
- Default-deny capability model for every side-effect
- Out-of-process skills (Rust or Python) via a strict protocol
- Typed connectors built on a minimal primitive set
- White Noise chat as a first-class messaging target (Marmot protocol over Nostr)
- Recipes that compose skills/connectors

## What this is NOT
- Not a “run arbitrary code” platform
- Not an open marketplace for unreviewed third-party skills (initially)
- Not broad outbound internet access by default

## MVP
Vertical slice only. See `docs/agent_platform.md`.

## Docs
- `docs/SESSION_HANDOFF.md`
- `docs/DEVELOPMENT.md`
- `docs/OPERATIONS.md`
- `docs/agent_platform.md`
- `docs/ARCHITECTURE.md`
- `docs/ARCHITECTURE_BRIEF.md`
- `docs/SECURITY.md`
- `docs/THREAT_MODEL.md`
- `docs/POLICY.md`
- `docs/API.md`
- `docs/RUNBOOK.md`
- `docs/ROADMAP.md`
- `docs/ADR/`
