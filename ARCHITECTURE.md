# ARCHITECTURE

This document defines the initial architecture for the **Skills Done Right** agent platform: a capability-secured workflow runtime where **skills request actions** and the **platform executes them** under policy, with full auditability.

> Design priority: **security boundaries and governance** over feature breadth.

---

## 1. Core Principles

1. **Default-deny capabilities**
2. **Authority stays in the platform**
3. **Skills are out-of-process**
4. **All side effects are policy-checked and audited**
5. **Small primitives, typed connectors, composable recipes**

---

## 2. Components

- **API Service (control plane)**: auth, policy checks, create runs, status.
- **Worker Service (data plane)**: execute runs/steps, invoke skills, execute approved actions.
- **Core Library**: shared types, capability model, policy enforcement.
- **Skill Runner**: spawn skills, enforce timeouts/resource limits, protocol I/O.
- **Persistence**: shared Postgres cluster per environment with one standardized app schema for runs/steps/audit; optional object store for blobs.

Only `api` and `worker` connect to Postgres directly. Agents/skills interact through platform APIs/protocols.

---

## 3. Three Layers

### Primitives (platform-owned)
- `object.read` / `object.write`
- `message.send`
- `db.query` (prefer registered queries)
- `http.request` (defer or strict allowlist)
- `emit_audit_event`

### Connectors (typed wrappers)
- White Noise (Nostr/Marmot), Slack, GitHub, Nostr, RSS, etc. (built-in first)

### Recipes (workflows)
Versioned, reviewable orchestration of skills + connectors.

---

## 4. Skill Execution
- Out-of-process skills, NDJSON protocol v0.
- Skills return `action_requests`; platform approves/denies and executes.

See `docs/agent_platform.md` for protocol details.

---

## 5. MVP Definition (2-week target)
A vertical slice:
- Create run (API)
- Execute 1–2 steps (Worker)
- Invoke one compute-only skill
- Execute `object.write` (and optionally `message.send`)

Defer: general http, UI, multi-tenancy, marketplace, microVMs.
