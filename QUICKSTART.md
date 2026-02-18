# QUICKSTART

This is the fastest path to a running local SecureAgnt stack in containers:
- Postgres
- API daemon (`secureagnt-api`)
- Worker daemon (`secureagntd`)

It also covers first API interactions so you can start testing behavior immediately.

## 1) Prerequisites

- `make`
- one container runtime:
  - Podman + compose support (`podman compose` or `podman-compose`), or
  - Docker + compose (`docker compose`)
- `curl`
- `jq` (optional, but strongly recommended)
- `uuidgen`

## 2) Start the stack (containers)

From repo root:

```bash
make container-info
make stack-up-build
make stack-ps
```

Expected:
- `postgres` mapped to `localhost:5432`
- `api` mapped to `localhost:8080`
- `worker` running in the stack profile

If you only want to tail logs:

```bash
make stack-logs
```

## 3) Verify API is reachable

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/ops/summary?window_secs=3600" | jq .
```

## 4) Seed one agent + one user (required for creating runs)

`POST /v1/runs` requires existing `agent_id` and `triggered_by_user_id`.

Generate IDs:

```bash
export AGENT_ID="$(uuidgen)"
export USER_ID="$(uuidgen)"
```

If you have `psql` installed locally:

```bash
psql "postgres://postgres:postgres@localhost:5432/agentdb" <<SQL
INSERT INTO agents (id, tenant_id, name, status)
VALUES ('${AGENT_ID}', 'single', 'quickstart-agent', 'active')
ON CONFLICT (id) DO NOTHING;

INSERT INTO users (id, tenant_id, external_subject, display_name, status)
VALUES ('${USER_ID}', 'single', 'quickstart-user', 'Quickstart User', 'active')
ON CONFLICT (id) DO NOTHING;
SQL
```

If you do not have local `psql`, use compose exec:

```bash
podman compose -f infra/containers/compose.yml --profile stack exec postgres \
  psql -U postgres -d agentdb -c \
  "INSERT INTO agents (id, tenant_id, name, status) VALUES ('${AGENT_ID}', 'single', 'quickstart-agent', 'active') ON CONFLICT (id) DO NOTHING;"

podman compose -f infra/containers/compose.yml --profile stack exec postgres \
  psql -U postgres -d agentdb -c \
  "INSERT INTO users (id, tenant_id, external_subject, display_name, status) VALUES ('${USER_ID}', 'single', 'quickstart-user', 'Quickstart User', 'active') ON CONFLICT (id) DO NOTHING;"
```

For Docker, replace `podman compose` with `docker compose`.

## 5) Create and track your first run

Create:

```bash
curl -sS -X POST "http://localhost:8080/v1/runs" \
  -H "content-type: application/json" \
  -H "x-tenant-id: single" \
  -H "x-user-role: owner" \
  -d "{
    \"agent_id\": \"${AGENT_ID}\",
    \"triggered_by_user_id\": \"${USER_ID}\",
    \"recipe_id\": \"show_notes_v1\",
    \"input\": {\"transcript_path\":\"podcasts/ep245/transcript.txt\"},
    \"requested_capabilities\": []
  }" | tee /tmp/secureagnt_run.json | jq .
```

Capture run ID:

```bash
export RUN_ID="$(jq -r '.id' /tmp/secureagnt_run.json)"
echo "$RUN_ID"
```

Status:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  "http://localhost:8080/v1/runs/${RUN_ID}" | jq .
```

Audit trail:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  "http://localhost:8080/v1/runs/${RUN_ID}/audit?limit=200" | jq .
```

## 6) Useful operator checks

Ops summary:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/ops/summary?window_secs=3600" | jq .
```

Action latency:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/ops/action-latency?window_secs=3600" | jq .
```

SIEM delivery alerts:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/audit/compliance/siem/deliveries/alerts?window_secs=3600&max_hard_failure_rate_pct=0&max_dead_letter_rate_pct=0&max_pending_count=0" | jq .
```

## 7) `agntctl` against container API

`agntctl` defaults to `http://localhost:3000`. For container stack, point it at `:8080`.

```bash
export AGNTCTL_API_BASE_URL="http://localhost:8080"
cargo run -p agntctl -- ops soak-gate --window-secs 3600
```

## 8) Stop the stack

```bash
make stack-down
```

## Web server note

The dedicated web operations console (M11) is not shipped yet.  
Current pattern is API-first on `:8080`, with optional reverse proxy/TLS in front for deployment.
