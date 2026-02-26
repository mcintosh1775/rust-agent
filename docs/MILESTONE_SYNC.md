# Milestone Sync Checklist

Use this before release, after each major milestone update, and at every handoff.

## 1) Milestone status sync

- Open `docs/ROADMAP.md` and confirm the milestone heading is current.
- Open `docs/SESSION_HANDOFF.md` top section and confirm `M18A/B/C` state matches roadmap.
- Open `docs/agent_platform.md` and confirm profile framing matches the milestone assumptions.
- Open `docs/ARCHITECTURE.md` and confirm deployment profile split matches `agent_platform.md`.
- Open `CHANGELOG.md` and confirm latest in `Unreleased` includes the release-facing distribution direction.

## 2) Quick sanity checks for text consistency

- Confirm all four docs mention the same two profiles:
  - `solo-lite`
  - `enterprise`
- Confirm the `M19` knowledge-retrieval roadmap entries are aligned for:
  - MCP transport approach (`rmcp`) and
  - QMD-backed local retrieval scope and policy constraints.
- Confirm the following phrasing is still aligned:
  - solo-lite: installer-first, systemd/services, SQLite defaults
  - enterprise: containerized stack, Postgres profile, broader interoperability surface
- Confirm the active milestone is the same across docs:
  - `M18` should remain the only active installer distribution milestone unless status changes.

## 3) Pre-release freeze check

- Before tagging:
  - update `docs/ROADMAP.md` (M18 phase state and scope)
  - update `docs/SESSION_HANDOFF.md` (live phase state)
  - update `docs/agent_platform.md` / `docs/ARCHITECTURE.md` only if the underlying profile model changed
  - update `CHANGELOG.md` `Unreleased` entry for release-facing installer behavior

## 4) After release upload

- Post-tag, keep all four documents aligned with the exact same:
  - profile naming
  - profile behavior split
  - milestone status labels (`Planned`, `Active`, `Completed`, etc.)

If anything is out of sync, update both:
- `docs/ROADMAP.md`
- `docs/SESSION_HANDOFF.md`
first, then patch related architecture/platform docs if needed.
