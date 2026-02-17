# ADR-0007: Pluggable Nostr signer modes (local key and NIP-46)

## Status
Accepted

## Context
SecureAgnt is Nostr-first and needs signer-backed agent identity for connector/auth flows.  
The project must serve both:
- self-hosted setups with minimal infrastructure, and
- enterprise deployments that keep private keys out of worker pods.

Making NIP-46 mandatory would block simple deployments. Making local hot keys the only path weakens hardened environments.

## Decision
Adopt pluggable signer modes:

1. `local_key` (default)
- Worker derives the public key from a local secret (`NOSTR_SECRET_KEY` or `NOSTR_SECRET_KEY_FILE`).
- Intended for self-hosted/smaller deployments.
- Secret file permissions must be owner-only (`0600`) when file-based key loading is used.

2. `nip46_signer` (optional)
- Worker uses a remote signer identity via NIP-46 configuration.
- Requires `NOSTR_NIP46_BUNKER_URI`; public key can be explicit (`NOSTR_NIP46_PUBLIC_KEY`) or extracted from URI.
- Intended for enterprise/hardened environments where worker hosts should not hold private keys.

The worker must validate signer configuration at startup and log signer mode/public key when available.

## Consequences
- Nostr signing becomes an explicit runtime concern, not a hidden connector detail.
- Self-hosted setups can run without NIP-46.
- Enterprise setups can enforce key isolation with NIP-46.
- White Noise relay publish can use either local signing or NIP-46 remote signing without changing recipe/action payloads.
- Future connector/auth work should consume a signer-provider abstraction rather than hardcoding key handling.
