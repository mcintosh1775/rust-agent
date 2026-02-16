# ARCHITECTURE

This document defines the initial architecture for the **Skills Done Right** agent platform: a capability-secured workflow runtime where **skills request actions** and the **platform executes them** under policy, with full auditability.

> Design priority: **security boundaries and governance** over feature breadth.

---

## 1. Core Principles

1. **Default-deny capabilities**
   - Every external side effect requires an explicit, scoped grant.
2. **Authority stays in the platform**
   - Skills do not get raw network, secrets, or filesystem access by default.
3. **Skills are out-of-process**
   - Skills run as separate processes (and later containers/microVMs), speaking a strict protocol.
4. **All side effects are policy-checked and audited**
   - Allowed/denied decisions and results are recorded.
5. **Small primitives, typed connectors, composable recipes**
   - Minimize the trusted surface area.

---

## 2. System Overview

### Components
- **API Service** (control plane)
  - AuthN/AuthZ, policy checks, creates runs, returns run status.
- **Worker Service** (data plane)
  - Executes steps, invokes skills, executes approved action requests.
- **Core Library** (shared)
  - Types, capability model, policy enforcement, action dispatcher interfaces.
- **Skill Runner**
  - Spawns skills, enforces timeouts/resource limits, handles protocol I/O.
- **Persistence**
  - Shared Postgres cluster per environment (`dev`/`staging`/`prod`) for state + audit.
  - One standardized app schema (e.g., `aegis`) managed by migrations; not schema-per-agent.
  - Optional object store for blobs/artifacts.

Data access boundary:
- Only `api` and `worker` services connect directly to Postgres.
- Agents and skills never connect to Postgres directly; they use platform APIs/protocols.

### Data Flow (happy path)
1. Client submits a **Recipe Run** to API.
2. API validates input + auth + policy, creates a **Run** and initial **Steps**.
3. Worker picks up the Run and executes steps:
   - invokes a Skill (compute) via Skill Runner
   - receives `action_requests`
   - platform evaluates each request against capabilities/policy
   - platform executes allowed actions using primitives/connectors
   - results recorded + fed into subsequent steps
4. Run completes with outputs + audit trail.

---

## 3. Three Layers

### Layer 1: Primitives (platform-owned)
Minimal privileged operations implemented inside the platform:
- `http.request` (later; allowlist + SSRF hardening)
- `object.read` / `object.write` (scoped paths, size caps)
- `message.send` (scoped providers + destinations)
- `db.query` (registered/prepared queries, not raw SQL)
- `emit_audit_event`

**MVP guidance:** start without general `http.request` or restrict it to a single allowlisted host to reduce risk.

### Layer 2: Connectors (typed wrappers)
Connectors provide typed interfaces over primitives:
- `WhiteNoise.sendEncryptedMessage(recipient, ciphertext_ref)` (Marmot protocol on Nostr)
- `Slack.sendMessage(channel_id, text)`
- `GitHub.createIssue(repo, title, body)`
- `Nostr.publishNote(text)`
Connectors:
- validate inputs
- declare required capabilities
- never expose raw primitive calls to skills/LLMs directly

### Layer 3: Recipes (workflows)
Recipes orchestrate skills and connectors into useful automations.
- versioned
- reviewable
- optionally require approval gates for sensitive steps

---

## 4. Skill Execution Model

### Skill boundary
- Skills run **out-of-process**.
- Skills are not allowed to directly perform privileged side effects.
- Skills return `action_requests` which the platform may approve/deny.

### Transport v0
- NDJSON over stdin/stdout
- One JSON object per line
- Request/response correlation via `id`

### Skill Protocol v0
See `docs/agent_platform.md` for full spec.
Key messages:
- `describe` / `describe_result`
- `invoke` / `invoke_result`
- optional `action_result` callback

---

## 5. Capability & Policy Model

### Capability token (conceptual)
A Run is executed with a set of granted capabilities:
- connector/method allowlist (e.g., `Slack.sendMessage`)
- network destination allowlist (e.g., `slack.com`)
- data path scopes (e.g., `write:shownotes/*`)
- rate limits and quotas
- time budgets

### Enforcement points
- API: validates caller is allowed to start a run with requested capabilities
- Worker: validates each action request before execution
- Primitive: performs last-mile checks (e.g., SSRF protections)

### Approval gates (optional, recommended)
For irreversible actions:
- posting externally
- emailing outside org
- payments / funds movement
- destructive operations (delete, revoke)

---

## 6. Persistence & Data Model (Postgres)

### Tables (initial)
- `runs`
  - id, tenant_id, recipe_id, status, created_at, started_at, finished_at
- `steps`
  - id, run_id, name, status, input_json, output_json, started_at, finished_at
- `artifacts`
  - id, run_id, path, content_type, size, checksum, storage_ref
- `action_requests`
  - id, step_id, action_type, args_json, justification, status
- `action_results`
  - id, action_request_id, status, result_json, error_json, executed_at
- `audit_events`
  - id, run_id, step_id, actor, event_type, payload_json, created_at

### Artifact storage
- MVP: store small artifacts inline (text) or in a local directory.
- Later: S3-compatible object store with per-tenant prefixes.

---

## 7. Security Posture (MVP → Enterprise)

### MVP “safe-ish single-tenant”
- No third-party skills
- No general outbound internet (deny-by-default egress)
- Secrets never passed to skills; only to connector implementations as needed
- Skills run as unprivileged subprocesses with timeouts

### Production hardening
- container isolation + seccomp/AppArmor + read-only filesystem
- per-skill network policies
- OpenTelemetry traces + metrics + structured logs with redaction

### Enterprise lockdown
- multi-tenancy + per-tenant encryption keys + quotas
- policy engine (OPA-style) and signed connectors/skills
- microVM isolation for untrusted code (Firecracker/Kata)
- immutable audit log export to SIEM

---

## 8. Deployment Topology

### Minimal deployment
- `api` service (stateless)
- `worker` service (stateless, scalable)
- Postgres (shared per environment)
- optional object store

### Network
- API behind reverse proxy with TLS
- private network access to Postgres from `api`/`worker` only
- strict outbound egress policies from worker/skill runtime hosts

---

## 9. Observability

- Structured logs (JSON), with secret redaction
- Metrics:
  - run/step latency
  - action approvals/denials
  - skill failures/timeouts
  - queue depth
- Tracing:
  - per-run trace with step spans
  - connector/action spans

---

## 10. MVP Definition (2-week target)

A realistic MVP is a **vertical slice**:
- API creates runs
- Worker executes a recipe with 1–2 steps
- Skill runner invokes one **compute-only** skill (Python ok)
- Platform executes 1 privileged action type:
  - `object.write` (store markdown)
  - `message.send` (White Noise notify; Slack optional for enterprise)

**Defer**:
- general `http.request`
- multi-tenancy
- marketplace/signing
- microVMs
- UI

---

## 11. Codebase Layout (suggested)

- `api/` (Axum)
- `worker/`
- `core/`
  - types, capabilities, policy checks, action dispatcher interfaces
- `skillrunner/`
  - subprocess runner, protocol codecs, resource limits
- `connectors/`
  - `slack/`, `github/`, `nostr/` (initially built-in)
- `skills/`
  - `python/` (reference skills)
  - `rust/` (reference skills)
- `docs/`
  - `agent_platform.md`
  - `docs/ARCHITECTURE.md` (this file)

---

## 12. Near-term Implementation Notes

- Keep the trusted computing base **small**: primitives + policy + action dispatcher.
- Start with **compute-only** skills to prove the protocol.
- Add “scary primitives” (especially network) only after you have:
  - capability enforcement
  - auditing
  - egress controls
  - SSRF hardening plan
