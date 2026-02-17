# Payments (NWC Baseline + Cashu Scaffold)

This document defines payment rail behavior for `payment.send`.

## Current runtime baseline (implemented)
- Active rail: Nostr Wallet Connect (NWC / NIP-47)
- Allowed destination scope: `nwc:<wallet_id>`
- Supported operations:
  - `pay_invoice`
  - `make_invoice`
  - `get_balance`
- Safety controls:
  - idempotency key required
  - run/tenant/agent spend caps
  - approval threshold gate
  - fail-closed wallet routing

## Cashu scaffold (planning only, not active)
Cashu is planned as an optional second rail for low-friction agent-to-agent micropayments.

Current status:
- no runtime Cashu execution path exists yet
- policy parser currently rejects `cashu:*` scopes
- worker currently rejects non-`nwc` payment destinations

Planned destination scope family:
- `cashu:<mint_id>`

Planned operation set:
- `send_tokens`
- `receive_tokens`
- `get_balance`

Planned runtime knobs (reserved; no current runtime effect):
- `PAYMENT_CASHU_ENABLED`
- `PAYMENT_CASHU_MINT_URIS`
- `PAYMENT_CASHU_MINT_URIS_REF`
- `PAYMENT_CASHU_DEFAULT_MINT`
- `PAYMENT_CASHU_TIMEOUT_MS`
- `PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN`

## Planned invariants for Cashu implementation
- Reuse the same `payment_requests`/`payment_results` ledger model.
- Reuse existing audit classification for high-risk payment events.
- Never store raw spendable tokens unredacted in audit/event payloads.
- Keep rail credentials and token material in secret references or encrypted storage, never in skill context.
- Preserve fail-closed behavior for unknown mint IDs and unsupported operations.

## Phased implementation targets
1. Contract phase:
- capability parser support for `cashu:*`
- API capability normalization support for `cashu:*`
- worker request validation + provider routing scaffold

2. Execution phase:
- Cashu transport adapter and mint allowlist controls
- token redemption/issuance flows with deterministic idempotency behavior
- integration tests for allow/deny, failure modes, and ledger consistency

3. Operations phase:
- rail-specific reconciliation fields in results metadata
- runbook entries for mint outage, key rotation, and replay/recovery workflows

## References
- `docs/ROADMAP.md` (M5C)
- `docs/ADR/ADR-0008-cashu-rail-planning.md`
- `docs/OPERATIONS.md`
- `docs/DEVELOPMENT.md`
