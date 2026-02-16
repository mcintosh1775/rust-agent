# AGENTS (Codex Instructions)

Before making changes, read:
1) `docs/agent_platform.md`
2) `ARCHITECTURE.md`
3) `SECURITY.md`
4) `docs/POLICY.md`

## Non-negotiables
- Skills are **out-of-process** (no in-process plugin loading).
- Default-deny capabilities gate every privileged side effect.
- Skills never get raw network, secrets, or filesystem by default.
- Skills **request actions**; platform approves/denies and executes.
- MVP: no general `http.request` (or single hardcoded allowlisted host only).
- Never mount Docker socket; no privileged containers.

## MVP scope
Prefer a thin vertical slice. If scope expands (UI, multi-tenancy, marketplace, microVMs), add an ADR first.

## Quality gates
- Add tests for capability denials, auditing, timeouts/crash handling.
- Avoid new deps unless essential; justify them.

## Expected layout (target)
- `api/`, `worker/`, `core/`, `skillrunner/`, `connectors/`, `skills/`, `docs/`

## Build/test
Create/maintain a minimal `Makefile` for:
- `make fmt` → `cargo fmt`
- `make lint` → `cargo clippy`
- `make test` → `cargo test`
- `make api` / `make worker`
