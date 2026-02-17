# Operations Guide

This is a living document for deploying and operating Aegis.

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
- Use one standardized app schema per environment (for example `aegis`).
- Do not create Postgres instances or schemas per agent as a default model.
- Only `api` and `worker` connect to Postgres directly.
- Skills and agents interact through platform APIs/protocols only.

## Security baseline
- API behind TLS reverse proxy.
- Private network access from `api`/`worker` to Postgres.
- Worker/skill runtime outbound egress deny-by-default.
- Secrets in Vault/KMS, never exposed to skills.
- Structured logs with redaction.
- Skills launched by worker run with cleared environments by default; only allowlisted env vars are passed (`WORKER_SKILL_ENV_ALLOWLIST`).
- API role presets (`x-user-role`) are currently header-driven; production deployments should set/override this only at trusted auth gateway boundaries.
- Worker trigger scheduler is enabled by default (`WORKER_TRIGGER_SCHEDULER_ENABLED=1`) and dispatches due interval triggers into queued runs.

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
  - set `LLM_REMOTE_COST_PER_1K_TOKENS_USD` to record estimated cost metadata in action results
- Current `message.send` connector path always persists outbound payloads to local outbox artifacts (`messages/...`) for traceability.
- Use secret references where possible (`*_REF`) instead of raw values:
  - currently supported: `env:` and `file:`
  - planned backends: Vault, AWS, Google Cloud, Azure
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
- Monitor Slack delivery states (`delivered_slack`, `dead_lettered_local_outbox`) and retry metadata in `delivery_context` for alerting and replay workflows.
- Monitor relay publish health by tracking action result fields (`delivery_state`, `accepted_relays`, `published_event_id`) and `action.failed` audits.
- Monitor trigger scheduler health:
  - due trigger lag (`next_fire_at` vs current time)
  - trigger fire ledger growth (`trigger_runs`)
  - repeated trigger dispatch failures/dead-letter events

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
- `docs/RUNBOOK.md`
- `docs/ARCHITECTURE.md`
