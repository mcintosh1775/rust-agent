# Policy & Capabilities (MVP)

## Capability types (v0)
- `object.read` / `object.write` (scoped prefixes)
- `message.send` (scoped provider+destination)
- `db.query` (registered query ids)
- `http.request` (disabled for MVP or strict allowlist)

## Default policy
Deny everything by default. Allow only explicitly granted capabilities.

## MVP example: show-notes recipe
Granted:
- `object.read` scope `podcasts/*`
- `object.write` scope `shownotes/*` max 500KB
- `message.send` scope `slack:C123456` max 20KB

Denied:
- all `http.request`
- all writes outside `shownotes/*`
- messages outside allowlisted destinations
