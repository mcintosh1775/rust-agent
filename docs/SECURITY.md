# SECURITY

## Model
- Default-deny capabilities
- Authority stays in the platform
- Out-of-process skills
- Audited side effects
- Redacted audit/action payload persistence for sensitive fields and token patterns

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

## Runtime hardening notes
- Skill subprocesses are launched with `env_clear` and explicit env allowlists only.
- Worker enforces timeout/output caps for skill execution.
- Sensitive values are redacted before writing action request/result and audit payloads.
- `message.send` supports optional provider-specific destination allowlists and fails closed for non-allowlisted targets when configured.
- Secret references are resolved through backend adapters with fail-closed cloud gates, optional short TTL caching, and version-pin support for deterministic rotation behavior.
- `local.exec` is template-based only (no arbitrary shell string execution), with path-root allowlists and per-invocation limits.
- `llm.infer` supports local-first routing with separate local/remote scope grants to prevent unintended remote token spend.

## Reporting
Until a private channel exists: open a GitHub issue with prefix `SECURITY:` (minimal detail).
