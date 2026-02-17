# Operations Guide

This is a living document for deploying and operating SecureAgnt.

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

## Security baseline
- API behind TLS reverse proxy.
- Private network access from `api`/`worker` to Postgres.
- Worker/skill runtime outbound egress deny-by-default.
- Secrets in Vault/KMS, never exposed to skills.
- Structured logs with redaction.
- Skills launched by worker run with cleared environments by default; only allowlisted env vars are passed (`WORKER_SKILL_ENV_ALLOWLIST`).
- Legacy skill marker emission is configurable via `WORKER_SKILL_EMIT_LEGACY_AEGIS_MARKER` (`1` default, set `0` to stop emitting `AEGIS_SKILL_SANDBOXED`).
- API role presets (`x-user-role`) are currently header-driven; production deployments should set/override this only at trusted auth gateway boundaries.
- Trigger mutation endpoints are role-restricted:
  - `owner`/`operator` can create/update/enable/disable/manual-fire triggers
  - `viewer` is denied (`403`) on trigger mutation routes
  - operator-trigger mutations require `x-user-id`; operators are restricted to their own trigger ownership
- Worker trigger scheduler is enabled by default (`WORKER_TRIGGER_SCHEDULER_ENABLED=1`) and dispatches:
  - due interval triggers
  - due cron triggers
  - due webhook trigger events from the trigger event queue
  - with tenant in-flight guardrail `WORKER_TRIGGER_TENANT_MAX_INFLIGHT_RUNS` (default `100`)
  - optional lease gate for HA scheduler coordination:
    - `WORKER_TRIGGER_SCHEDULER_LEASE_ENABLED` (default `1`)
    - `WORKER_TRIGGER_SCHEDULER_LEASE_NAME`
    - `WORKER_TRIGGER_SCHEDULER_LEASE_TTL_MS`

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
- Keep `local.exec` disabled unless needed (`WORKER_LOCAL_EXEC_ENABLED=0` by default). When enabled, use minimal read/write root allowlists.
- Keep `LLM_MODE=local_first` (or `local_only`) unless remote routing is explicitly needed.
- Remote LLM egress defaults to blocked. To enable:
  - set `LLM_REMOTE_EGRESS_ENABLED=1`
  - set explicit `LLM_REMOTE_HOST_ALLOWLIST` entries for allowed remote hosts
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
- Current `message.send` connector path always persists outbound payloads to local outbox artifacts (`messages/...`) for traceability.
- `payment.send` execution persists payment outbox artifacts under `payments/...` plus DB ledger rows in `payment_requests` and `payment_results`.
- Payment reconciliation/reporting baseline:
  - query tenant payment ledger via `GET /v1/payments`
  - query aggregated payment counters via `GET /v1/payments/summary`
  - supports filters: `run_id`, `agent_id`, `status`, `destination`, `idempotency_key`
  - supports summary filters: `window_secs`, `agent_id`, `operation`
  - includes latest payment result/error metadata for settlement verification workflows
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
  - migration compatibility: `AEGIS_SECRET_ENABLE_CLOUD_CLI` is accepted until `2026-06-30` and planned for removal on `2026-07-01`
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
- Monitor `llm.infer` action result `token_accounting` fields (`consumed_tokens`, `remote_token_budget_remaining`, `estimated_cost_usd`) to track spend and budget pressure.
- Monitor persisted remote usage ledger growth (`llm_token_usage`) and query totals via:
  - `GET /v1/usage/llm/tokens` (role requirement: `owner` or `operator`; `viewer` denied)
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
- Traces:
  - per-run spans across API -> worker -> action execution
- Logs:
  - structured JSON with correlation IDs (`run_id`, `step_id`, `action_request_id`)

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
- `docs/RUNBOOK.md`
- `docs/ARCHITECTURE.md`
