CREATE TABLE IF NOT EXISTS agents (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE (tenant_id, name)
);

CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  external_subject TEXT,
  display_name TEXT,
  status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_tenant_external_subject
  ON users (tenant_id, external_subject);

CREATE INDEX IF NOT EXISTS idx_users_tenant_created_at
  ON users (tenant_id, created_at);

CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  triggered_by_user_id TEXT REFERENCES users(id),
  recipe_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
  input_json TEXT NOT NULL,
  requested_capabilities TEXT NOT NULL,
  granted_capabilities TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  started_at TEXT,
  finished_at TEXT,
  error_json TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  lease_owner TEXT,
  lease_expires_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_status_created_at
  ON runs (status, created_at);
CREATE INDEX IF NOT EXISTS idx_runs_tenant_agent_created_at
  ON runs (tenant_id, agent_id, created_at);
CREATE INDEX IF NOT EXISTS idx_runs_tenant_user_created_at
  ON runs (tenant_id, triggered_by_user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_runs_tenant_status_created_at
  ON runs (tenant_id, status, created_at, id);
CREATE INDEX IF NOT EXISTS idx_runs_queue_claim
  ON runs (status, lease_expires_at, created_at);
CREATE INDEX IF NOT EXISTS idx_runs_lease_owner
  ON runs (lease_owner, lease_expires_at);

CREATE TABLE IF NOT EXISTS steps (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  tenant_id TEXT NOT NULL,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  user_id TEXT REFERENCES users(id),
  name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'skipped')),
  input_json TEXT NOT NULL,
  output_json TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  started_at TEXT,
  finished_at TEXT,
  error_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_steps_run_id ON steps (run_id);
CREATE INDEX IF NOT EXISTS idx_steps_tenant_agent_started_at ON steps (tenant_id, agent_id, started_at);
CREATE INDEX IF NOT EXISTS idx_steps_status ON steps (status);

CREATE TABLE IF NOT EXISTS artifacts (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  content_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  checksum TEXT,
  storage_ref TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_artifacts_run_id ON artifacts (run_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_artifacts_run_path ON artifacts (run_id, path);

CREATE TABLE IF NOT EXISTS action_requests (
  id TEXT PRIMARY KEY,
  step_id TEXT NOT NULL REFERENCES steps(id) ON DELETE CASCADE,
  action_type TEXT NOT NULL,
  args_json TEXT NOT NULL,
  justification TEXT,
  status TEXT NOT NULL CHECK (status IN ('requested', 'allowed', 'denied', 'executed', 'failed')),
  decision_reason TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_action_requests_step_id ON action_requests (step_id);
CREATE INDEX IF NOT EXISTS idx_action_requests_status_created_at ON action_requests (status, created_at);

CREATE TABLE IF NOT EXISTS action_results (
  id TEXT PRIMARY KEY,
  action_request_id TEXT NOT NULL UNIQUE REFERENCES action_requests(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('executed', 'failed', 'denied')),
  result_json TEXT,
  error_json TEXT,
  executed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_action_results_action_request_id ON action_results (action_request_id);

CREATE TABLE IF NOT EXISTS audit_events (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  step_id TEXT REFERENCES steps(id) ON DELETE SET NULL,
  tenant_id TEXT NOT NULL,
  agent_id TEXT REFERENCES agents(id),
  user_id TEXT REFERENCES users(id),
  actor TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_audit_events_run_created_at ON audit_events (run_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_tenant_agent_created_at ON audit_events (tenant_id, agent_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_tenant_user_created_at ON audit_events (tenant_id, user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_event_type_created_at ON audit_events (event_type, created_at);

CREATE TABLE IF NOT EXISTS triggers (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  agent_id TEXT NOT NULL REFERENCES agents(id),
  triggered_by_user_id TEXT REFERENCES users(id),
  recipe_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('enabled', 'disabled')),
  trigger_type TEXT NOT NULL CHECK (trigger_type IN ('interval', 'webhook', 'cron')),
  interval_seconds INTEGER,
  cron_expression TEXT,
  schedule_timezone TEXT NOT NULL DEFAULT 'UTC',
  misfire_policy TEXT NOT NULL DEFAULT 'fire_now' CHECK (misfire_policy IN ('fire_now', 'skip')),
  max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
  max_inflight_runs INTEGER NOT NULL DEFAULT 1 CHECK (max_inflight_runs > 0),
  jitter_seconds INTEGER NOT NULL DEFAULT 0 CHECK (jitter_seconds >= 0 AND jitter_seconds <= 3600),
  consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
  dead_lettered_at TEXT,
  dead_letter_reason TEXT,
  webhook_secret_ref TEXT,
  input_json TEXT NOT NULL,
  requested_capabilities TEXT NOT NULL,
  granted_capabilities TEXT NOT NULL,
  next_fire_at TEXT NOT NULL,
  last_fired_at TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_triggers_due ON triggers (status, trigger_type, next_fire_at);
CREATE INDEX IF NOT EXISTS idx_triggers_tenant_status ON triggers (tenant_id, status, created_at);

CREATE TABLE IF NOT EXISTS trigger_runs (
  id TEXT PRIMARY KEY,
  trigger_id TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
  scheduled_for TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('created', 'duplicate', 'failed')),
  dedupe_key TEXT NOT NULL,
  error_json TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_trigger_runs_trigger_dedupe ON trigger_runs (trigger_id, dedupe_key);
CREATE INDEX IF NOT EXISTS idx_trigger_runs_run_id ON trigger_runs (run_id);

CREATE TABLE IF NOT EXISTS trigger_events (
  id TEXT PRIMARY KEY,
  trigger_id TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  tenant_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'processed', 'dead_lettered')),
  attempts INTEGER NOT NULL DEFAULT 0,
  next_attempt_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  last_error_json TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  processed_at TEXT,
  dead_lettered_at TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_trigger_events_trigger_event_id ON trigger_events (trigger_id, event_id);
CREATE INDEX IF NOT EXISTS idx_trigger_events_due ON trigger_events (status, next_attempt_at, created_at);
CREATE INDEX IF NOT EXISTS idx_trigger_events_tenant_status_due ON trigger_events (tenant_id, status, next_attempt_at, created_at);

CREATE TABLE IF NOT EXISTS trigger_audit_events (
  id TEXT PRIMARY KEY,
  trigger_id TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  tenant_id TEXT NOT NULL,
  actor TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_trigger_audit_events_trigger_created
  ON trigger_audit_events (trigger_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS scheduler_leases (
  lease_name TEXT PRIMARY KEY,
  lease_owner TEXT NOT NULL,
  lease_expires_at TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS payment_requests (
  id TEXT PRIMARY KEY,
  action_request_id TEXT NOT NULL UNIQUE REFERENCES action_requests(id) ON DELETE CASCADE,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  tenant_id TEXT NOT NULL,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  provider TEXT NOT NULL CHECK (provider IN ('nwc', 'cashu')),
  operation TEXT NOT NULL CHECK (operation IN ('pay_invoice', 'make_invoice', 'get_balance')),
  destination TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  amount_msat INTEGER,
  request_json TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'requested',
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_payment_requests_tenant_idempotency
  ON payment_requests (tenant_id, idempotency_key);
CREATE INDEX IF NOT EXISTS idx_payment_requests_run ON payment_requests (run_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_payment_requests_tenant_created ON payment_requests (tenant_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS payment_results (
  id TEXT PRIMARY KEY,
  payment_request_id TEXT NOT NULL REFERENCES payment_requests(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  result_json TEXT,
  error_json TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_payment_results_request_created
  ON payment_results (payment_request_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS llm_token_usage (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  action_request_id TEXT NOT NULL UNIQUE REFERENCES action_requests(id) ON DELETE CASCADE,
  tenant_id TEXT NOT NULL,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  route TEXT NOT NULL CHECK (route IN ('local', 'remote')),
  model_key TEXT NOT NULL,
  consumed_tokens INTEGER NOT NULL CHECK (consumed_tokens >= 0),
  estimated_cost_usd REAL,
  window_started_at TEXT NOT NULL,
  window_duration_seconds INTEGER NOT NULL CHECK (window_duration_seconds > 0),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_llm_token_usage_tenant_created ON llm_token_usage (tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_llm_token_usage_tenant_agent_created ON llm_token_usage (tenant_id, agent_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_llm_token_usage_tenant_model_created ON llm_token_usage (tenant_id, model_key, created_at DESC);

CREATE TABLE IF NOT EXISTS compliance_audit_events (
  id TEXT PRIMARY KEY,
  source_audit_event_id TEXT NOT NULL UNIQUE REFERENCES audit_events(id) ON DELETE CASCADE,
  tamper_chain_seq INTEGER,
  tamper_prev_hash TEXT,
  tamper_hash TEXT,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  step_id TEXT REFERENCES steps(id) ON DELETE SET NULL,
  tenant_id TEXT NOT NULL,
  agent_id TEXT REFERENCES agents(id),
  user_id TEXT REFERENCES users(id),
  actor TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  request_id TEXT,
  session_id TEXT,
  action_request_id TEXT,
  payment_request_id TEXT,
  created_at TEXT NOT NULL,
  recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_compliance_audit_events_tenant_created
  ON compliance_audit_events (tenant_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_compliance_audit_events_run_created
  ON compliance_audit_events (run_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_compliance_audit_events_event_type_created
  ON compliance_audit_events (event_type, created_at, id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_compliance_audit_events_tenant_chain_seq
  ON compliance_audit_events (tenant_id, tamper_chain_seq);

CREATE TABLE IF NOT EXISTS compliance_audit_policies (
  tenant_id TEXT PRIMARY KEY,
  compliance_hot_retention_days INTEGER NOT NULL DEFAULT 180 CHECK (compliance_hot_retention_days > 0),
  compliance_archive_retention_days INTEGER NOT NULL DEFAULT 2555 CHECK (compliance_archive_retention_days > 0),
  legal_hold INTEGER NOT NULL DEFAULT 0 CHECK (legal_hold IN (0, 1)),
  legal_hold_reason TEXT,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS compliance_siem_delivery_outbox (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
  adapter TEXT NOT NULL CHECK (adapter IN ('secureagnt_ndjson', 'splunk_hec', 'elastic_bulk')),
  delivery_target TEXT NOT NULL,
  content_type TEXT NOT NULL DEFAULT 'application/x-ndjson',
  payload_ndjson TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'failed', 'delivered', 'dead_lettered')),
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
  next_attempt_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  leased_by TEXT,
  lease_expires_at TEXT,
  last_error TEXT,
  last_http_status INTEGER,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  delivered_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_compliance_siem_delivery_outbox_status_next_attempt
  ON compliance_siem_delivery_outbox (status, next_attempt_at, created_at, id);
CREATE INDEX IF NOT EXISTS idx_compliance_siem_delivery_outbox_tenant_created
  ON compliance_siem_delivery_outbox (tenant_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS compliance_siem_delivery_alert_acks (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  run_scope TEXT NOT NULL,
  delivery_target TEXT NOT NULL,
  acknowledged_by_user_id TEXT NOT NULL,
  acknowledged_by_role TEXT NOT NULL CHECK (acknowledged_by_role IN ('owner', 'operator')),
  note TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  acknowledged_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE (tenant_id, run_scope, delivery_target)
);

CREATE INDEX IF NOT EXISTS idx_compliance_siem_delivery_alert_acks_tenant_scope_ack
  ON compliance_siem_delivery_alert_acks (tenant_id, run_scope, acknowledged_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS memory_records (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
  step_id TEXT REFERENCES steps(id) ON DELETE SET NULL,
  memory_kind TEXT NOT NULL CHECK (memory_kind IN ('session', 'semantic', 'procedural', 'handoff')),
  scope TEXT NOT NULL CHECK (length(scope) > 0),
  content_json TEXT NOT NULL,
  summary_text TEXT,
  source TEXT NOT NULL DEFAULT 'worker',
  redaction_applied INTEGER NOT NULL DEFAULT 0 CHECK (redaction_applied IN (0, 1)),
  expires_at TEXT,
  compacted_at TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_agent_kind_created
  ON memory_records (tenant_id, agent_id, memory_kind, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_scope_created
  ON memory_records (tenant_id, scope, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_expires
  ON memory_records (tenant_id, expires_at);
CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_compacted_created
  ON memory_records (tenant_id, compacted_at, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS memory_compactions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  agent_id TEXT REFERENCES agents(id) ON DELETE CASCADE,
  memory_kind TEXT NOT NULL CHECK (memory_kind IN ('session', 'semantic', 'procedural', 'handoff')),
  scope TEXT NOT NULL CHECK (length(scope) > 0),
  source_count INTEGER NOT NULL CHECK (source_count > 0),
  source_entry_ids TEXT NOT NULL,
  summary_json TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_memory_compactions_tenant_agent_kind_created
  ON memory_compactions (tenant_id, agent_id, memory_kind, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS llm_gateway_admission_leases (
  namespace TEXT NOT NULL,
  lane TEXT NOT NULL CHECK (lane IN ('interactive', 'batch')),
  slot_index INTEGER NOT NULL CHECK (slot_index > 0),
  lease_id TEXT NOT NULL,
  lease_owner TEXT NOT NULL,
  lease_expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (namespace, lane, slot_index)
);

CREATE INDEX IF NOT EXISTS idx_llm_gateway_admission_active
  ON llm_gateway_admission_leases (namespace, lane, lease_expires_at DESC);

CREATE TABLE IF NOT EXISTS llm_gateway_cache_entries (
  cache_key_sha256 TEXT PRIMARY KEY,
  namespace TEXT NOT NULL,
  route TEXT NOT NULL CHECK (route IN ('local', 'remote')),
  model TEXT NOT NULL,
  response_json TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_llm_gateway_cache_namespace_updated
  ON llm_gateway_cache_entries (namespace, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_llm_gateway_cache_expires
  ON llm_gateway_cache_entries (expires_at);

ALTER TABLE runs
  ADD COLUMN semantic_dedupe_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_runs_tenant_semantic_dedupe_active
  ON runs (tenant_id, semantic_dedupe_key)
  WHERE status IN ('queued', 'running');

ALTER TABLE trigger_events
  ADD COLUMN semantic_dedupe_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_trigger_events_trigger_semantic_dedupe_active
  ON trigger_events (trigger_id, semantic_dedupe_key)
  WHERE semantic_dedupe_key IS NOT NULL;
