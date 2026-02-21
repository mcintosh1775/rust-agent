# Operations Guide

This is a living document for deploying and operating SecureAgnt.

For the full enterprise-grade operator reference, use:
- `docs/OPERATIONS_MANUAL.md`

This guide remains the concise operational baseline and points to canonical controls.

## Scope
- Service topology
- Runtime operations and safety controls
- Incident handling and maintenance checkpoints

## Production topology (baseline)
- `api` service (control plane)
- `worker` service (data plane)
- Shared Postgres cluster per environment (`dev`, `staging`, `prod`)
- Optional object store for artifacts

Rules:
- Use one standardized app schema per environment (for example `secureagnt`).
- Do not create Postgres instances or schemas per agent as a default model.
- Only `api` and `worker` connect to Postgres directly.
- Skills and agents interact through platform APIs/protocols only.

Filesystem/service naming baseline:
- Config dir: `/etc/secureagnt/`
- Primary config: `/etc/secureagnt/secureagnt.yaml`
- State dir: `/var/lib/secureagnt/`
- Logs dir: `/var/log/secureagnt/`
- systemd units:
  - `secureagnt.service` (worker daemon)
  - `secureagnt-api.service` (API daemon)
  - unit templates are provided in `infra/systemd/`

Systemd install baseline:
```bash
sudo cp infra/systemd/secureagnt*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now secureagnt.service secureagnt-api.service
```

macOS launchd baseline templates:
- `infra/launchd/secureagnt.plist`
- `infra/launchd/secureagnt-api.plist`

Launchd install baseline:
```bash
sudo cp infra/launchd/secureagnt*.plist /Library/LaunchDaemons/
sudo launchctl load /Library/LaunchDaemons/secureagnt.plist
sudo launchctl load /Library/LaunchDaemons/secureagnt-api.plist
```

Container-compose baseline:
- Compose file: `infra/containers/compose.yml`
- Default profile:
  - `postgres` only (`make db-up`)
- `stack` profile:
  - `postgres + api + worker` (`make stack-up`)
  - API service runs migrations during startup (`API_RUN_MIGRATIONS=1`)
  - image builds are throttled with `SECUREAGNT_CARGO_BUILD_JOBS` (default `2`)

Container stack workflow:
```bash
make container-info
make stack-build
make stack-up
make stack-up-build
make stack-ps
make stack-logs
make stack-down
```

Deployment preflight portability checks:
```bash
make deploy-preflight
DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 make deploy-preflight
```

Profile presets (optional before `make stack-up*`):
- solo/dev:
  - `set -a; source infra/config/profile.solo-dev.env; set +a`
- enterprise:
  - `set -a; source infra/config/profile.enterprise.env; set +a`

Web console baseline:
- API serves the M11A console shell at `GET /console`.
- In stack mode, open `http://localhost:8080/console`.
- Console role selector supports `owner`, `operator`, and `viewer`.
- `viewer` requests are shown as role-restricted (`ROLE_FORBIDDEN`) on reporting panels that require higher role privileges.
- Drill-down panels now include:
  - run/action latency traces
  - run detail and run audit for selected `run-id`
  - payments ledger filtered by optional `run-id`/`agent-id`
  - compliance delivery alerts
- Console includes `LLM Gateway Lanes` visibility backed by:
  - `/v1/ops/llm-gateway`
- Console controls persist locally in browser storage (`secureagnt_console_controls_v1`) for repeat workflows.
- Console threshold chips highlight warning/critical posture for latency, token burn, payment failures, and SIEM delivery failures.
- Console supports operator snapshot exports:
  - `Export Snapshot JSON` (full panel payloads + controls + thresholds)
  - `Export Health JSON` (summary posture payload)

Build behavior:
- `make stack-up` starts containers without rebuilding images.
- `make stack-up-build` forces rebuild + start.
- `make stack-build` rebuilds images only.

## Security baseline
- API behind TLS reverse proxy.
- Private network access from `api`/`worker` to Postgres.
- Worker/skill runtime outbound egress deny-by-default.
- Secrets in Vault/KMS, never exposed to skills.
- Structured logs with redaction.
- Skills launched by worker run with cleared environments by default; only allowlisted env vars are passed (`WORKER_SKILL_ENV_ALLOWLIST`).
- API role presets (`x-user-role`) are currently header-driven; production deployments should set/override this only at trusted auth gateway boundaries.
- Trusted auth-gateway enforcement (recommended for production):
  - `API_TRUSTED_PROXY_AUTH_ENABLED=1`
  - configure shared secret via:
    - `API_TRUSTED_PROXY_SHARED_SECRET`, or
    - `API_TRUSTED_PROXY_SHARED_SECRET_REF`
  - role/user-header requests must include `x-auth-proxy-token`
  - missing/invalid proxy token returns `401 UNAUTHORIZED`
- Compliance alert acknowledgements:
  - use `POST /v1/audit/compliance/siem/deliveries/alerts/ack` for operator workflow state
  - require `x-user-id` for accountability; enforce at auth gateway in production
  - optional `run_id` allows per-run alert scope; omitted `run_id` uses tenant-global scope (`*`)
- Trigger mutation endpoints are role-restricted:
  - `owner`/`operator` can create/update/enable/disable/manual-fire triggers
  - `viewer` is denied (`403`) on trigger mutation routes
  - operator-trigger mutations require `x-user-id`; operators are restricted to their own trigger ownership
- Optional API tenant in-flight guardrail:
  - `API_TENANT_MAX_INFLIGHT_RUNS` enforces max queued+running run count per tenant for `POST /v1/runs`
  - over-capacity requests fail with `429` (`TENANT_INFLIGHT_LIMITED`)
- Optional API tenant trigger-capacity guardrail:
  - `API_TENANT_MAX_TRIGGERS` enforces max trigger definitions per tenant for `POST /v1/triggers`, `POST /v1/triggers/cron`, and `POST /v1/triggers/webhook`
  - over-capacity requests fail with `429` (`TENANT_TRIGGER_LIMITED`)
- Optional API tenant memory-capacity guardrail:
  - `API_TENANT_MAX_MEMORY_RECORDS` enforces max active memory rows per tenant for `POST /v1/memory/records` and `POST /v1/memory/handoff-packets`
  - over-capacity requests fail with `429` (`TENANT_MEMORY_LIMITED`)
- Worker trigger scheduler is enabled by default (`WORKER_TRIGGER_SCHEDULER_ENABLED=1`) and dispatches:
  - due interval triggers
  - due cron triggers
  - due webhook trigger events from the trigger event queue
  - with tenant in-flight guardrail `WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS` (default `100`)
  - optional lease gate for HA scheduler coordination:
    - `WORKER_TRIGGER_SCHEDULER_LEASE_ENABLED` (default `1`)
    - `WORKER_TRIGGER_SCHEDULER_LEASE_NAME`
    - `WORKER_TRIGGER_SCHEDULER_LEASE_TTL_MS`
- Trigger availability semantics:
  - webhook event dead-letter state is tracked in `trigger_events.status='dead_lettered'`
  - trigger schedule-broken state is tracked on trigger rows via `triggers.dead_lettered_at` / `triggers.dead_letter_reason`
  - API returns `409 CONFLICT` for mutation/ingest paths when trigger exists but is unavailable (`disabled` or schedule-broken)

## Nostr signer operations
- Signer mode is explicit via runtime config:
  - `local_key` (default): self-hosted/smaller deployments.
  - `nip46_signer`: enterprise/hardened mode with remote signer.
- Prefer `nip46_signer` when worker hosts are not trusted for private key custody.
- If using `local_key` file mode, enforce owner-only file permissions (`0600`).
- Track and monitor configured signer public keys (`npub`) as part of identity inventory.

## Core operational controls
- Disable external actions by policy when needed (`message.send`, `http.request`).
- Scale workers to zero to halt execution safely.
- Rotate credentials if exfiltration is suspected.
- Preserve append-only audit trails for investigations.
- Audit/action payloads are redacted before persistence for sensitive keys/token formats.
- Agent context profile controls:
  - enable context loading with `WORKER_AGENT_CONTEXT_ENABLED=1`
  - use `WORKER_AGENT_CONTEXT_REQUIRED=1` for fail-closed profile enforcement
  - root path defaults to `agent_context` (`WORKER_AGENT_CONTEXT_ROOT` overrides)
  - tenant-aware path resolution:
    - `<root>/<tenant_id>/<agent_id>/`
    - fallback `<root>/<agent_id>/`
  - context load/injection audit visibility:
    - `agent.context.loaded`
    - `agent.context.not_found`
    - `agent.context.error`
  - API operator inspection and compile controls:
    - `GET /v1/agents/{agent_id}/context` for checksum/precedence/mutability metadata
    - `POST /v1/agents/{agent_id}/heartbeat/compile` for policy-safe heartbeat intent compile
    - `POST /v1/agents/{agent_id}/heartbeat/materialize` for governed trigger materialization:
      - `apply=false` for plan-only preview
      - `apply=true` requires `approval_confirmed=true` and `x-user-id`
      - existing matching schedules are detected and skipped
  - API context mutation path is opt-in and disabled by default:
    - enable with `API_AGENT_CONTEXT_MUTATION_ENABLED=1`
    - immutable files are always denied (`AGENTS.md`, `TOOLS.md`, `IDENTITY.md`, `SOUL.md`)
    - `USER.md` and `HEARTBEAT.md` are owner-only
    - `MEMORY.md`, `memory/*.md`, and `sessions/*.jsonl` are owner/operator
    - `sessions/*.jsonl` accepts append mode only
- Keep `local.exec` disabled unless needed (`WORKER_LOCAL_EXEC_ENABLED=0` by default). When enabled, use minimal read/write root allowlists.
- Keep `LLM_MODE=local_first` (or `local_only`) unless remote routing is explicitly needed.
- LLM gateway large-input controls:
  - `LLM_MAX_INPUT_BYTES` bounds raw prompt size before preprocessing.
  - `LLM_LARGE_INPUT_THRESHOLD_BYTES` defines when large-input policy activates.
  - `LLM_LARGE_INPUT_POLICY` controls default behavior:
    - `direct`
    - `summarize_first`
    - `chunk_and_retrieve`
    - `escalate_remote`
  - `LLM_LARGE_INPUT_SUMMARY_TARGET_BYTES` caps summarize-first output size.
  - `LLM_CONTEXT_RETRIEVAL_TOP_K`, `LLM_CONTEXT_RETRIEVAL_MAX_BYTES`, `LLM_CONTEXT_RETRIEVAL_CHUNK_BYTES` tune retrieval packing.
- LLM gateway admission/cache/verifier controls:
  - admission gates:
    - `LLM_ADMISSION_ENABLED`
    - `LLM_ADMISSION_INTERACTIVE_MAX_INFLIGHT`
    - `LLM_ADMISSION_BATCH_MAX_INFLIGHT`
  - optional distributed mode for multi-worker/shared gateway capacity:
    - `LLM_DISTRIBUTED_ENABLED`
    - `LLM_DISTRIBUTED_FAIL_OPEN`
    - `LLM_DISTRIBUTED_OWNER`
    - `LLM_DISTRIBUTED_ADMISSION_ENABLED`
    - `LLM_DISTRIBUTED_ADMISSION_LEASE_MS`
    - `LLM_DISTRIBUTED_CACHE_ENABLED`
    - `LLM_DISTRIBUTED_CACHE_NAMESPACE_MAX_ENTRIES`
    - storage tables:
      - `llm_gateway_admission_leases`
      - `llm_gateway_cache_entries`
    - solo/small deployments (single worker or a few agents) should keep this disabled.
  - response cache:
    - `LLM_CACHE_ENABLED`
    - `LLM_CACHE_TTL_SECS`
    - `LLM_CACHE_MAX_ENTRIES`
  - verifier escalation:
    - `LLM_VERIFIER_ENABLED`
    - `LLM_VERIFIER_MODE` (`heuristic`, `deterministic`, `model_judge`, `hybrid`)
    - `LLM_VERIFIER_MIN_SCORE_PCT`
    - `LLM_VERIFIER_ESCALATE_REMOTE`
    - `LLM_VERIFIER_MIN_RESPONSE_CHARS`
    - optional model-judge controls:
      - `LLM_VERIFIER_JUDGE_BASE_URL`
      - `LLM_VERIFIER_JUDGE_MODEL`
      - `LLM_VERIFIER_JUDGE_API_KEY` / `LLM_VERIFIER_JUDGE_API_KEY_REF`
      - `LLM_VERIFIER_JUDGE_TIMEOUT_MS`
      - `LLM_VERIFIER_JUDGE_FAIL_OPEN`
  - lane-SLO controls:
    - `LLM_SLO_INTERACTIVE_MAX_LATENCY_MS`
    - `LLM_SLO_BATCH_MAX_LATENCY_MS`
    - `LLM_SLO_ALERT_THRESHOLD_PCT`
    - `LLM_SLO_BREACH_ESCALATE_REMOTE`
  - local tier controls:
    - primary local endpoint:
      - `LLM_LOCAL_BASE_URL`
      - `LLM_LOCAL_MODEL`
      - `LLM_LOCAL_API_KEY` / `LLM_LOCAL_API_KEY_REF`
    - optional small local endpoint:
      - `LLM_LOCAL_SMALL_BASE_URL`
      - `LLM_LOCAL_SMALL_MODEL`
      - `LLM_LOCAL_SMALL_API_KEY` / `LLM_LOCAL_SMALL_API_KEY_REF`
    - lane defaults:
      - `LLM_LOCAL_INTERACTIVE_TIER` (`workhorse` or `small`)
      - `LLM_LOCAL_BATCH_TIER` (`workhorse` or `small`)
  - lane telemetry endpoint:
    - `GET /v1/ops/llm-gateway`
- Remote LLM egress defaults to blocked. To enable:
  - set `LLM_REMOTE_EGRESS_ENABLED=1`
  - set explicit `LLM_REMOTE_HOST_ALLOWLIST` entries for allowed remote hosts
- Remote egress class policy:
  - `LLM_REMOTE_EGRESS_CLASS=cloud_allowed` (default): allow remote by route policy + allowlist.
  - `LLM_REMOTE_EGRESS_CLASS=redacted_only`: require `llm.infer` args `redacted=true` for remote routes.
  - `LLM_REMOTE_EGRESS_CLASS=never_leaves_prem`: block all remote routes fail closed.
- Optional remote LLM spend control:
  - set `LLM_REMOTE_TOKEN_BUDGET_PER_RUN` to fail runs that exceed the per-run remote token budget
  - set `LLM_REMOTE_TOKEN_BUDGET_PER_TENANT` to enforce tenant rolling-window remote token caps
  - set `LLM_REMOTE_TOKEN_BUDGET_PER_AGENT` to enforce agent rolling-window remote token caps
  - set `LLM_REMOTE_TOKEN_BUDGET_PER_MODEL` to enforce model rolling-window remote token caps
  - set `LLM_REMOTE_TOKEN_BUDGET_WINDOW_SECS` to control rolling-window duration (default `86400`)
  - set `LLM_REMOTE_TOKEN_BUDGET_SOFT_ALERT_THRESHOLD_PCT` (`1..100`) to emit soft-alert audits when usage nears budget limits
  - set `LLM_REMOTE_COST_PER_1K_TOKENS_USD` to record estimated cost metadata in action results
- Payment rail baseline controls (`payment.send`):
  - `PAYMENT_NWC_ENABLED=1` to allow NWC payment execution path
  - `PAYMENT_NWC_URI` / `PAYMENT_NWC_URI_REF` to enable live NIP-47 relay transport (recommended: `_REF`)
  - `PAYMENT_NWC_WALLET_URIS` / `PAYMENT_NWC_WALLET_URIS_REF` for wallet-id routing (`wallet_id=nwc_uri`, comma/newline or JSON object)
  - optional wildcard wallet map entry (`*=`) defines default routed wallet when a specific id is absent
  - per-wallet multi-route values are supported with `|` separators (`wallet-main=uri_a|uri_b`)
  - `PAYMENT_NWC_ROUTE_STRATEGY` controls route selection (`ordered` default, `deterministic_hash`)
  - `PAYMENT_NWC_ROUTE_FALLBACK_ENABLED` controls failover across alternate routes (`1` default)
  - `PAYMENT_NWC_ROUTE_ROLLOUT_PERCENT` controls canary rollout of multi-route behavior (`100` default, `0` primary-only)
  - `PAYMENT_NWC_ROUTE_HEALTH_FAIL_THRESHOLD` sets consecutive-failure threshold before route quarantine
  - `PAYMENT_NWC_ROUTE_HEALTH_COOLDOWN_SECS` sets route quarantine cooldown duration
  - `PAYMENT_NWC_TIMEOUT_MS` for NIP-47 relay request timeout budget
  - `PAYMENT_MAX_SPEND_MSAT_PER_RUN` to cap per-run satoshi spend
  - `PAYMENT_MAX_SPEND_MSAT_PER_TENANT` to cap aggregate tenant spend
  - `PAYMENT_MAX_SPEND_MSAT_PER_AGENT` to cap aggregate agent spend
  - `PAYMENT_APPROVAL_THRESHOLD_MSAT` to require explicit approval flag for higher-value payout actions
  - `PAYMENT_NWC_MOCK_BALANCE_MSAT` controls mock balance output in local/dev paths
  - Cashu scaffold knobs:
    - `PAYMENT_CASHU_ENABLED`
    - `PAYMENT_CASHU_MINT_URIS` / `PAYMENT_CASHU_MINT_URIS_REF`
    - `PAYMENT_CASHU_DEFAULT_MINT`
    - `PAYMENT_CASHU_TIMEOUT_MS`
    - `PAYMENT_CASHU_MAX_SPEND_MSAT_PER_RUN`
    - `PAYMENT_CASHU_MOCK_ENABLED`
    - `PAYMENT_CASHU_MOCK_BALANCE_MSAT`
    - `PAYMENT_CASHU_HTTP_ENABLED`
    - `PAYMENT_CASHU_HTTP_ALLOW_INSECURE`
    - `PAYMENT_CASHU_AUTH_HEADER`
    - `PAYMENT_CASHU_AUTH_TOKEN` / `PAYMENT_CASHU_AUTH_TOKEN_REF`
  - Cashu rail execution remains fail-closed unless one of these execution modes is enabled:
    - mock mode: `PAYMENT_CASHU_MOCK_ENABLED=1`
    - live HTTP mode: `PAYMENT_CASHU_HTTP_ENABLED=1`
- Current `message.send` connector path always persists outbound payloads to local outbox artifacts (`messages/...`) for traceability.
- Worker artifact writes are tenant-scoped on disk under `<WORKER_ARTIFACT_ROOT>/tenants/<tenant_id>/...` to prevent cross-tenant path collisions on shared hosts.
- Optional connector destination allowlists:
  - `WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST`
  - `WORKER_MESSAGE_SLACK_DEST_ALLOWLIST`
  - when configured, non-allowlisted `message.send` destinations are denied (fail closed).
- Optional worker governance approval gate:
  - `WORKER_APPROVAL_REQUIRED_ACTION_TYPES` (CSV action types, for example `payment.send,message.send`)
  - configured action types require explicit action-level approval flags (`approved=true`; `payment.send` also accepts `payment_approved=true`)
  - missing approval is denied with `reason=approval_required`.
- Optional worker skill provenance gate:
  - `WORKER_SKILL_SCRIPT_SHA256` (sha256 hex digest for the configured skill script path)
  - mismatch fails skill invoke before side effects execute.
- `payment.send` execution persists payment outbox artifacts under `payments/...` plus DB ledger rows in `payment_requests` and `payment_results`.
- Payment reconciliation/reporting baseline:
  - query tenant payment ledger via `GET /v1/payments`
  - query aggregated payment counters via `GET /v1/payments/summary`
  - supports filters: `run_id`, `agent_id`, `status`, `destination`, `idempotency_key`
  - supports summary filters: `window_secs`, `agent_id`, `operation`
  - includes latest payment result/error metadata plus normalized reconciliation fields:
    - `settlement_rail`
    - `normalized_outcome`
    - `normalized_error_code`
    - `normalized_error_class`
- Keep NWC credentials out of run payloads and artifacts:
  - use logical `destination` values (`nwc:<wallet_id>`) in actions
  - configure wallet-connect URI via `PAYMENT_NWC_WALLET_URIS_REF` or `PAYMENT_NWC_URI_REF` on worker hosts
  - inline `nostr+walletconnect://...` destinations are rejected
  - when wallet-map routing is enabled, unknown wallet ids fail closed unless wildcard/default routing is configured
  - when fallback is enabled, route attempts are tracked in action result metadata (`result.nwc.route`)
  - route metadata now includes rollout and health counters (`rollout_limited`, `skipped_unhealthy_count`, `health_*`)
- For approval-gated amounts (`PAYMENT_APPROVAL_THRESHOLD_MSAT`), missing approval causes action failure and run failure by default.
- Use secret references where possible (`*_REF`) instead of raw values:
  - always supported: `env:` and `file:`
  - optional CLI adapters: `vault:`, `aws-sm:`, `gcp-sm:`, `azure-kv:`
  - version pins are supported via query params (`?version=...`, `?version_id=...`, `?version_stage=...` depending on backend)
  - cloud adapters are disabled by default and enabled with `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI=1`
- Secret cache controls:
  - `SECUREAGNT_SECRET_CACHE_TTL_SECS` (default `30`, set `0` to disable caching)
  - `SECUREAGNT_SECRET_CACHE_MAX_ENTRIES` (default `1024`)
  - use lower TTL when rapid rotation pickup is required
- For White Noise destinations, workers publish signed Nostr events when `NOSTR_RELAYS` is configured:
  - `local_key` mode signs locally.
  - `nip46_signer` mode signs through the configured bunker signer.
- For Slack destinations, workers deliver via webhook when `SLACK_WEBHOOK_URL` is configured; failed webhook attempts are recorded and payload remains in local outbox.
- Slack delivery retry controls:
  - `SLACK_MAX_ATTEMPTS` (default `3`)
  - `SLACK_RETRY_BACKOFF_MS` (base backoff; exponential per attempt)
  - exhausted retries move delivery state to `dead_lettered_local_outbox`
- `llm.infer` route policy:
  - local scopes: `local:*` / `local:<model>`
  - remote scopes: `remote:*` / `remote:<model>`
- Run queue-lane prioritization:
  - run input `queue_class`/`llm_queue_class` supports `interactive` and `batch`.
  - worker claim path prioritizes `interactive` while aging old `batch` runs to avoid starvation.
- `llm.infer` action result payload includes `gateway` routing metadata:
  - stable reason code (`gateway.reason_code`)
  - selected route (`gateway.selected_route`)
  - local-tier routing fields:
    - `gateway.local_tier_requested`
    - `gateway.local_tier_selected`
    - `gateway.local_tier_reason_code`
  - request class/lane (`gateway.request_class`, `gateway.queue_lane`)
  - large-input policy/audit fields:
    - `gateway.large_input_policy`
    - `gateway.large_input_applied`
    - `gateway.large_input_reason_code`
    - `gateway.prompt_bytes_original`
    - `gateway.prompt_bytes_effective`
  - retrieval guardrail counters:
    - `gateway.retrieval_candidate_documents`
    - `gateway.retrieval_selected_documents`
  - per-action local tier override:
    - `llm.infer` args `local_tier=workhorse|small`
  - cache/verifier metadata:
    - `gateway.cache_status`
    - `gateway.cache_key_sha256`
    - cache status values include local (`hit`, `miss`), distributed (`distributed_hit`, `distributed_miss`), and fail-open fallback (`distributed_fail_open_local_*`) paths
    - `gateway.verifier_enabled`
    - `gateway.verifier_mode`
    - `gateway.verifier_score_pct`
    - `gateway.verifier_judge_score_pct`
    - `gateway.verifier_threshold_pct`
    - `gateway.verifier_escalated`
    - `gateway.verifier_reason_code`
  - admission metadata:
    - `gateway.admission_status` values include:
      - `admitted`
      - `disabled`
      - `distributed_admitted`
      - `distributed_fail_open_local`
  - configured egress class (`gateway.remote_egress_class`)
  - resolved remote host (`gateway.remote_host`) when remote selected
- Monitor `llm.infer` action result `token_accounting` fields (`consumed_tokens`, `remote_token_budget_remaining`, `estimated_cost_usd`) to track spend and budget pressure.
- Monitor persisted remote usage ledger growth (`llm_token_usage`) and query totals via:
  - `GET /v1/usage/llm/tokens` (role requirement: `owner` or `operator`; `viewer` denied)
- Memory plane baseline controls:
  - write/query endpoints:
    - `POST /v1/memory/records` (`owner`/`operator`)
    - `GET /v1/memory/records` (`owner`/`operator`)
    - `POST /v1/memory/handoff-packets` (`owner`/`operator`)
    - `GET /v1/memory/handoff-packets` (`owner`/`operator`)
    - `GET /v1/memory/retrieve` (`owner`/`operator`)
    - `GET /v1/memory/compactions/stats` (`owner`/`operator`)
  - retention endpoint:
    - `POST /v1/memory/records/purge-expired` (`owner` only)
  - memory writes are redacted before persistence/indexing; monitor `redaction_applied=true` patterns.
  - expired memory records are excluded from memory list/retrieve query paths immediately; purge controls remain responsible for physical deletion.
  - worker compaction controls:
    - `WORKER_MEMORY_COMPACTION_ENABLED`
    - `WORKER_MEMORY_COMPACTION_MIN_RECORDS`
    - `WORKER_MEMORY_COMPACTION_MAX_GROUPS_PER_CYCLE`
    - `WORKER_MEMORY_COMPACTION_MIN_AGE_SECS`
  - memory scopes should remain `memory:`-prefixed and tenant-scoped in operational runbooks/policies.
- Monitor soft-alert audit events (`llm.budget.soft_alert`) to detect near-exhaustion before hard-stop failures.
- Monitor Slack delivery states (`delivered_slack`, `dead_lettered_local_outbox`) and retry metadata in `delivery_context` for alerting and replay workflows.
- Monitor relay publish health by tracking action result fields (`delivery_state`, `accepted_relays`, `published_event_id`) and `action.failed` audits.
- Monitor trigger scheduler health:
  - due trigger lag (`next_fire_at` vs current time)
  - trigger fire ledger growth (`trigger_runs`)
  - in-flight pressure versus trigger limits (`triggers.max_inflight_runs`)
  - webhook trigger queue depth and age (`trigger_events`)
  - repeated trigger dispatch failures/dead-letter events (`trigger_events.status='dead_lettered'`)
  - interval misfire skips (failed `trigger_runs` rows with misfire error metadata)

## Database operations
- Migration ownership: platform migrations manage schema lifecycle.
- Backup strategy: scheduled base backups + WAL archiving (or managed equivalent).
- Restore drill: rehearse point-in-time restore in staging on a schedule.
- Capacity: use connection pooling and monitor saturation indicators.

## Observability
- Metrics:
  - run/step latency
  - action allow/deny and execution status
  - skill failures/timeouts
  - worker queue depth
  - tenant operational summary endpoint for dashboards/soak checks:
    - `GET /v1/ops/summary`
  - tenant run-duration histogram endpoint for latency-distribution monitoring:
    - `GET /v1/ops/latency-histogram`
  - tenant action-latency aggregate endpoint for action-path monitoring:
    - `GET /v1/ops/action-latency`
  - tenant action-latency trace endpoint for per-action regression analysis:
    - `GET /v1/ops/action-latency-traces`
  - tenant latency-traces endpoint for per-run regression analysis:
    - `GET /v1/ops/latency-traces`
- Traces:
  - per-run spans across API -> worker -> action execution
- Logs:
  - structured JSON with correlation IDs (`run_id`, `step_id`, `action_request_id`)

Operational summary query example:
```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:3000/v1/ops/summary?window_secs=3600" | jq .
```

Latency histogram query example:
```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:3000/v1/ops/latency-histogram?window_secs=3600" | jq .
```

Latency traces query example:
```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:3000/v1/ops/latency-traces?window_secs=3600&limit=500" | jq .
```

Action latency query example:
```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:3000/v1/ops/action-latency?window_secs=3600" | jq .
```

Action latency traces query example:
```bash
curl -sS \
  -H "x-tenant-id: single" \
  -H "x-user-role: operator" \
  "http://localhost:3000/v1/ops/action-latency-traces?window_secs=3600&limit=200&action_type=payment.send" | jq .
```

Threshold gate example (non-interactive, exit code `3` on threshold breach):
```bash
cargo run -p agntctl -- ops soak-gate \
  --api-base-url http://localhost:3000 \
  --tenant-id single \
  --user-role operator \
  --window-secs 3600 \
  --max-queued-runs 25 \
  --max-failed-runs-window 5 \
  --max-dead-letter-events-window 0 \
  --max-p95-run-duration-ms 5000 \
  --max-action-p95-ms 1500 \
  --max-action-failed-rate-pct 20 \
  --max-action-denied-rate-pct 30
```

Perf regression gate example (non-interactive, exit code `3` on regression breach):
```bash
cargo run -p agntctl -- ops perf-gate \
  --api-base-url http://localhost:3000 \
  --tenant-id single \
  --user-role operator \
  --window-secs 3600 \
  --baseline-summary-json agntctl/fixtures/ops_summary_ok.json \
  --baseline-histogram-json agntctl/fixtures/ops_latency_histogram_baseline.json \
  --baseline-traces-json agntctl/fixtures/ops_latency_traces_baseline.json \
  --max-p95-regression-ms 250 \
  --max-avg-regression-ms 150 \
  --tail-bucket-lower-ms 5000 \
  --max-tail-regression-pct 25 \
  --max-trace-p99-regression-ms 300 \
  --max-trace-max-regression-ms 1000 \
  --max-trace-top5-avg-regression-ms 400
```

Capture a fresh baseline snapshot from staging API telemetry:
```bash
make capture-perf-baseline
```

Optional controls:
- `AGNTCTL_API_BASE_URL` (default `http://localhost:3000`)
- `AGNTCTL_TENANT_ID` (default `single`)
- `AGNTCTL_USER_ROLE` (default `operator`)
- `WINDOW_SECS` (default `3600`)
- `TRACE_LIMIT` (default `500`)
- `MAX_ACTION_P95_MS` (optional soak gate threshold)
- `MAX_ACTION_FAILED_RATE_PCT` (optional soak gate threshold)
- `MAX_ACTION_DENIED_RATE_PCT` (optional soak gate threshold)
- `SUMMARY_JSON` (optional local fixture for summary soak checks)
- `ACTION_LATENCY_JSON` (optional local fixture for action-latency soak checks)
- `CAPTURE_BASELINE_OUTPUT_DIR` (default `agntctl/fixtures/generated`)
- `CAPTURE_BASELINE_PREFIX` (default `ops_baseline_<utc_timestamp>`)

Pre-release gate workflow:
```bash
make release-gate
```

Notes:
- `make release-gate` runs `make validation-gate` then optional soak checks.
- soak gate is optional by default; set `RELEASE_GATE_SKIP_SOAK=0` to include `make soak-gate`.
- validation DB suite re-run is optional; set `RELEASE_GATE_RUN_DB_SUITES=1` (or `VALIDATION_GATE_RUN_DB_SUITES=1`) to run `make test-db`, `make test-api-db`, and `make test-worker-db`.
  - DB-enabled validation also runs `make isolation-gate` for targeted tenant boundary checks.
- validation coverage gate is optional; set `RELEASE_GATE_RUN_COVERAGE=1` (or `VALIDATION_GATE_RUN_COVERAGE=1`) to run `make coverage`.
- security-gate DB worker checks are opt-in; set `RELEASE_GATE_RUN_DB_SECURITY=1` if you want them included in release-gate runs.
- compliance gate is enabled by default in validation/release gates; set `RELEASE_GATE_RUN_COMPLIANCE=0` (or `VALIDATION_GATE_RUN_COMPLIANCE=0`) to skip.
- governance gate is enabled by default in validation/release gates; set `RELEASE_GATE_RUN_GOVERNANCE=0` (or `VALIDATION_GATE_RUN_GOVERNANCE=0`) to skip.

Validation gate workflow:
```bash
make validation-gate
```

Compliance durability gate workflow:
```bash
make compliance-gate
```

Tenant isolation gate workflow:
```bash
make isolation-gate
```

M7 tenant-hardening sign-off workflow:
```bash
make m7-signoff
```

M8 production-readiness sign-off workflow:
```bash
make m8-signoff
```

M5C payments sign-off workflow:
```bash
make m5c-signoff
```

M6 security-hardening sign-off workflow:
```bash
make m6-signoff
```

M6A durable-memory sign-off workflow:
```bash
make m6a-signoff
```

M8A compliance-plane sign-off workflow:
```bash
make m8a-signoff
```

M9 governance sign-off workflow:
```bash
make m9-signoff
```

M10 cross-platform packaging sign-off workflow:
```bash
make m10-signoff
make m10-matrix-gate
```

Governance supply-chain gate workflow:
```bash
make governance-gate
```

Optional controls:
- `VERIFY_JSON` / `SLO_JSON` for fixture-backed local gate execution
- `TARGETS_JSON` for fixture-backed per-target summary checks
- `MAX_HARD_FAILURE_RATE_PCT`
- `MAX_DEAD_LETTER_RATE_PCT`
- `MAX_OLDEST_PENDING_AGE_SECS`
- `MAX_TARGET_HARD_FAILURE_RATE_PCT`
- `MAX_TARGET_DEAD_LETTER_RATE_PCT`
- `MAX_TARGET_PENDING_COUNT`
- `ALLOW_CHAIN_GAPS=1` (disables chain-verified requirement)

Deployment preflight workflow:
```bash
make release-manifest
make release-manifest-verify
make deploy-preflight
DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 make deploy-preflight
```

Optional controls:
- `RELEASE_MANIFEST_OUTPUT` to change manifest output path
- `RELEASE_MANIFEST_FILES` to override the default artifact file list
- `RELEASE_MANIFEST_INPUT` to verify a non-default manifest path
- `DEPLOY_PREFLIGHT_VERIFY_MANIFEST=1` to enforce manifest verification during preflight
- Record per-OS execution evidence in `docs/M10_EXECUTION_CHECKLIST.md`

Security gate workflow:
```bash
make security-gate
```

Optional controls:
- `RUN_DB_SECURITY=1` to enable DB-backed worker security tests
- `RUN_DB_TESTS=1` also enables DB-backed worker security tests
- `TEST_DATABASE_URL` to override DB target for DB-backed worker security tests

## Audit model
Use two audit planes in enterprise deployments:
- `Operational Audit`:
  - high-volume troubleshooting telemetry
  - includes run/step/action lifecycle and connector transport diagnostics
  - default retention target: 30-day hot + 180-day archive
- `Compliance Audit`:
  - high-trust control/forensics records
  - includes policy/approval decisions, payment events, external side effects, control-plane mutations
  - default retention target: 180-day hot + 7-year archive
  - legal-hold records are non-purgeable until hold removal

Current baseline implementation:
- Compliance-plane routing is DB-backed:
  - table: `compliance_audit_events`
  - source: routed from `audit_events` via trigger classification
  - baseline routed classes: `action.denied`, `action.failed`, and high-risk `action.requested|allowed|executed` for `payment.send`/`message.send`, plus `run.failed`
- API read path:
  - `GET /v1/audit/compliance` (tenant-scoped, owner/operator only)
  - `GET /v1/audit/compliance/verify` (tenant-scoped hash-chain verification summary, owner/operator only)
  - `GET /v1/audit/compliance/policy` (tenant retention/legal-hold policy, owner/operator)
  - `GET /v1/audit/compliance/export` (`application/x-ndjson` export path for batch ingestion)
  - `GET /v1/audit/compliance/siem/export` (adapter-formatted NDJSON for SIEM pipelines)
  - `GET /v1/audit/compliance/siem/deliveries` (delivery queue observability)
  - `GET /v1/audit/compliance/siem/deliveries/summary` (delivery status counters + oldest pending age)
  - `GET /v1/audit/compliance/siem/deliveries/slo` (rolling-window SLO counters/rates)
  - `GET /v1/audit/compliance/siem/deliveries/targets` (delivery status counters grouped by target)
  - `GET /v1/audit/compliance/siem/deliveries/alerts` (threshold-derived target alert rows)
  - `POST /v1/audit/compliance/siem/deliveries` (queues SIEM delivery outbox rows for worker delivery processing)
  - `POST /v1/audit/compliance/siem/deliveries/{id}/replay` (requeues dead-letter rows)
  - `GET /v1/audit/compliance/replay-package` (deterministic incident replay package per run)
- API control path:
  - `PUT /v1/audit/compliance/policy` (owner only)
  - `POST /v1/audit/compliance/purge` (owner only)
- Tamper-evidence baseline:
  - each compliance event stores `tamper_chain_seq`, `tamper_prev_hash`, and `tamper_hash`
  - chain verification function: `verify_compliance_audit_chain(tenant_id)`
- Retention/legal-hold baseline:
  - policy table: `compliance_audit_policies`
  - defaults when no policy row exists:
    - `compliance_hot_retention_days=180`
    - `compliance_archive_retention_days=2555`
    - `legal_hold=false`
  - purge function respects legal hold:
    - `purge_expired_compliance_audit_events(tenant_id, as_of)`
- SIEM delivery outbox baseline:
  - table: `compliance_siem_delivery_outbox`
  - statuses: `pending`, `processing`, `failed`, `delivered`, `dead_lettered`
  - non-retryable delivery failures dead-letter immediately (no retry backoff):
    - HTTP `400`, `401`, `403`, `404`, `405`, `410`, `422`
    - unsupported target/configuration errors
  - worker controls:
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED`
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_BATCH_SIZE`
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_LEASE_MS`
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_BACKOFF_MS`
    - `WORKER_COMPLIANCE_SIEM_DELIVERY_RETRY_JITTER_MAX_MS`
    - `WORKER_COMPLIANCE_SIEM_HTTP_ENABLED`
    - `WORKER_COMPLIANCE_SIEM_HTTP_TIMEOUT_MS`
    - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_HEADER`
    - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN`
    - `WORKER_COMPLIANCE_SIEM_HTTP_AUTH_TOKEN_REF`
  - local validation targets:
    - `mock://success`
    - `mock://fail`
  - operator SLO workflow:
    - use `/v1/audit/compliance/siem/deliveries/slo?window_secs=<n>` for dead-letter and hard-failure rate tracking
    - use `/v1/audit/compliance/siem/deliveries/targets` to pinpoint failing targets before replay
    - use `/v1/audit/compliance/siem/deliveries/alerts?...` to drive threshold-based escalation paths
- Replay package manifest signing:
  - configure key via `COMPLIANCE_REPLAY_SIGNING_KEY` or `COMPLIANCE_REPLAY_SIGNING_KEY_REF`
  - without key, manifest remains deterministic but unsigned
  - runbook rotation procedure: `docs/RUNBOOK.md` section `Compliance replay signing-key rotation`

## Release and change management
- Keep releases small and tagged.
- Update `CHANGELOG.md` for each release.
- Apply migrations before or with compatible service rollout.
- Validate rollback paths for both service binaries and schema changes.

## End-user support basics
- Expose run status and audit retrieval endpoints for support workflows.
- Document allowed capability bundles for common recipes.
- Keep clear operator playbooks for deny-by-default policy overrides.

## Related docs
- `docs/SECURITY.md`
- `docs/THREAT_MODEL.md`
- `docs/POLICY.md`
- `docs/SECRETS.md`
- `docs/PAYMENTS.md`
- `docs/RUNBOOK.md`
- `docs/ARCHITECTURE.md`
- `docs/CROSS_PLATFORM.md`
