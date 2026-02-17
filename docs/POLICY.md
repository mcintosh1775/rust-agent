# Policy & Capabilities (MVP)

## Capability types (v0)
- `object.read` / `object.write` (scoped prefixes)
- `message.send` (scoped provider+destination)
- `llm.infer` (scoped local vs remote route)
- `local.exec` (scoped template id)
- `db.query` (registered query ids)
- `http.request` (disabled for MVP or strict allowlist)

## Default policy
Deny everything by default. Allow only explicitly granted capabilities.

For API-created runs, grants are recipe-aware:
- known recipe IDs apply API-managed capability bundles
- user-requested capabilities are intersected with bundle scope
- empty requested list receives the recipe bundle defaults
- optional role presets (`x-user-role`) further constrain grants:
  - `owner` (default): recipe bundle as defined
  - `operator`: removes `local.exec`
  - `viewer`: allows only `object.read` and local-route `llm.infer`

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
