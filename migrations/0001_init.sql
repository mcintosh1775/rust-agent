CREATE TABLE IF NOT EXISTS agents (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  name text NOT NULL,
  status text NOT NULL CHECK (status IN ('active', 'disabled')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, name)
);

CREATE TABLE IF NOT EXISTS users (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  external_subject text,
  display_name text,
  status text NOT NULL CHECK (status IN ('active', 'disabled')),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_tenant_external_subject
  ON users (tenant_id, external_subject)
  WHERE external_subject IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_users_tenant_created_at ON users (tenant_id, created_at);

CREATE TABLE IF NOT EXISTS runs (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  agent_id uuid NOT NULL REFERENCES agents(id),
  triggered_by_user_id uuid REFERENCES users(id),
  recipe_id text NOT NULL,
  status text NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
  input_json jsonb NOT NULL,
  requested_capabilities jsonb NOT NULL,
  granted_capabilities jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  started_at timestamptz,
  finished_at timestamptz,
  error_json jsonb
);

CREATE INDEX IF NOT EXISTS idx_runs_status_created_at ON runs (status, created_at);
CREATE INDEX IF NOT EXISTS idx_runs_tenant_agent_created_at ON runs (tenant_id, agent_id, created_at);
CREATE INDEX IF NOT EXISTS idx_runs_tenant_user_created_at ON runs (tenant_id, triggered_by_user_id, created_at);

CREATE TABLE IF NOT EXISTS steps (
  id uuid PRIMARY KEY,
  run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  tenant_id text NOT NULL,
  agent_id uuid NOT NULL REFERENCES agents(id),
  user_id uuid REFERENCES users(id),
  name text NOT NULL,
  status text NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'skipped')),
  input_json jsonb NOT NULL,
  output_json jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  started_at timestamptz,
  finished_at timestamptz,
  error_json jsonb
);

CREATE INDEX IF NOT EXISTS idx_steps_run_id ON steps (run_id);
CREATE INDEX IF NOT EXISTS idx_steps_tenant_agent_started_at ON steps (tenant_id, agent_id, started_at);
CREATE INDEX IF NOT EXISTS idx_steps_status ON steps (status);

CREATE TABLE IF NOT EXISTS artifacts (
  id uuid PRIMARY KEY,
  run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  path text NOT NULL,
  content_type text NOT NULL,
  size_bytes bigint NOT NULL,
  checksum text,
  storage_ref text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_artifacts_run_id ON artifacts (run_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_artifacts_run_path ON artifacts (run_id, path);

CREATE TABLE IF NOT EXISTS action_requests (
  id uuid PRIMARY KEY,
  step_id uuid NOT NULL REFERENCES steps(id) ON DELETE CASCADE,
  action_type text NOT NULL,
  args_json jsonb NOT NULL,
  justification text,
  status text NOT NULL CHECK (status IN ('requested', 'allowed', 'denied', 'executed', 'failed')),
  decision_reason text,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_action_requests_step_id ON action_requests (step_id);
CREATE INDEX IF NOT EXISTS idx_action_requests_status_created_at ON action_requests (status, created_at);

CREATE TABLE IF NOT EXISTS action_results (
  id uuid PRIMARY KEY,
  action_request_id uuid NOT NULL REFERENCES action_requests(id) ON DELETE CASCADE,
  status text NOT NULL CHECK (status IN ('executed', 'failed', 'denied')),
  result_json jsonb,
  error_json jsonb,
  executed_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_action_results_action_request_id ON action_results (action_request_id);

CREATE TABLE IF NOT EXISTS audit_events (
  id uuid PRIMARY KEY,
  run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  step_id uuid REFERENCES steps(id) ON DELETE SET NULL,
  tenant_id text NOT NULL,
  agent_id uuid REFERENCES agents(id),
  user_id uuid REFERENCES users(id),
  actor text NOT NULL,
  event_type text NOT NULL,
  payload_json jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_audit_events_run_created_at ON audit_events (run_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_tenant_agent_created_at ON audit_events (tenant_id, agent_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_tenant_user_created_at ON audit_events (tenant_id, user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_events_event_type_created_at ON audit_events (event_type, created_at);
