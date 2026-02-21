# Agent Files Profile (SecureAgnt)

## Purpose
Define a portable, `AGENTS.md`-compatible file profile for agent identity, user preferences, memory, and proactive behavior.

This profile keeps two properties explicit:
- Skills execute side effects under policy.
- LLMs reason and plan when a skill cannot directly solve the task.

## Scope
This is a contract for workspace-level agent files and load behavior.
It does not bypass runtime capability policy in `docs/POLICY.md`.

## Canonical File Set
- `AGENTS.md`
  - Operational rules, hard constraints, workspace conventions.
- `TOOLS.md`
  - Tool allow/deny and invocation constraints.
- `IDENTITY.md`
  - Stable name/role/scope metadata for the agent.
- `SOUL.md`
  - Personality, communication boundaries, value posture.
- `USER.md`
  - Human-specific style and collaboration preferences.
- `MEMORY.md`
  - Verified long-term facts.
- `HEARTBEAT.md`
  - Proactive behavior intents (checklists/reminders/schedules).
- `BOOTSTRAP.md`
  - First-run onboarding prompts for one-off/solo setups.
- `skills/**/SKILL.md`
  - Skill-level implementation contracts.
- `memory/YYYY-MM-DD.md`
  - Raw daily memory logs (working notes).
- `sessions/*.jsonl`
  - Session transcripts/audit trail.

## Precedence and Conflict Resolution
Higher items override lower items.

1. Platform/runtime policy and enforcement (`core` policy, API role gates, worker guards)
2. `AGENTS.md` and `TOOLS.md`
3. `IDENTITY.md` and `SOUL.md`
4. `USER.md`
5. `MEMORY.md`
6. `memory/*.md` and `sessions/*.jsonl` (reference context only)

Additional rules:
- Nearest-path `AGENTS.md` wins over parent directories.
- If two files at same precedence conflict, choose the more restrictive rule.
- Persona files never weaken safety policy.
- Memory files never authorize side effects.

## Mutability Model
- Human/admin-owned (agent must not edit):
  - `AGENTS.md`, `TOOLS.md`, `IDENTITY.md`, `SOUL.md`
- Human-primary, agent-suggested edits only:
  - `USER.md`, `HEARTBEAT.md`, `BOOTSTRAP.md`
- Agent-managed with guardrails:
  - `MEMORY.md` (only verified facts promoted from logs)
  - `memory/YYYY-MM-DD.md` (append/update allowed)
  - `sessions/*.jsonl` (append-only transcript/audit artifacts)

## LLM and Skill Runtime Semantics
- LLMs are used for:
  - planning
  - decomposition
  - ambiguity resolution
  - fallback reasoning when no skill directly solves a step
- Skills are used for:
  - deterministic execution
  - adapter-specific side effects
  - validated action argument shaping
- Side effects must still flow through action requests and policy checks.
- LLM reasoning output must not directly execute privileged operations.

## Heartbeat Semantics
`HEARTBEAT.md` defines intent, not direct privileged execution.

Recommended flow:
1. Parse heartbeat intents into typed schedule candidates.
2. Validate against policy/role constraints.
3. Materialize as governed triggers (`/v1/triggers*`) with audit provenance.
4. Require explicit approval for new/modified high-risk proactive actions.

## Security and Audit Requirements
- File-derived context should be loaded with provenance (path + checksum + timestamp).
- Mutations to memory/heartbeat artifacts should be auditable.
- Session/raw memory logs are non-authoritative until promoted/verified.
- Secret material must not be stored in profile files.

## Implementation Guidance
Runtime baseline now implemented in worker:
- typed loader + validator in `core/src/agent_context.rs`
- worker-side load/inject path in `worker/src/lib.rs`
- audit events:
  - `agent.context.loaded`
  - `agent.context.not_found`
  - `agent.context.error`

Current worker config controls:
- `WORKER_AGENT_CONTEXT_ENABLED`
- `WORKER_AGENT_CONTEXT_REQUIRED`
- `WORKER_AGENT_CONTEXT_ROOT`
- `WORKER_AGENT_CONTEXT_REQUIRED_FILES`
- `WORKER_AGENT_CONTEXT_MAX_FILE_BYTES`
- `WORKER_AGENT_CONTEXT_MAX_TOTAL_BYTES`
- `WORKER_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR`

Context directory resolution order:
1. `<WORKER_AGENT_CONTEXT_ROOT>/<tenant_id>/<agent_id>/`
2. `<WORKER_AGENT_CONTEXT_ROOT>/<agent_id>/`

Worker injects loaded profile data into skill input under:
- `agent_context`

Control-plane/operator baseline now implemented in API:
- context inspection endpoint:
  - `GET /v1/agents/{agent_id}/context`
  - returns metadata/checksums/mutability classification without file contents
- heartbeat compile endpoint:
  - `POST /v1/agents/{agent_id}/heartbeat/compile`
  - compiles heartbeat intents into trigger candidates (no side effects)
- heartbeat materialization endpoint:
  - `POST /v1/agents/{agent_id}/heartbeat/materialize`
  - supports plan-only previews (`apply=false`) and governed trigger creation (`apply=true`) with explicit approval confirmation
- context mutation endpoint (disabled by default):
  - `POST /v1/agents/{agent_id}/context`
  - enabled only with `API_AGENT_CONTEXT_MUTATION_ENABLED=1`
  - enforces mutability boundaries:
    - immutable: denied
    - human-primary: owner only
    - agent-managed: owner/operator
    - `sessions/*.jsonl` append-only
- bootstrap endpoints:
  - `GET /v1/agents/{agent_id}/bootstrap`
    - inspect bootstrap status (`pending`, `completed`, `not_configured`, or `disabled`)
  - `POST /v1/agents/{agent_id}/bootstrap/complete`
    - owner-only completion path with explicit `x-user-id` attribution
    - writes optional initial profile content (`IDENTITY.md`, `SOUL.md`, `USER.md`, `HEARTBEAT.md`)
    - appends completion event to `sessions/bootstrap.status.jsonl`
