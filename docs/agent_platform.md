# Project Brief: SecureAgnt (Rust Agent Platform)
*(Draft for personal notes + Codex input. Updated with multi-language skills + Skill Protocol v0 spec.)*

## Deployment profiles

SecureAgnt runs as one platform with two operating profiles:

- **solo-lite**
  - single-operator posture, no UI dependency
  - binary + systemd services (`secureagnt-api`, `secureagntd`)
  - SQLite-by-default storage
  - installer-first setup via `scripts/install/secureagnt-solo-lite-installer.sh`
  - tighter defaults and simpler operational blast radius
- **enterprise**
  - containerized stack profile with shared operational services
  - Postgres + broader feature set for multi-tenant/runtime extension
  - richer connector and policy surface for teams and interoperable agents

These profiles share the same core engine, capability model, and policy enforcement. The difference is profile defaults, deployment topology, and enabled surfaces.

## Context / Motivation
Agent platforms show the direction things are going: automated workflows that can actually **do** things (not just chat). The problem is that the “skills/extensions” ecosystem is both:
- **~100% of the power**, and
- **~90% of the security risk** (prompt injection → tool misuse, SSRF, secret exfil, malicious plugins).

This project proposes a re-implementation that **throws out the current “skills” approach** and rebuilds it around:
- **default-deny capabilities**
- **strict isolation boundaries**
- **auditable side effects**
- **tight governance / signing**

The core can be Rust, but **skills can be Rust or Python** safely by making skills **out-of-process RPC services**, not in-process plugins.

Both profiles use the same policy-first skill/action model; `solo-lite` keeps the default runtime envelope smaller while preserving the same guarantees.

---

# Goals
1. **Safe-by-default**: a “fresh install on a VPS” should not be a security foot-gun.
2. **Powerful**: skills/connectors remain the value driver.
3. **Enterprise-ready path**: auditability, policy, tenant isolation, strong execution boundaries.
4. **Open source** core.

## Non-goals (initially)
- Open “skill marketplace” with arbitrary third-party uploads.
- “Run any script/shell command” as a normal capability.
- Broad outbound network access.

---

# Key Thesis
## Skills are not “plugins” — they are **capabilities**
Treat skills as **privileged authority**. Each action needs:
- explicit permission,
- narrow scope,
- audit trail,
- revocation.

---

# Architecture Overview (3 Layers)
## Layer 1: Primitives (small + safe, owned by the platform)
Only a tiny set of guarded primitives exist, implemented in the **platform** (Rust recommended):

- `http_request` (allowlist-only, SSRF hardened)
- `read_object` / `write_object` (scoped storage)
- `send_message` (scoped destinations, allowlisted channels)
- `db_query` (parameterized, least privilege)
- `emit_audit_event` (immutable event log)
- `llm_infer` (optional; with strict input/output logging & redaction controls)

**Important:** Primitives are the *only* way to touch the outside world. Everything else composes on top.

## Layer 2: Connectors (typed wrappers around primitives)
Connectors expose a typed interface:
- `WhiteNoise.sendEncryptedMessage(recipient, ciphertext_ref)` (Marmot protocol over Nostr)
- `Slack.sendMessage(channel_id, text)`
- `GitHub.createIssue(repo, title, body)`
- `Nostr.publishNote(text)`
Connectors:
- declare required capabilities,
- validate inputs,
- never expose raw primitive access to the model.

## Layer 3: Recipes (workflows)
Recipes compose connectors:
- “Summarize transcript → generate show notes → post to Nostr → notify White Noise”
Recipes are:
- versioned
- reviewable
- can require approvals for sensitive actions

### Profile surface guidance

- `solo-lite`
  - keeps the action/connectors set compact and conservative
  - defaults to local-first storage and local-only bootstrap paths
- `enterprise`
  - enables the broader connector and policy surface expected for team use
  - relies on the full containerized operational profile

---

# Multi-language Skills (Rust + Python) without losing security
## The rule
**Skills must be out-of-process.**  
They run as separate processes (or containers/microVMs) and communicate over a strict protocol.

### Why this is good
- Skills can be written in **Rust or Python**
- Platform keeps the real authority (network, secrets, storage)
- Skills can be sandboxed + resource-limited
- Side effects are audited and policy-checked at the platform boundary

### What NOT to do
- Do **not** load Python/Rust “skills” as in-process libraries with direct access to secrets/network/filesystem.
  That collapses your trust boundary.

---

# Capability Model (Default-deny)
Each run receives a capability set:
- Allowed connector methods: `WhiteNoise.sendEncryptedMessage`, `Slack.sendMessage`, `GitHub.createIssue`
- Allowed network destinations: `api.github.com`, `slack.com` (host allowlist)
- Data scopes: `read:podcasts/*`, `write:shownotes/*`
- Rate limits: requests/minute, payload size caps
- Time box: hard deadline per step and per run

**No capability = action denied.**

---

# Isolation Roadmap
## MVP Isolation (fast)
- Skills run as subprocesses under a dedicated unprivileged user
- No secrets in environment variables
- Strict stdin/stdout protocol
- OS-level controls:
  - read-only rootfs where possible
  - no Docker socket mounts
  - outbound firewall default-deny

## Production Isolation
- Container + seccomp/AppArmor + read-only FS + drop all caps
- Per-skill network policy (deny by default)

## Enterprise Isolation Target
- microVMs (Firecracker/Kata) for untrusted third-party skills
- Stronger tenant isolation

---

# Governance: “Limits on what is added (and by whom)”
### Roles
- **Core maintainers**: primitives, runtime, security controls
- **Reviewed connector authors**: vetted + signed connectors/skills only
- **Tenant admins**: allowlist skills/connectors and recipe versions
- **End users**: create recipes using approved skills/connectors

### Supply chain controls (future but planned)
- Skill packages signed by publisher
- Version pinned
- Reproducible builds target
- Tests + declared permissions required

---

# Skill Protocol v0 (Spec)
This spec is designed to be:
- dead simple to implement in Rust or Python
- strict enough to enforce policy at the platform boundary
- compatible with “skills request actions; platform executes them”

## Design Principle
Skills do **not** perform side effects directly.
Skills may **request** actions; the platform decides (policy) and executes (primitives), then returns results.

## Transport
MVP: **JSON messages over stdin/stdout** (one JSON object per line; NDJSON).
Later: gRPC over Unix sockets, or HTTP over localhost, without changing semantics.

## Message Types
### 1) `describe` request/response
Platform -> Skill:
```json
{ "type": "describe", "id": "req-1" }
```

Skill -> Platform:
```json
{
  "type": "describe_result",
  "id": "req-1",
  "skill": {
    "name": "summarize_transcript",
    "version": "0.1.0",
    "description": "Summarize a transcript into markdown sections",
    "inputs_schema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] },
    "outputs_schema": { "type": "object", "properties": { "markdown": { "type": "string" } }, "required": ["markdown"] },
    "requested_capabilities": [
      { "capability": "object.read", "scope": "podcasts/*" },
      { "capability": "object.write", "scope": "shownotes/*" }
    ],
    "action_types": ["object.read", "object.write"]
  }
}
```

### 2) `invoke` request
Platform -> Skill:
```json
{
  "type": "invoke",
  "id": "req-2",
  "context": {
    "tenant_id": "t-123",
    "run_id": "r-456",
    "step_id": "s-789",
    "time_budget_ms": 20000,
    "granted_capabilities": [
      { "capability": "object.read", "scope": "podcasts/*" }
    ]
  },
  "input": { "text": "..." }
}
```

### 3) `invoke_result` response
Skill -> Platform:
```json
{
  "type": "invoke_result",
  "id": "req-2",
  "output": { "markdown": "# Summary\n..." },
  "action_requests": [
    {
      "action_id": "a-1",
      "action_type": "object.write",
      "args": { "path": "shownotes/ep245.md", "content": "# ..." },
      "justification": "Persist generated show notes for this run"
    }
  ]
}
```

## Action Types (v0)
- `http.request` (defer for MVP or hard allowlist)
- `object.read`
- `object.write`
- `message.send`
- `message.receive`
- `db.query`

## Error Model
```json
{ "code": "STRING_ENUM", "message": "human readable", "details": { } }
```

Suggested codes:
- `INVALID_INPUT`, `TIMEOUT`, `SKILL_CRASHED`, `POLICY_DENY`, `CAPABILITY_MISSING`, `RATE_LIMITED`, `INTERNAL_ERROR`

---

# Milestones (Rough Ranges)
## Milestone A: “Safe-ish single-tenant”
API + worker + Postgres state + strict egress + secrets redaction + audit + skill runner  
**Typical:** ~1–3 weeks

## Milestone B: “Production-grade”
Retries/idempotency/timeouts + observability + 5–10 vetted connectors  
**Typical:** ~4–10 weeks total

## Milestone C: “Enterprise lockdown”
Multi-tenancy + policy engine + signing + microVM isolation  
**Typical:** ~3–6 months total
