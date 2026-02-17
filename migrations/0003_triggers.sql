CREATE TABLE IF NOT EXISTS triggers (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  agent_id uuid NOT NULL REFERENCES agents(id),
  triggered_by_user_id uuid REFERENCES users(id),
  recipe_id text NOT NULL,
  status text NOT NULL CHECK (status IN ('enabled', 'disabled')),
  trigger_type text NOT NULL CHECK (trigger_type IN ('interval')),
  interval_seconds bigint NOT NULL CHECK (interval_seconds > 0),
  input_json jsonb NOT NULL,
  requested_capabilities jsonb NOT NULL,
  granted_capabilities jsonb NOT NULL,
  next_fire_at timestamptz NOT NULL,
  last_fired_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_triggers_due
  ON triggers (status, trigger_type, next_fire_at);

CREATE INDEX IF NOT EXISTS idx_triggers_tenant_status
  ON triggers (tenant_id, status, created_at);

CREATE TABLE IF NOT EXISTS trigger_runs (
  id uuid PRIMARY KEY,
  trigger_id uuid NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  run_id uuid REFERENCES runs(id) ON DELETE SET NULL,
  scheduled_for timestamptz NOT NULL,
  status text NOT NULL CHECK (status IN ('created', 'duplicate', 'failed')),
  dedupe_key text NOT NULL,
  error_json jsonb,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_trigger_runs_trigger_dedupe
  ON trigger_runs (trigger_id, dedupe_key);

CREATE INDEX IF NOT EXISTS idx_trigger_runs_run_id
  ON trigger_runs (run_id);
