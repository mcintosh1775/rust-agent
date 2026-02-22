# ADR-0010: Workspace-based crate version alignment

## Status
Accepted

## Context
The repository now builds multiple crates (`api`, `worker`, `core`, `skillrunner`, `agntctl`) that were previously each declaring independent package versions.

This split versioning causes confusing release metadata, even though releases are already tracked as git tags (`vX.Y.Z`), and can allow drift where one crate is unintentionally published with stale version metadata.

## Decision
Use one source of version truth for crate package versions:

- Set `[workspace.package].version` as the canonical release version.
- Configure each member package with `version.workspace = true`.
- Enforce alignment in CI/gates with a drift check (`make verify-workspace-versions`).

## Consequences
- Release metadata is coherent across all crates (`core`, `api`, `worker`, `skillrunner`, `agntctl`).
- CI catches accidental version drift before verification/release gates.
- Tagging and changelog release entries should remain the primary external release signal; workspace version should be updated when shipping a new release candidate.

## Compliance impact
- Reduces release-process ambiguity without changing runtime behavior or permissions model.
- Keeps non-functional release governance auditable via existing `CHANGELOG.md` and release tags.

