# ADR-0006: Sandboxed local execution primitive with strict allowlists

## Status
Accepted

## Context
Some enterprise workflows may require limited host-local actions (for example reading/writing specific working directories or invoking narrow utility commands).

Allowing arbitrary shell execution would collapse security boundaries and violate the platform's default-deny capability model.

## Decision
If local execution is introduced, it must be implemented as a constrained platform primitive, not general shell access.

Required controls:
- No arbitrary `sh`/`bash` command strings.
- Only allowlisted command templates identified by stable IDs.
- Typed/validated argument schemas per template.
- Filesystem access constrained to allowlisted read/write path prefixes.
- Hard runtime limits per invocation (timeout, memory, output bytes, process count).
- Unprivileged runtime identity and isolation controls.
- Full audit trail for request, allow/deny decision, execution result, and termination cause.

Capability model requirements:
- Grants must be explicit and scoped (for example `system.exec:template_id`, `fs.read:prefix`, `fs.write:prefix`).
- Missing grant or out-of-scope args/paths must fail closed.

## Consequences
- Enables narrowly-scoped local automation without introducing general remote-code-execution behavior.
- Keeps authority at the platform boundary with policy/audit enforcement.
- Adds implementation complexity (template registry, argument validation, runtime confinement).
- Must be delivered with integration tests for deny-by-default and sandbox escape resistance.
