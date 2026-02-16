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
- Mounting Docker socket
- Running workers/skills as root

## Deployment minimums (prod)
- TLS reverse proxy in front of API
- Worker in private network
- Outbound egress deny-by-default from workers/skill hosts
- Secrets from Vault/KMS; never exposed to skills
- Structured logs with redaction

## Reporting
Until a private channel exists: open a GitHub issue with prefix `SECURITY:` (minimal detail).
