# RUNBOOK (MVP)

## Start (local)
1) Postgres:
   - `make container-info`
   - `make db-up`
2) Use one standardized app schema (for example `aegis`) for platform tables in this environment.
   - Migrations own schema creation/versioning; do not create a DB/schema per agent.
3) Migrate:
   - `make migrate`
4) Run:
   - `make api`
   - `make worker`

## Access boundary
- Agents/skills call platform APIs/protocols.
- Only `api` and `worker` services connect directly to Postgres.

## Incident actions
- Disable external actions via policy (deny `message.send` / `http.request`)
- Scale workers to zero to stop execution
- Revoke/rotate credentials if exfil suspected
