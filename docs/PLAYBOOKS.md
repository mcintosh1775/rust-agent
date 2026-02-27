# Playbooks

Operational and release procedures are collected here to reduce decision drift.

## Session Handoff Playbook

1. Choose one primary lane (`release`, `operations`, `security`, `context-control`).
2. Complete the lane objective.
3. Collect evidence (command logs, run IDs, artifacts).
4. Update `docs/SESSION_HANDOFF.md`:
   - current lane
   - completed work
   - next action
5. Append ledger entry:

```bash
HANDOFF_LANE=context-control \
HANDOFF_GOAL="reduce session context churn" \
HANDOFF_COMPLETED="Added structured lane/playbook workflow and recording script." \
HANDOFF_RISKS="Need consistent use at every session boundary." \
HANDOFF_NEXT="Continue release lane work; run smoke evidence capture." \
make handoff
```

## Release Readiness Playbook

For every release lane cycle:

1. Prepare release artifacts and manifest parity.
2. Record handoff evidence:
   - `make release-distribution-check TAG=<tag>`
   - `make release-smoke-check TAG=<tag> DB=<runtime db>`
   - `make release-llm-smoke` when remote inference is enabled.
3. Run `make release-gate` as required by release rules.
4. Create/update changelog entries for user-visible behavior.
5. Capture final status in `docs/SESSION_HANDOFF.md` and `docs/task-ledger.md`.

## Incident / unexpected behavior playbook

1. Pause the active lane and record `context-control` notes immediately.
2. Capture failing log snippets and DB query output.
3. Move to `security` or `operations` lane if fix scope changes.
4. Re-run minimal validation for the affected profile.
5. Close with `make handoff` so next session picks up only the active facts.
