# RUNBOOK (MVP)

## Start (local)
1) Postgres:
   - `make container-info`
   - `make db-up`
   - default compose file: `infra/containers/compose.yml`
2) Use one standardized app schema (for example `secureagnt`) for platform tables in this environment.
   - Migrations own schema creation/versioning; do not create a DB/schema per agent.
3) Migrate:
   - `make migrate`
4) Run:
   - `make api`
   - `make worker`
   - optional aliases: `make secureagnt-api`, `make secureagntd`

## Access boundary
- Agents/skills call platform APIs/protocols.
- Only `api` and `worker` services connect directly to Postgres.

## Incident checklist (first 15 minutes)
1) Stabilize execution:
   - Disable privileged side effects by policy (`message.send`, `payment.send`, `http.request`).
   - Scale workers to zero to stop new action execution.
2) Preserve evidence:
   - Keep audit and compliance retention policies intact (enable legal hold where required).
   - Export compliance events for affected tenant/time window (`GET /v1/audit/compliance/export`).
3) Scope impact:
   - Query tenant ops summary for current queue/failure pressure (`GET /v1/ops/summary`).
   - Query run and payment ledgers for suspect run IDs (`GET /v1/runs/{id}/audit`, `GET /v1/payments`).
4) Remediate:
   - Rotate affected credentials/secrets.
   - Re-enable workers after policy and secret controls are confirmed.

## Backup and restore drill (Postgres)
Run from a host with network access to Postgres.

Backup:
```bash
PGPASSWORD="$POSTGRES_PASSWORD" pg_dump \
  --host "$POSTGRES_HOST" \
  --port "${POSTGRES_PORT:-5432}" \
  --username "$POSTGRES_USER" \
  --format custom \
  --file secureagnt_$(date +%Y%m%d_%H%M%S).dump \
  "$POSTGRES_DB"
```

Restore (staging drill target):
```bash
createdb -h "$POSTGRES_HOST" -p "${POSTGRES_PORT:-5432}" -U "$POSTGRES_USER" secureagnt_restore_drill
PGPASSWORD="$POSTGRES_PASSWORD" pg_restore \
  --host "$POSTGRES_HOST" \
  --port "${POSTGRES_PORT:-5432}" \
  --username "$POSTGRES_USER" \
  --dbname secureagnt_restore_drill \
  --clean --if-exists \
  secureagnt_YYYYMMDD_HHMMSS.dump
```

## Migration rollback workflow
Current migrations are forward-only. Rollback should use restore + redeploy:
1) Stop API/worker writes (scale workers to zero; block mutating API traffic).
2) Restore database from the last known-good backup.
3) Deploy the previous known-good application build.
4) Run validation checks:
   - `GET /v1/ops/summary`
   - sample `GET /v1/runs/{id}`
   - sample `GET /v1/runs/{id}/audit`
5) Reopen traffic after validation passes.

## Soak check baseline
Use a low-cost rolling check during staging soak windows:

```bash
for i in $(seq 1 30); do
  curl -sS \
    -H "x-tenant-id: single" \
    -H "x-user-role: operator" \
    "http://localhost:3000/v1/ops/summary?window_secs=3600" | jq .
  sleep 60
done
```

Watch for:
- sustained growth in `queued_runs` with low `running_runs`
- spikes in `failed_runs_window`
- non-zero `dead_letter_trigger_events_window`
