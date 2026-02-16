# SECURITY

## Model
- Default-deny capabilities
- Authority stays in the platform
- Out-of-process skills
- Audited side effects

## Forbidden patterns
- Passing secrets to skills via env/files/context
- Broad `http.request` without allowlist + SSRF hardening
- In-process plugin loading
- Arbitrary shell command execution as a normal capability
- Mounting Docker socket
- Running workers/skills as root

If local host execution is required, use the constrained sandbox model in `docs/ADR/ADR-0006-sandboxed-local-exec-primitive.md`.

## Deployment minimums (prod)
- TLS reverse proxy in front of API
- Worker in private network
- Outbound egress deny-by-default from workers/skill hosts
- Secrets from Vault/KMS; never exposed to skills
- Structured logs with redaction

## Reporting
Until a private channel exists: open a GitHub issue with prefix `SECURITY:` (minimal detail).
