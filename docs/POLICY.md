# Policy & Capabilities (MVP)

## Capability types (v0)
- `object.read` / `object.write` (scoped prefixes)
- `memory.read` / `memory.write` (scoped memory namespaces)
- `message.send` (scoped provider+destination)
- `payment.send` (scoped payment rails/destinations, NWC-first)
- `llm.infer` (scoped local vs remote route)
- `local.exec` (scoped template id)
- `db.query` (registered query ids)
- `http.request` (disabled for MVP or strict allowlist)

## Default policy
Deny everything by default. Allow only explicitly granted capabilities.

Current payment capability parsing accepts:
- `nwc:*` (NWC runtime baseline)
- `cashu:*` (Cashu scaffold routing path; execution remains fail-closed until full rail implementation)

For API-created runs, grants are recipe-aware:
- known recipe IDs apply API-managed capability bundles
- user-requested capabilities are intersected with bundle scope
- empty requested list receives the recipe bundle defaults
- optional role presets (`x-user-role`) further constrain grants:
  - `owner` (default): recipe bundle as defined
  - `operator`: removes `local.exec`
  - `viewer`: allows only `object.read` and local-route `llm.infer`

Memory policy baseline:
- memory scopes use `memory:` prefix (for example `memory:project/*`).
- write and read are separate capabilities (`memory.write`, `memory.read`).
- memory record mutation endpoints are API role-gated (`owner`/`operator` write, `owner` purge).

Trigger mutation policy (API):
- `owner` and `operator` can create/fire triggers.
- `viewer` cannot mutate triggers (`POST /v1/triggers`, `POST /v1/triggers/cron`, `POST /v1/triggers/webhook`, `PATCH /v1/triggers/{id}`, `POST /v1/triggers/{id}/enable`, `POST /v1/triggers/{id}/disable`, `POST /v1/triggers/{id}/fire` return `403`).
- operator trigger mutation requires `x-user-id`; operators can only create/mutate triggers for their own user id.
- Webhook event ingestion (`POST /v1/triggers/{id}/events`) is controlled by trigger secret validation when configured, not by role header.

Governance policy gates (worker):
- Optional approval gate for irreversible actions:
  - `WORKER_APPROVAL_REQUIRED_ACTION_TYPES` (CSV action types)
  - configured action types require explicit approval flags in action args (`approved=true`; `payment.send` also accepts `payment_approved=true`)
- Optional skill provenance gate:
  - `WORKER_SKILL_SCRIPT_SHA256`
  - mismatch fails skill invoke before side effects execute.

## MVP example: show-notes recipe
Granted:
- `object.read` scope `podcasts/*`
- `object.write` scope `shownotes/*` max 500KB
- `message.send` scope `whitenoise:npub1...` max 20KB
- `llm.infer` scope `local:*` max 32KB
- `local.exec` scope `local.exec:file.head` max 4KB
- `message.send` scope `slack:C123456` max 20KB (enterprise optional)

Denied:
- all `http.request`
- all writes outside `shownotes/*`
- messages outside allowlisted White Noise/Slack destinations
