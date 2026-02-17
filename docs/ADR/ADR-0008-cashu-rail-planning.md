# ADR-0008: Cashu payment rail planning scaffold

## Status
Accepted

## Context
SecureAgnt currently executes `payment.send` through an NWC-first (NIP-47) connector path.
That baseline is intentionally conservative and works well for invoice-based Lightning payment flows.

For agent-to-agent micropayments, we also want an ecash-native option with strong UX and low-latency settlement.
Cashu is a strong candidate, but introducing a second rail changes policy scope semantics, custody boundaries,
telemetry, and reconciliation requirements.

## Decision
Adopt a phased Cashu planning scaffold now, without enabling runtime execution yet.

1. Keep NWC as the only active payment execution rail for now.
2. Reserve Cashu as an optional second rail under `payment.send` in a future milestone.
3. Document the planned contract now so later implementation stays deterministic:
- destination scope family: `cashu:<mint_id>` (planned)
- dedicated runtime flags and secret references (planned)
- payment ledger + audit invariants reused across NWC and Cashu
4. Keep Cashu optional for all deployment profiles; never require it for baseline operation.

## Consequences
- Current production behavior does not change: only `nwc:*` destinations are accepted.
- Planning artifacts now exist for contributors to implement Cashu without reopening architecture decisions.
- Security and ops teams can pre-review custody/redaction/approval expectations before code ships.
- Future implementation work should include capability parser updates, rail-specific validation,
  and integration tests for both allow/deny and reconciliation behavior.
