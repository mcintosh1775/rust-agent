# ADR-0009: Agent context files profile and precedence model

## Status
Accepted

## Context
SecureAgnt needs a portable way to express agent identity, user preferences, memory, and proactive behavior across sessions and tooling.
The project already uses `AGENTS.md` conventions and has strong runtime policy boundaries.

Without an explicit profile, different sessions/tools may interpret context files inconsistently, causing non-deterministic behavior.

## Decision
Adopt a formal agent-files profile with strict precedence and mutability rules.

1. Canonical files:
- `AGENTS.md`
- `TOOLS.md`
- `IDENTITY.md`
- `SOUL.md`
- `USER.md`
- `MEMORY.md`
- `HEARTBEAT.md`
- `skills/**/SKILL.md`
- `memory/YYYY-MM-DD.md`
- `sessions/*.jsonl`

2. Precedence:
- runtime/policy enforcement
- `AGENTS.md` + `TOOLS.md`
- `IDENTITY.md` + `SOUL.md`
- `USER.md`
- `MEMORY.md`
- raw logs (`memory/*`, `sessions/*`)

3. Mutability:
- policy/identity files are human/admin-controlled.
- `USER.md` and `HEARTBEAT.md` are human-primary with agent suggestions.
- `MEMORY.md` and raw logs are agent-managed under verification/audit guardrails.

4. Runtime semantics:
- LLMs reason and plan.
- Skills execute deterministic capabilities.
- Side effects always pass through action-request + policy gates.

5. Heartbeat semantics:
- `HEARTBEAT.md` expresses intent.
- Proactive work is materialized through governed trigger APIs, not direct bypass paths.

Reference profile document:
- `docs/AGENT_FILES.md`

## Consequences
- Session behavior is more deterministic and portable.
- Context conflicts resolve predictably with security-first ordering.
- Agent persona/prefs can evolve without weakening capability policy.
- Proactive behavior remains auditable and policy-governed.

Follow-on work should implement:
- typed loader/validator for profile files
- checksum/provenance recording for loaded context
- audit visibility of effective context state (with redaction)
