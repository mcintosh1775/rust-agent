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
MAX_QUEUED_RUNS=25 \
MAX_FAILED_RUNS_WINDOW=5 \
MAX_DEAD_LETTER_EVENTS_WINDOW=0 \
MAX_P95_RUN_DURATION_MS=5000 \
ITERATIONS=30 \
SLEEP_SECS=60 \
API_BASE_URL=http://localhost:3000 \
TENANT_ID=single \
bash scripts/ops/soak_gate.sh
```

Watch for:
- sustained growth in `queued_runs` with low `running_runs`
- spikes in `failed_runs_window`
- non-zero `dead_letter_trigger_events_window`

Single-check mode (no loop):
```bash
cargo run -p agntctl -- ops soak-gate \
  --api-base-url http://localhost:3000 \
  --tenant-id single \
  --user-role operator \
  --window-secs 3600 \
  --max-queued-runs 25 \
  --max-failed-runs-window 5 \
  --max-dead-letter-events-window 0 \
  --max-p95-run-duration-ms 5000
```

## Perf baseline capture
Capture fresh perf-gate baseline files from staging before evaluating candidate regressions:

```bash
AGNTCTL_API_BASE_URL=http://localhost:3000 \
AGNTCTL_TENANT_ID=single \
AGNTCTL_USER_ROLE=operator \
WINDOW_SECS=3600 \
TRACE_LIMIT=500 \
CAPTURE_BASELINE_OUTPUT_DIR=agntctl/fixtures/generated \
make capture-perf-baseline
```

Then run regression checks against the captured baseline:

```bash
cargo run -p agntctl -- ops perf-gate \
  --api-base-url http://localhost:3000 \
  --tenant-id single \
  --user-role operator \
  --window-secs 3600 \
  --baseline-summary-json agntctl/fixtures/generated/ops_baseline_YYYYMMDDTHHMMSSZ_summary.json \
  --baseline-histogram-json agntctl/fixtures/generated/ops_baseline_YYYYMMDDTHHMMSSZ_latency_histogram.json \
  --baseline-traces-json agntctl/fixtures/generated/ops_baseline_YYYYMMDDTHHMMSSZ_latency_traces.json
```

## Compliance replay signing-key rotation
Use this workflow when rotating replay manifest signing keys.

1) Baseline current signing behavior:
   - call `GET /v1/audit/compliance/replay-package?run_id=<id>` and record:
     - `manifest.digest_sha256`
     - `manifest.signing_mode`
     - `manifest.signature`
2) Stage new signing key material in your secret backend as a new version.
3) Pin API to the new key version:
   - set `COMPLIANCE_REPLAY_SIGNING_KEY_REF` to version-pinned secret reference.
4) Roll API and verify:
   - replay package still returns deterministic `digest_sha256` for same source data.
   - `signature` value changes when key changes.
   - `signing_mode` remains `hmac-sha256`.
5) Remove stale key versions after rollback window expires.

Rollback:
- revert `COMPLIANCE_REPLAY_SIGNING_KEY_REF` to previous version-pinned key.
- restart API and confirm replay package signatures return to prior key lineage.
