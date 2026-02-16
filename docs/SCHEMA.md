# SCHEMA (MVP)

This document describes the MVP Postgres schema. Keep it minimal and stable; avoid premature normalization.

## Conventions
- Use `uuid` primary keys (or `text` IDs with a prefix) consistently.
- Timestamps in UTC (`timestamptz`).
- Store structured payloads in `jsonb`.
- Keep large blob content out of Postgres; store only metadata + a storage reference.
- Carry first-class actor linkage (`tenant_id`, `agent_id`, `user_id`) on operational/audit tables to support enterprise traceability and queryability.

---

## Table: agents
Agent identity metadata.

Columns:
- `id` (uuid PK)
- `tenant_id` (text)
- `name` (text)
- `status` (text) — `active|disabled`
- `created_at` (timestamptz)

Indexes:
- `(tenant_id, created_at)`
- unique `(tenant_id, name)`

---

## Table: users
User identity metadata (local or mapped from external IdP subject).

Columns:
- `id` (uuid PK)
- `tenant_id` (text)
- `external_subject` (text, nullable)
- `display_name` (text, nullable)
- `status` (text) — `active|disabled`
- `created_at` (timestamptz)

Indexes:
- `(tenant_id, created_at)`
- unique `(tenant_id, external_subject)` where `external_subject` is not null

---

## Table: runs
Tracks a recipe execution.

Columns:
- `id` (uuid PK)
- `tenant_id` (text)
- `agent_id` (uuid FK → agents.id)
- `triggered_by_user_id` (uuid FK → users.id, nullable)
- `recipe_id` (text)
- `status` (text) — `queued|running|succeeded|failed|canceled`
- `input_json` (jsonb)
- `requested_capabilities` (jsonb) — as submitted by API
- `granted_capabilities` (jsonb) — after policy enforcement
- `created_at` (timestamptz)
- `started_at` (timestamptz, nullable)
- `finished_at` (timestamptz, nullable)
- `error_json` (jsonb, nullable)

Indexes:
- `(status, created_at)` for worker polling
- `(tenant_id, agent_id, created_at)`
- `(tenant_id, triggered_by_user_id, created_at)`

---

## Table: steps
A run is composed of steps (skill invocations or internal operations).

Columns:
- `id` (uuid PK)
- `run_id` (uuid FK → runs.id)
- `tenant_id` (text)
- `agent_id` (uuid FK → agents.id)
- `user_id` (uuid FK → users.id, nullable)
- `name` (text)
- `status` (text) — `queued|running|succeeded|failed|skipped`
- `input_json` (jsonb)
- `output_json` (jsonb, nullable)
- `started_at` (timestamptz, nullable)
- `finished_at` (timestamptz, nullable)
- `error_json` (jsonb, nullable)

Indexes:
- `(run_id)`
- `(tenant_id, agent_id, started_at)`
- `(status)` if steps are polled separately (optional)

---

## Table: artifacts
Metadata for run outputs (content stored elsewhere or inline for tiny text).

Columns:
- `id` (uuid PK)
- `run_id` (uuid FK)
- `path` (text) — e.g. `shownotes/ep245.md`
- `content_type` (text) — e.g. `text/markdown`
- `size_bytes` (bigint)
- `checksum` (text, nullable)
- `storage_ref` (text) — e.g. `localfs:/var/lib/app/artifacts/...` or `s3://bucket/key`
- `created_at` (timestamptz)

Indexes:
- `(run_id)`
- unique `(run_id, path)` (optional)

---

## Table: action_requests
Each skill can request privileged actions. Platform records requests and decisions.

Columns:
- `id` (uuid PK)
- `step_id` (uuid FK → steps.id)
- `action_type` (text) — `object.write|message.send|...`
- `args_json` (jsonb)
- `justification` (text, nullable)
- `status` (text) — `requested|allowed|denied|executed|failed`
- `decision_reason` (text, nullable)
- `created_at` (timestamptz)

Indexes:
- `(step_id)`
- `(status, created_at)`

---

## Table: action_results
Execution results for action requests (including denials if you prefer a single table).

Columns:
- `id` (uuid PK)
- `action_request_id` (uuid FK)
- `status` (text) — `executed|failed|denied`
- `result_json` (jsonb, nullable)
- `error_json` (jsonb, nullable)
- `executed_at` (timestamptz)

Indexes:
- `(action_request_id)`

---

## Table: audit_events
Append-only audit trail. Never mutate existing rows.

Columns:
- `id` (uuid PK)
- `run_id` (uuid FK)
- `step_id` (uuid FK, nullable)
- `tenant_id` (text)
- `agent_id` (uuid FK → agents.id, nullable)
- `user_id` (uuid FK → users.id, nullable)
- `actor` (text) — `api|worker|skill:<name>|system`
- `event_type` (text) — e.g. `run.created`, `skill.invoked`, `action.denied`, `action.executed`
- `payload_json` (jsonb)
- `created_at` (timestamptz)

Indexes:
- `(run_id, created_at)`
- `(tenant_id, agent_id, created_at)`
- `(tenant_id, user_id, created_at)`
- `(event_type, created_at)` (optional)

---

## Migration Notes
- Start with a single migration directory, e.g. `migrations/0001_init.sql`.
- Create/use one standardized app schema per environment (for example `aegis`), shared by all agents.
- Do not provision database-per-agent or schema-per-agent for normal operation.
- Prefer `sqlx` compile-time checking once query set stabilizes.
