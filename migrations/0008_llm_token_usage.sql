CREATE TABLE IF NOT EXISTS llm_token_usage (
  id uuid PRIMARY KEY,
  run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  action_request_id uuid NOT NULL UNIQUE REFERENCES action_requests(id) ON DELETE CASCADE,
  tenant_id text NOT NULL,
  agent_id uuid NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  route text NOT NULL,
  model_key text NOT NULL,
  consumed_tokens bigint NOT NULL,
  estimated_cost_usd double precision,
  window_started_at timestamptz NOT NULL,
  window_duration_seconds bigint NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT llm_token_usage_route_check CHECK (route IN ('local', 'remote')),
  CONSTRAINT llm_token_usage_consumed_non_negative CHECK (consumed_tokens >= 0),
  CONSTRAINT llm_token_usage_window_positive CHECK (window_duration_seconds > 0)
);

CREATE INDEX IF NOT EXISTS idx_llm_token_usage_tenant_created
  ON llm_token_usage (tenant_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_llm_token_usage_tenant_agent_created
  ON llm_token_usage (tenant_id, agent_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_llm_token_usage_tenant_model_created
  ON llm_token_usage (tenant_id, model_key, created_at DESC);
