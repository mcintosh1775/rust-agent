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

## Cashu scaffold (contract + routing scaffold + mock/live transport baseline implemented)
Cashu is an optional second rail target for low-friction agent-to-agent micropayments.

Current status:
- capability + API scope parsing accepts `cashu:*`
- recipe bundle `payments_cashu_v1` grants `payment.send` with `cashu:*`
- worker parses `cashu:<mint_id>` destinations and validates Cashu config guardrails
- worker can execute deterministic mock outcomes when `PAYMENT_CASHU_MOCK_ENABLED=1`
  - supports `pay_invoice`, `make_invoice`, and `get_balance`
  - persists normal executed ledger outcomes (`payment_requests`/`payment_results`) and payment outbox artifacts
- worker can execute live HTTP outcomes when `PAYMENT_CASHU_HTTP_ENABLED=1`
  - endpoint path mapping:
    - `pay_invoice` -> `POST /v1/pay_invoice`
    - `make_invoice` -> `POST /v1/make_invoice`
    - `get_balance` -> `GET /v1/balance`
  - normalizes key settlement fields into ledger payloads:
    - `pay_invoice`: `settlement_status`, `payment_hash`, `payment_preimage`, `fee_msat`
    - `make_invoice`: `invoice`, `payment_hash`, `amount_msat`
    - `get_balance`: `balance_msat`
  - enforces HTTPS by default (set `PAYMENT_CASHU_HTTP_ALLOW_INSECURE=1` only for local/dev)
  - optional auth header/token injection via `PAYMENT_CASHU_AUTH_HEADER` + `PAYMENT_CASHU_AUTH_TOKEN(_REF)`
- default runtime remains fail-closed when both mock and live HTTP modes are disabled

Destination scope family:
- `cashu:<mint_id>`

Current operation surface (shared with NWC path):
- `pay_invoice`
- `make_invoice`
- `get_balance`

Future extension candidates:
- `send_tokens`
- `receive_tokens`

Runtime knobs:
- `PAYMENT_CASHU_ENABLED` (default off)
- `PAYMENT_CASHU_MINT_URIS`
- `PAYMENT_CASHU_MINT_URIS_REF`
- `PAYMENT_CASHU_DEFAULT_MINT`
- `PAYMENT_CASHU_TIMEOUT_MS`
- `PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN`
- `PAYMENT_CASHU_MOCK_ENABLED` (default off; enables deterministic mock execution)
- `PAYMENT_CASHU_MOCK_BALANCE_MSAT` (mock response value for `get_balance`)
- `PAYMENT_CASHU_HTTP_ENABLED` (default off; enables live HTTP execution)
- `PAYMENT_CASHU_HTTP_ALLOW_INSECURE` (default off; allows non-HTTPS only for local/dev)
- `PAYMENT_CASHU_AUTH_HEADER` (default `authorization`)
- `PAYMENT_CASHU_AUTH_TOKEN`
- `PAYMENT_CASHU_AUTH_TOKEN_REF`

## Planned invariants for Cashu implementation
- Reuse the same `payment_requests`/`payment_results` ledger model.
- Reuse existing audit classification for high-risk payment events.
- Never store raw spendable tokens unredacted in audit/event payloads.
- Keep rail credentials and token material in secret references or encrypted storage, never in skill context.
- Preserve fail-closed behavior for unknown mint IDs and unsupported operations.

## Phased implementation targets
1. Contract phase:
- capability parser support for `cashu:*` (implemented)
- API capability normalization support for `cashu:*` (implemented)
- worker request validation + provider routing scaffold (implemented)

2. Execution phase:
- transport baseline implemented for deterministic mock + live HTTP endpoint mapping
- remaining: token redemption/issuance flows with deterministic idempotency behavior
- remaining: deeper integration tests for failure classes and ledger metadata coverage

3. Operations phase:
- rail-specific reconciliation fields in results metadata
- runbook entries for mint outage, key rotation, and replay/recovery workflows

## References
- `docs/ROADMAP.md` (M5C)
- `docs/ADR/ADR-0008-cashu-rail-planning.md`
- `docs/OPERATIONS.md`
- `docs/DEVELOPMENT.md`
