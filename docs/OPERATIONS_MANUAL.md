# SecureAgnt Operations Manual

## 1. Document Intent
This manual is the operator source of truth for running SecureAgnt in production-like environments.

Design goals:
- deterministic operations
- fast incident containment
- auditable change control
- security-first defaults

Use this manual with:
- `docs/OPERATIONS.md` (concise controls summary)
- `docs/RUNBOOK.md` (quick response playbook)
- `docs/API.md` (endpoint contracts)
- `docs/SECURITY.md` (security constraints)
- `docs/ROADMAP.md` (milestone and rollout context)

## 2. Audience
- Platform operators (day-1/day-2 service operations)
- Security and compliance operators (audit, legal hold, retention)
- SRE/incident responders
- Change/release managers

## 3. Operating Model
SecureAgnt runs a split-plane model:
- Control plane: `secureagnt-api`
- Data plane: `secureagntd` worker
- State plane: shared Postgres per environment

Hard boundary:
- only API and worker connect directly to Postgres
- skills and external agents use APIs/protocols only

## 4. Service Topology
### 4.1 Baseline Components
- API service (`secureagnt-api`)
- Worker service (`secureagntd`)
- Postgres
- optional reverse proxy/TLS terminator
- optional object storage for larger artifacts

### 4.2 Filesystem Conventions
- config root: `/etc/secureagnt/`
- config file: `/etc/secureagnt/secureagnt.yaml`
- state root: `/var/lib/secureagnt/`
- logs root: `/var/log/secureagnt/`

### 4.3 Supervisor Templates
- Linux systemd:
  - `infra/systemd/secureagnt.service`
  - `infra/systemd/secureagnt-api.service`
- macOS launchd:
  - `infra/launchd/secureagnt.plist`
  - `infra/launchd/secureagnt-api.plist`

## 5. Environment Strategy
Run separate environments with separate Postgres instances or DBs:
- `dev`
- `staging`
- `prod`

Rules:
- never share credentials across environments
- never promote test data into production
- keep schema migrations environment-local and controlled by release process

## 6. Install and Bootstrap
### 6.1 Container Path (Podman/Docker)
1. Preflight:
```bash
make container-info
```
2. Optional profile selection:
```bash
set -a
source infra/config/profile.solo-dev.env
set +a
```
or:
```bash
set -a
source infra/config/profile.enterprise.env
set +a
```
or (M15 solo-lite scaffold):
```bash
set -a
source infra/config/profile.solo-lite.env
set +a
make solo-lite-init
make solo-lite-smoke
```
Current note: SQLite runtime parity is still in progress:
- API currently runs a scoped SQLite route profile (runs, triggers, memory, payments/usage reporting, ops summary); non-profile routes return `SQLITE_PROFILE_ENDPOINT_UNAVAILABLE`.
- Worker supports SQLite core run-loop parity including scheduler/memory-compaction/compliance-outbox flows.
3. Start Postgres only:
```bash
make db-up
```
4. Start full stack:
```bash
make stack-build
make stack-up
make stack-ps
```
5. Seed baseline data (optional):
```bash
make quickstart-seed
```
6. Follow logs:
```bash
make stack-logs
```

### 6.2 Native Binary Path
1. Build:
```bash
make build
```
2. Start API:
```bash
make secureagnt-api
```
3. Start worker:
```bash
make secureagntd
```
4. Optional systemd enablement:
```bash
sudo cp infra/systemd/secureagnt*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now secureagnt.service secureagnt-api.service
```

## 7. Day-0 Readiness Checklist
Before enabling production traffic, verify all checks:
- database reachable and migrations current
- API process healthy and listening on expected bind
- worker running with expected lease owner id
- trusted proxy auth configured for role/user headers
- secrets references resolved from approved backend
- local-exec disabled unless explicitly needed
- remote LLM egress blocked unless explicitly needed
- remote LLM egress class set intentionally:
  - `cloud_allowed` (default)
  - `redacted_only` (remote requires `llm.infer` args `redacted=true`)
  - `never_leaves_prem` (fail-closed remote denial)
- large-input policy posture set intentionally:
  - `LLM_LARGE_INPUT_POLICY`
  - `LLM_LARGE_INPUT_THRESHOLD_BYTES`
  - `LLM_MAX_INPUT_BYTES`
- admission/cache/verifier posture set intentionally:
  - `LLM_ADMISSION_*`
  - `LLM_CACHE_*`
  - `LLM_VERIFIER_*`
  - local tier posture:
    - `LLM_LOCAL_INTERACTIVE_TIER`
    - `LLM_LOCAL_BATCH_TIER`
    - optional small endpoint (`LLM_LOCAL_SMALL_*`) configured only when intended
  - lane-SLO posture:
    - `LLM_SLO_INTERACTIVE_MAX_LATENCY_MS`
    - `LLM_SLO_BATCH_MAX_LATENCY_MS`
    - `LLM_SLO_ALERT_THRESHOLD_PCT`
    - `LLM_SLO_BREACH_ESCALATE_REMOTE`
  - verifier mode posture:
    - deterministic-only (`LLM_VERIFIER_MODE=deterministic`) for no extra token spend
    - `model_judge`/`hybrid` only when judge endpoint + budget are explicitly planned
  - distributed mode only when needed for multi-worker scale:
    - `LLM_DISTRIBUTED_ENABLED`
    - `LLM_DISTRIBUTED_ADMISSION_*`
    - `LLM_DISTRIBUTED_CACHE_*`
- queue-lane posture reviewed for run traffic shape:
  - run input lane keys (`queue_class` / `llm_queue_class`)
  - expected `interactive` vs `batch` mix and capacity assumptions
- payment rail flags set to intended mode (NWC/Cashu mock/live)
- audit/compliance retention policy reviewed
- release gates and rollback plan validated
- deployment preflight run for current packaging mode:
  - `make deploy-preflight`
  - container mode recommendation: `DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 make deploy-preflight`
  - portability matrix static gate:
    - `make m10-matrix-gate`

## 8. Health Validation Procedure
### 8.1 Service Liveness
- API startup log contains `api started`.
- Worker startup log contains `worker started`.

### 8.2 Control Plane Queries
Use role `operator` and tenant header:
- `GET /v1/ops/summary`
- `GET /v1/ops/latency-histogram`
- `GET /v1/ops/action-latency`
- `GET /v1/ops/llm-gateway`
- `GET /v1/usage/llm/tokens`

### 8.3 Compliance Plane Queries
- `GET /v1/audit/compliance/siem/deliveries/summary`
- `GET /v1/audit/compliance/siem/deliveries/slo`
- `GET /v1/audit/compliance/siem/deliveries/alerts`

### 8.4 Console Validation
- open `/console`
- verify role-restricted and forbidden panel handling
- verify LLM gateway lane panel renders expected lane counters/threshold posture
- verify alert acknowledgment flow works with `x-user-id`
- verify heartbeat materialization controls:
  - preview path renders materialization payload in console panel
  - apply path enforces `x-user-id` + approval confirmation posture
- verify bootstrap controls:
  - `Load Bootstrap` renders `/v1/agents/{id}/bootstrap` status
  - `Complete Bootstrap` enforces `owner` + `x-user-id`

### 8.5 Agent Context Validation
- verify context metadata endpoint returns deterministic checksums:
  - `GET /v1/agents/{agent_id}/context`
- verify bootstrap status endpoint posture:
  - `GET /v1/agents/{agent_id}/bootstrap`
  - status should be:
    - `pending` when `BOOTSTRAP.md` exists and completion has not been recorded
    - `completed` after completion event append
    - `disabled` when `API_AGENT_BOOTSTRAP_ENABLED=0`
- verify bootstrap completion workflow for solo/dev:
  - `POST /v1/agents/{agent_id}/bootstrap/complete`
  - requires `owner` and `x-user-id`
  - confirms completion event append at `sessions/bootstrap.status.jsonl`
- verify heartbeat compile works from profile file:
  - `POST /v1/agents/{agent_id}/heartbeat/compile` with `{}` body
- verify heartbeat materialization workflow:
  - plan-only preview:
    - `POST /v1/agents/{agent_id}/heartbeat/materialize` with `{"apply":false}`
  - governed apply (change window):
    - `POST /v1/agents/{agent_id}/heartbeat/materialize` with `{"apply":true,"approval_confirmed":true}`
    - include `x-user-id` for approval attribution
- verify returned payload includes:
  - `aggregate_sha256`
  - `summary_digest_sha256`
  - mutability classification for listed files
- if context mutation is enabled in your environment, validate guardrails:
  - immutable files are denied
  - `sessions/*.jsonl` requires append mode
  - role restrictions match policy (`owner` vs `operator`)

## 9. Routine Operations Calendar
### 9.1 Daily
- review `ops/summary` and queue pressure
- review SIEM delivery summary and alert count
- review payment failures and dead-letter posture
- review remote token usage against budgets

### 9.2 Weekly
- run isolation/compliance/security gates
- validate backup completion and spot-restore sample
- review trigger growth and disabled/dead-letter trigger states
- rotate ephemeral secrets where required by policy

### 9.3 Monthly
- perform full restore drill in staging
- review retention and legal hold settings
- review capability policies and high-risk action approvals
- refresh performance baseline capture

## 10. SLO and Alert Baseline
Recommended initial operating thresholds:
- queued runs: warn at `>25`, critical at `>100`
- failed runs (window): warn at `>5`, critical at `>20`
- run p95 latency ms: warn at `>5000`, critical at `>30000`
- SIEM hard-failure rate: warn at `>5%`, critical at `>10%`
- SIEM dead-letter rate: warn at `>1%`, critical at `>5%`
- payment failures (window): warn at `>1`, critical at `>5`
- remote token burn (window): alert at `>=80%` configured budget

Tune thresholds per tenant and workload profile.

## 11. Incident Management
### 11.1 Severity Model
- `SEV-1`: data loss, broad outage, security compromise in progress
- `SEV-2`: major degradation, critical workflow blocked
- `SEV-3`: partial degradation, workaround exists
- `SEV-4`: minor defect or operator inconvenience

### 11.2 First 15 Minutes
1. Contain:
   - pause workers or scale to zero
   - disable high-risk action types by policy
2. Preserve:
   - do not purge logs/audit/compliance evidence
   - export compliance/audit windows
3. Scope:
   - identify impacted tenants/run IDs
   - inspect payment and message outbox results
4. Recover:
   - rotate secrets/keys if needed
   - validate controls before resuming workers

## 12. Scenario Playbooks
### 12.1 API Unavailable
- verify process and bind port
- inspect startup errors
- confirm DB connectivity and credentials
- rollback latest change if startup regression detected

### 12.2 Worker Queue Backlog
- inspect `queued_runs` and `running_runs`
- verify worker lease progress
- inspect trigger dispatch rates
- scale worker replicas if safe and policy allows

### 12.3 Trigger Storm
- disable or patch offending trigger(s)
- use in-flight guardrails to prevent runaway run creation
- audit trigger mutation events for source attribution

### 12.4 Postgres Degradation
- check connection saturation and lock pressure
- reduce worker throughput or pause workers
- restore from backup if corruption suspected

### 12.5 Remote LLM Token Budget Exhaustion
- inspect `/v1/usage/llm/tokens`
- lower concurrency or switch to `LLM_MODE=local_only`
- adjust budgets only through change-control
- inspect `llm.infer` action `gateway.reason_code` and `gateway.selected_route` values for escalation/fallback patterns
- inspect local tier routing markers (`gateway.local_tier_requested`, `gateway.local_tier_selected`, `gateway.local_tier_reason_code`) to verify lane defaults/fallback behavior
- inspect `gateway.large_input_*` and retrieval counters when spikes are tied to oversized prompts or repo-ingestion flows

### 12.6 NWC/Cashu Rail Failures
- inspect payment summary and ledger status/error classes
- verify route map and default route configuration
- verify secrets references and auth token validity
- fail closed on uncertain settlement posture

### 12.7 Nostr Signer/Relay Issues
- verify signer mode (`local_key` or `nip46_signer`)
- verify relay reachability and auth
- preserve outbox artifacts for replay

### 12.8 SIEM Delivery Degradation
- inspect summary/slo/alerts endpoints
- replay dead-letter rows only after destination remediation
- acknowledge alerts with `x-user-id` and note for accountability

## 13. Security Operations
### 13.1 Trusted Auth Gateway
Enable trusted proxy auth for role/user header usage:
- `API_TRUSTED_PROXY_AUTH_ENABLED=1`
- set `API_TRUSTED_PROXY_SHARED_SECRET` or `_REF`
- enforce `x-auth-proxy-token` at gateway boundary

### 13.2 Secrets Operations
- prefer `_REF` secret sources over inline values
- maintain secret inventory and rotation cadence
- validate secret resolution on deploy

### 13.3 High-Risk Controls
- keep `local.exec` disabled by default
- keep remote LLM egress disabled by default
- require approvals for irreversible actions where possible
- enforce skill script digest gate in hardened environments

## 14. Audit and Compliance Operations
### 14.1 Compliance Data Plane
Key flows:
- query compliance events
- verify tamper chain
- export for external analysis
- replay package generation for incidents

### 14.2 Retention and Legal Hold
- set tenant policy via compliance policy endpoints
- apply legal hold before investigative export and triage
- avoid purge while investigations are active

### 14.3 SIEM Delivery Ops
- monitor delivery summary, slo, and alerts continuously
- use replay endpoint for dead-letter rows
- use alert ack endpoint to capture response ownership

## 15. Backup, Restore, and Recovery
### 15.1 Backup Policy
- nightly full backup minimum
- encrypted backups with controlled access
- defined retention schedule per compliance requirements

### 15.2 Restore Drills
- run scheduled restore drills in staging
- validate run/audit/payment/compliance integrity after restore
- record RTO and RPO from each drill

### 15.3 Rollback Strategy
- migrations are treated as forward-only
- rollback via database restore + known-good application rollback
- reopen traffic only after explicit validation checks pass

## 16. Capacity and Performance Management
### 16.1 Capacity Signals
- queue depth trends
- run p95 latency
- Postgres utilization and query timings
- outbox backlog for SIEM/message/payment paths

### 16.2 Performance Gates
Use provided tooling:
- `make perf-gate`
- `make soak-gate`
- `make capture-perf-baseline`

Re-baseline only via controlled change process.

## 17. Change and Release Management
### 17.1 Pre-Release Gate
```bash
make release-gate
```

### 17.2 Governance Gate
```bash
make governance-gate
make release-manifest
make release-manifest-verify
make deploy-preflight
```

### 17.3 DB-Backed Verification
```bash
make verify-db
make test-api-db
make test-worker-db
```

### 17.4 Go/No-Go Criteria
Release only if:
- release gate passes
- M10 execution evidence captured in:
  - `docs/M10_EXECUTION_CHECKLIST.md`
- no unresolved critical security findings
- rollback path is validated and documented
- on-call owner is assigned

## 18. Tenant Lifecycle Operations
### 18.1 Tenant Onboarding
- create tenant policy defaults (retention, legal hold false)
- assign capability bundle defaults
- define messaging/payment destination allowlists
- verify audit and ops visibility

### 18.2 Tenant Offboarding
- disable trigger creation and execution
- export required audit/compliance artifacts
- enforce retention/legal requirements before purge
- revoke secrets and external connector permissions

## 19. Configuration Baselines
### 19.1 Worker Controls (minimum hardened)
- `WORKER_LOCAL_EXEC_ENABLED=0`
- `LLM_MODE=local_first` (or `local_only`)
- remote egress disabled unless explicitly needed
- destination allowlists for message sends when in production
- agent-context profile enabled for production identity posture:
  - `WORKER_AGENT_CONTEXT_ENABLED=1`
  - `WORKER_AGENT_CONTEXT_REQUIRED=1`
  - `WORKER_AGENT_CONTEXT_ROOT` points to controlled, read-only profile storage
  - profile path convention:
    - `<root>/<tenant_id>/<agent_id>/`
    - fallback `<root>/<agent_id>/`

### 19.2 API Controls (minimum hardened)
- trusted proxy auth enabled
- tenant capacity guardrails set
- strict role header management only at trusted gateway
- agent-context API loader controls configured for deterministic profile resolution:
  - `API_AGENT_CONTEXT_ROOT`
  - `API_AGENT_CONTEXT_REQUIRED_FILES`
  - `API_AGENT_CONTEXT_MAX_FILE_BYTES`
  - `API_AGENT_CONTEXT_MAX_TOTAL_BYTES`
  - `API_AGENT_CONTEXT_MAX_DYNAMIC_FILES_PER_DIR`
- context mutation endpoint remains disabled unless explicitly needed:
  - `API_AGENT_CONTEXT_MUTATION_ENABLED=0` (default)
  - `API_AGENT_BOOTSTRAP_ENABLED=0` (recommended for controlled enterprise rollouts)
  - if enabled temporarily, enforce owner/operator role policy and monitor changes

### 19.3 Payment Controls (minimum hardened)
- explicit max spend limits
- approval threshold for high-value sends
- route health thresholds enabled

## 20. Command Reference
Common operations:
```bash
make container-info
make db-up
make db-down
make stack-build
make stack-up
make stack-up-build
make stack-ps
make stack-logs
make stack-down
make agent-context-init
make solo-lite-init
make solo-lite-smoke
make build
make verify
make verify-db
make test
make test-db
make test-api-db
make test-worker-db
make runbook-validate
make security-gate
make compliance-gate
make isolation-gate
make validation-gate
make governance-gate
make release-gate
```

## 21. Escalation and Ownership
Define and maintain:
- primary on-call owner
- secondary escalation owner
- security escalation contact
- compliance escalation contact

Store contacts and rotation policy in your internal incident management system.
Use `docs/templates/ESCALATION_ROSTER_TEMPLATE.md` as the canonical source template.

## 22. Manual Maintenance Policy
Update this manual whenever:
- new privileged capability is introduced
- new integration path is released
- new incident class is discovered
- release gate logic changes
- compliance/legal requirements change

Treat documentation updates as part of done criteria for operationally significant milestones.

## 23. Appendix A - Environment Escalation Rosters
### 23.1 Solo/Dev (Single Operator)
| Role | Primary | Secondary | Escalation Trigger |
|---|---|---|---|
| Platform Owner | `<name>` | `<backup>` | any prod-impacting incident |
| Security Contact | `<name>` | `<backup>` | suspected key/secret exposure |
| Compliance Contact | `<name>` | `<backup>` | legal hold / export request |

### 23.2 Team/Self-Hosted (Small Team)
| Role | Primary | Secondary | Escalation Trigger |
|---|---|---|---|
| On-call Engineer | `<name>` | `<name>` | API/worker degradation > 15 min |
| Platform Lead | `<name>` | `<name>` | SEV-1 or repeated SEV-2 incidents |
| Security Lead | `<name>` | `<name>` | auth boundary bypass or secret leak |
| Data/Compliance Owner | `<name>` | `<name>` | tamper-chain verify failure |

### 23.3 Enterprise Production
| Role | Primary | Secondary | Escalation Trigger |
|---|---|---|---|
| NOC / SRE On-call | `<rotation>` | `<rotation>` | uptime/SLO breach |
| Platform Engineering Manager | `<name>` | `<name>` | SEV-1 declaration |
| Security Incident Commander | `<name>` | `<name>` | confirmed compromise indicators |
| Compliance Officer | `<name>` | `<name>` | regulatory disclosure threshold |
| Business Owner | `<name>` | `<name>` | customer-impacting outage > SLA |

### 23.4 Roster Governance
- version rosters with change history and effective date
- review and re-acknowledge roster ownership monthly
- validate all escalation paths during quarterly incident drills

## 24. Appendix B - Change Ticket Templates
Canonical templates are available in:
- `docs/templates/CHANGE_TICKET_TEMPLATE.md`

### 24.1 Minimum Fields (All Changes)
- ticket id + owner + reviewer
- environment scope (`dev|staging|prod`)
- affected services (`api`, `worker`, `db`, connectors)
- rollout window + rollback window
- validation commands + success criteria
- security/compliance impact statement

### 24.2 Standard Planned Change Checklist
1. pre-change snapshot captured (ops summary, SIEM SLO, token usage)
2. required approvals recorded
3. rollout sequence documented
4. rollback sequence validated
5. post-change validation evidence attached

### 24.3 Emergency Change Checklist
1. incident id linked
2. emergency approver documented
3. blast-radius assessment recorded
4. temporary controls and expiry documented
5. retroactive review scheduled within one business day
