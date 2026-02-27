# Workflow Lanes

This document defines fixed work lanes for session planning and review.

## Why lanes exist
- Keep each session focused on one operational trajectory.
- Prevent context churn from crossing many unrelated topics in one pass.
- Provide a repeatable handoff boundary between humans and Codex sessions.

## Lanes

### Lane `release`
Scope:
- release packaging
- artifact checks
- startup/LLM smoke checks
- release-gate / upload / versioning

Completion criteria:
- all required checks in `docs/PLAYBOOKS.md#release-readiness` are complete and captured.
- handoff log entry records all expected artifacts and command outputs.

### Lane `operations`
Scope:
- installer behavior
- systemd/container rollout
- smoke scaffolding and operational scripts

Completion criteria:
- runbooks are updated for behavior changes.
- run commands have at least one reproducible validation step.

### Lane `security`
Scope:
- policy/capability changes
- credential and secret-handling adjustments
- guardrail or denial-path fixes

Completion criteria:
- tests cover denial and failure behavior.
- policy defaults and docs align with implementation.

### Lane `context-control`
Scope:
- session handoff process
- task-queue hygiene
- documentation debt reduction

Completion criteria:
- `docs/SESSION_HANDOFF.md` has current session summary.
- latest entry is appended to `docs/task-ledger.md`.

## Session lane rules
1. At least one lane is active for each session.
2. Each lane has one primary objective and a bounded outcome.
3. Avoid taking tasks in another lane unless explicitly handoff with rationale.
4. Close each lane handoff with a completed `make handoff` command (or manual equivalent).

## Lane handoff output
Every active lane should leave:
- 2-4 bullet completion notes in `docs/SESSION_HANDOFF.md`.
- one structured entry in `docs/task-ledger.md`.
