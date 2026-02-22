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
- `python3` (required for M15 solo-lite init/smoke scripts)
- `curl` and `git` (for one-command installer and source bootstrap fallback)

## 1a) One-command solo-lite installer

From a fresh machine, download and run the installer script:

```bash
curl -fsSL https://raw.githubusercontent.com/nearai/secureagnt/main/scripts/install/secureagnt-solo-lite-installer.sh \
  -o /tmp/secureagnt-solo-lite-installer.sh
chmod +x /tmp/secureagnt-solo-lite-installer.sh
bash /tmp/secureagnt-solo-lite-installer.sh
```

The installer is interactive by default (Linux x86_64 only) and asks for:

- agent name
- role
- communication style/personality
- key values
- boundaries for `SOUL.md`
- sandbox root (default: `/opt/agent`)
- worker artifact root (default: `<sandbox_root>/artifacts`)
- local.exec read roots
- local.exec write roots

It sets up:

- `secureagnt-api`, `secureagntd`, and `agntctl` binaries
- a seeded `agent_context/<tenant>/<agent_id>/` with generated `IDENTITY.md`, `SOUL.md`, and `USER.md`
- a first boot solo-lite `agent run` to validate local stack startup
- instructions for interactive chat mode (`scripts/ops/solo_lite_chat.py`)

Non-interactive/default install with env overrides:

```bash
SECUREAGNT_NON_INTERACTIVE=1 \
SECUREAGNT_RELEASE_VERSION=latest \
bash /tmp/secureagnt-solo-lite-installer.sh
```

Seeded defaults:

- name: `solo-lite-agent`
- role: `Personal coordinator and operations assistant for a single workspace`
- style: `concise, practical, evidence-first`
- values: `secure-by-default behavior, auditable actions, clear communication`
- boundaries: `do not bypass policy, do not invent authority, escalate uncertainty for high-risk actions`
- sandbox root: `/opt/agent`
- worker artifact root: `/opt/agent/artifacts`
- local.exec read roots: `/opt/agent/artifacts`
- local.exec write roots: `/opt/agent/artifacts`

The installer also accepts binary/worktree overrides via env vars from the script usage output:

- `SECUREAGNT_INSTALL_HOME`
- `SECUREAGNT_BINARY_DIR`
- `SECUREAGNT_WORKTREE`
- `SECUREAGNT_RELEASE_REPO`
- `SECUREAGNT_RELEASE_VERSION`
- `SECUREAGNT_PLATFORM_TAG`
- `SECUREAGNT_SANDBOX_ROOT`
- `WORKER_ARTIFACT_ROOT`
- `WORKER_LOCAL_EXEC_READ_ROOTS`
- `WORKER_LOCAL_EXEC_WRITE_ROOTS`

## 2) Start the stack (containers)

From repo root:

Choose deployment profile (optional but recommended):

Solo/dev (minimal, non-enterprise):

```bash
set -a
source infra/config/profile.solo-dev.env
set +a
```

Enterprise (hardened baseline):

```bash
set -a
source infra/config/profile.enterprise.env
set +a
```

Profile loading note:
- On `podman-compose` 1.3.x, source one of these profile files before `make stack-up*` so all compose-referenced env vars are set (including intentional empty values).

Solo-lite profile (M15 complete):

```bash
set -a
source infra/config/profile.solo-lite.env
set +a
make solo-lite-init
make solo-lite-smoke
```

Solo-lite note:
- API SQLite mode exposes a scoped profile for runs, triggers, agent context/bootstrap/heartbeat control-plane endpoints, memory, payments/usage reporting, core ops endpoints (summary/latency/action-latency/llm-gateway), and compliance replay/verify/policy/purge + SIEM delivery surfaces; non-profile routes return `SQLITE_PROFILE_ENDPOINT_UNAVAILABLE`.
- worker supports SQLite for core run-loop paths including scheduler, memory-compaction, and compliance-outbox flows.
- `make solo-lite-init` and `make solo-lite-smoke` provide the SQLite schema + lifecycle smoke baseline.
- `make stack-lite-smoke` validates the running `api-lite` container profile over HTTP (including SQLite compliance/ops route checks).

Enterprise profile note:
- sets `LLM_REMOTE_EGRESS_CLASS=redacted_only`, so remote `llm.infer` calls are allowed only when action args include `redacted=true`.
- also enables gateway verifier escalation + response cache defaults (`LLM_VERIFIER_ENABLED=1`, `LLM_CACHE_ENABLED=1`).
- verifier defaults to deterministic mode (`LLM_VERIFIER_MODE=deterministic`) to avoid extra judge-model token burn unless you explicitly configure `LLM_VERIFIER_JUDGE_*`.
- enables optional shared gateway controls for multi-worker setups (`LLM_DISTRIBUTED_ENABLED=1`); for a single worker/small local setup you can set this back to `0`.
- local-tier defaults remain `workhorse` for both lanes; to activate dual local tiers set:
  - `LLM_LOCAL_SMALL_MODEL` (and optional `LLM_LOCAL_SMALL_BASE_URL`)
  - `LLM_LOCAL_INTERACTIVE_TIER=small` and/or `LLM_LOCAL_BATCH_TIER=small`
- includes lane-SLO defaults for gateway monitoring/tuning:
  - `LLM_SLO_INTERACTIVE_MAX_LATENCY_MS=6000`
  - `LLM_SLO_BATCH_MAX_LATENCY_MS=30000`
  - `LLM_SLO_ALERT_THRESHOLD_PCT=80`

Then start the stack:

```bash
make container-info
make stack-up-build
make stack-ps
```

Or start the no-Postgres solo-lite stack:

```bash
make stack-lite-up-build
make stack-lite-ps
make stack-lite-smoke
make stack-lite-soak
make stack-lite-signoff
```

One-command solo-lite agent run (start stack, seed agent/user, scaffold context files, submit run, and print audit summary):

```bash
make solo-lite-agent
```

This launcher starts stack-lite with worker context loading enabled (`WORKER_AGENT_CONTEXT_ENABLED=1`) so the scaffolded `agent_context/<tenant>/<agent-id>/` files are usable immediately.
If `api-lite` is already reachable, it skips stack startup and just seeds/scaffolds/submits a run.
It also provisions a per-agent Nostr keypair under `var/agent_keys/<tenant>/<agent_id>/` and prints:
- `AGENT_NPUB`
- `AGENT_NSEC_FILE`
Use `--print-agent-nsec` only when you explicitly need the secret printed to stdout.
By default, launcher-managed stack wiring uses local signer mode (`NOSTR_SIGNER_MODE=local_key`) with `NOSTR_SECRET_KEY_FILE` mapped into `worker-lite`.

Custom text prompt path (re-use running stack):

```bash
python3 scripts/ops/solo_lite_agent_run.py \
  --no-start-stack \
  --text "Summarize this: release prep is complete and signoff passed."
```

Use richer non-LLM digest output style:

```bash
python3 scripts/ops/solo_lite_agent_run.py \
  --no-start-stack \
  --summary-style ops_digest \
  --text "Create an operations digest: queue depth stable, one retry spike, no critical alerts."
```

Interactive chat-like loop (same agent/user across turns):

```bash
make solo-lite-chat
```

Inside chat:
- send any plain text prompt to create a run
- `/style ops_digest` or `/style summary`
- `/ids` to print tenant/agent/user ids
- `/keys` to print `AGENT_NPUB` and `AGENT_NSEC_FILE`
- `/last` to print last run id
- `/exit` to stop

Enterprise signer wiring path (NIP-46) from launcher:

```bash
python3 scripts/ops/solo_lite_agent_run.py \
  --nostr-signer-mode nip46_signer \
  --nostr-nip46-bunker-uri 'bunker://<npub>?relay=wss://relay.example' \
  --nostr-nip46-public-key <npub_or_hex_pubkey> \
  --nostr-relays 'wss://relay.example'
```

Operator -> agent White Noise proof path (relay -> webhook trigger -> run):

0) Bootstrap (or reuse) an operator keypair:

```bash
cargo run -p agntctl -- operator bootstrap-identity --name quickstart
```

1) Start a one-shot bridge listener (auto-creates webhook trigger when `--trigger-id` is omitted):

```bash
cargo run -p agntctl -- operator listen -- \
  --base-url http://localhost:18080 \
  --tenant-id single \
  --relay wss://relay.damus.io \
  --agent-id <AGENT_ID> \
  --agent-pubkey <AGENT_NPUB_OR_HEX> \
  --operator-pubkey <OPERATOR_NPUB_OR_HEX> \
  --max-events 1
```

2) Send a White Noise text-note to the agent pubkey:

```bash
cargo run -p agntctl -- operator send -- \
  --relay wss://relay.damus.io \
  --to <AGENT_NPUB_OR_HEX> \
  --text "operator check-in: summarize queue health" \
  --secret-key <OPERATOR_NSEC_OR_HEX>
```

The bridge prints enqueue status JSON and the worker then processes a queued run from that trigger event.
Run input receives trigger metadata under `_trigger` plus inbound relay content under `event_payload`.
By default, bridge-managed trigger creation uses `recipe_id=operator_reply_v1` (minimal `message.send` capability bundle) and auto-replies to the inbound event author.

One-command roundtrip smoke (operator message + agent reply path):

```bash
make whitenoise-roundtrip-smoke
```

Optional relay override:

```bash
WHITENOISE_SMOKE_ARGS="--nostr-relay wss://relay.damus.io" make whitenoise-roundtrip-smoke
```

One-command enterprise roundtrip smoke (Postgres `stack` profile):

```bash
make whitenoise-enterprise-smoke
```

Optional relay/signer override:

```bash
WHITENOISE_ENTERPRISE_SMOKE_ARGS="--nostr-relay wss://relay.damus.io --nostr-signer-mode nip46_signer --nostr-nip46-bunker-uri <BUNKER_URI>" make whitenoise-enterprise-smoke
```

CI-safe/local relay mode (no public relay dependency):

```bash
WHITENOISE_ENTERPRISE_SMOKE_ARGS="--spawn-mock-relay" make whitenoise-enterprise-smoke
```

Expected solo-lite host endpoint:
- `api-lite` mapped to `localhost:18080` by default (`SOLO_LITE_API_PORT`)

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

Optional: open the baseline web console in your browser:

```text
http://localhost:8080/console
```

Console note:
- `owner` and `operator` can query the default reporting panels.
- `viewer` will show role-restricted panel states for reporting endpoints that enforce higher role access.
- For run drill-down, paste a `run-id` and click `Load Run Context` to fetch `/v1/runs/:id` and `/v1/runs/:id/audit`.
- For one-off onboarding, use `Load Bootstrap` / `Complete Bootstrap` with `agent-id` (and `x-user-id` for completion).
- The console remembers tenant/role/filter controls in browser local storage for repeat sessions.
- Use `Export Snapshot JSON` / `Export Health JSON` to download current console telemetry views for incident notes or handoff.

## 4) Seed one agent + one user (required for creating runs)

`POST /v1/runs` requires existing `agent_id` and `triggered_by_user_id`.

One-command path:

```bash
make quickstart-seed
```

That command:
- generates `AGENT_ID` and `USER_ID` (unless you provide them),
- inserts agent/user rows,
- prints export lines you can use directly.

Optional overrides:

```bash
TENANT_ID=single \
QUICKSTART_AGENT_NAME="quickstart-agent" \
QUICKSTART_USER_SUBJECT="quickstart-user" \
QUICKSTART_USER_DISPLAY_NAME="Quickstart User" \
make quickstart-seed
```

Manual path (if you want explicit SQL instead):

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

## 5) (Optional) Enable per-agent context profile loading

Create profile files for your seeded agent:

```bash
TENANT_ID=single \
AGENT_ID="${AGENT_ID}" \
AGENT_NAME="show-producer" \
make agent-context-init
```

Enable context loading in container stack mode and restart:

```bash
WORKER_AGENT_CONTEXT_ENABLED=1 \
WORKER_AGENT_CONTEXT_REQUIRED=1 \
make stack-up-build
```

Path convention used by worker:
- `agent_context/<tenant_id>/<agent_id>/...`
- fallback: `agent_context/<agent_id>/...`

The worker injects loaded profile data into skill input as `agent_context` and emits audit events:
- `agent.context.loaded`
- `agent.context.not_found`
- `agent.context.error`

Inspect effective context metadata from API:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/agents/${AGENT_ID}/context" | jq .
```

Inspect bootstrap status (`BOOTSTRAP.md`) for one-off setup:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/agents/${AGENT_ID}/bootstrap" | jq .
```

Complete bootstrap (owner + `x-user-id` required):

```bash
curl -sS -X POST \
  -H "content-type: application/json" \
  -H "x-tenant-id: single" \
  -H "x-user-role: owner" \
  -H "x-user-id: ${USER_ID}" \
  "http://localhost:8080/v1/agents/${AGENT_ID}/bootstrap/complete" \
  -d '{"user_markdown":"# USER\nprefers concise updates","completion_note":"quickstart bootstrap complete"}' | jq .
```

Compile heartbeat intents from `HEARTBEAT.md` to trigger candidates (no side effects):

```bash
curl -sS -X POST \
  -H "content-type: application/json" \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/agents/${AGENT_ID}/heartbeat/compile" \
  -d '{}' | jq .
```

Preview governed heartbeat materialization (plan-only):

```bash
curl -sS -X POST \
  -H "content-type: application/json" \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/agents/${AGENT_ID}/heartbeat/materialize" \
  -d '{"apply":false}' | jq .
```

Apply materialization with explicit approval attribution:

```bash
curl -sS -X POST \
  -H "content-type: application/json" \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  -H "x-user-id: ${USER_ID}" \
  "http://localhost:8080/v1/agents/${AGENT_ID}/heartbeat/materialize" \
  -d '{"apply":true,"approval_confirmed":true,"approval_note":"quickstart approved"}' | jq .
```

Optional context mutation API (off by default):

```bash
API_AGENT_CONTEXT_MUTATION_ENABLED=1 make stack-up-build
```

Then append a session line:

```bash
curl -sS -X POST \
  -H "content-type: application/json" \
  -H "x-tenant-id: single" \
  -H "x-user-role: owner" \
  "http://localhost:8080/v1/agents/${AGENT_ID}/context" \
  -d '{"relative_path":"sessions/quickstart.jsonl","content":"{\"event\":\"quickstart\"}","mode":"append"}' | jq .
```

## 6) Create and track your first run

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

Queue-lane note:
- include `"queue_class":"interactive"` (default) or `"queue_class":"batch"` inside `input` to hint worker claim priority for mixed latency workloads.

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

## 7) Useful operator checks

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

LLM gateway lanes:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/ops/llm-gateway?window_secs=3600" | jq .
```

SIEM delivery alerts:

```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:8080/v1/audit/compliance/siem/deliveries/alerts?window_secs=3600&max_hard_failure_rate_pct=0&max_dead_letter_rate_pct=0&max_pending_count=0" | jq .
```

## 8) `agntctl` against container API

`agntctl` defaults to `http://localhost:3000`. For container stack, point it at `:8080`.

```bash
export AGNTCTL_API_BASE_URL="http://localhost:8080"
cargo run -p agntctl -- ops soak-gate --window-secs 3600
```

## 9) Stop the stack

```bash
make stack-down
```

## Web server note

The M11A baseline web console is served by the API process at `/console` (no separate web container yet).
For production, continue placing TLS/auth in front of API via your reverse proxy.
