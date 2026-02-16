# RUNBOOK (MVP)

## Start (local)
1) Postgres:
   - `docker run --name agentdb -e POSTGRES_PASSWORD=postgres -p 5432:5432 -d postgres:16`
2) Migrate:
   - `make migrate`
3) Run:
   - `make api`
   - `make worker`

## Incident actions
- Disable external actions via policy (deny `message.send` / `http.request`)
- Scale workers to zero to stop execution
- Revoke/rotate credentials if exfil suspected
