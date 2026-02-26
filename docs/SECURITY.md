# SECURITY

## Model
- Default-deny capabilities
- Authority stays in the platform
- Out-of-process skills
- Audited side effects
- Redacted audit/action payload persistence for sensitive fields and token patterns

## Profile model

- **solo-lite profile** defaults
  - host-installed systemd services
  - SQLite-first data persistence
  - minimal externally exposed surface (no web UI requirement)
  - policy-first runtime with reduced default feature set
- **enterprise profile** defaults
  - containerized stack deployments
  - stronger network segmentation and optional enterprise-grade hardening controls
  - full policy and feature surface for team/interop scenarios

Both profiles share policy semantics; only deployment and enabled surface differ.

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
- `message.send` and `message.receive` support optional provider-specific allowlists and fail closed when targets/sources are not allowed.
- Secret references are resolved through backend adapters with fail-closed cloud gates, optional short TTL caching, and version-pin support for deterministic rotation behavior.
- `local.exec` is template-based only (no arbitrary shell string execution), with path-root allowlists and per-invocation limits.
- `llm.infer` supports local-first routing with separate local/remote scope grants to prevent unintended remote token spend.

## Reporting
Until a private channel exists: open a GitHub issue with prefix `SECURITY:` (minimal detail).

## Dependency checks
To run dependency CVE checks locally:

```bash
make cargo-audit
```

This target is network-aware and will skip when crates.io is unreachable, which is common in isolated CI contexts.
Set `CARGO_AUDIT_REQUIRE_NETWORK=1` if you want the command to fail when network access is unavailable.
