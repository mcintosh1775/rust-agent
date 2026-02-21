CREATE TABLE IF NOT EXISTS llm_gateway_admission_leases (
  namespace text NOT NULL,
  lane text NOT NULL,
  slot_index integer NOT NULL,
  lease_id uuid NOT NULL,
  lease_owner text NOT NULL,
  lease_expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (namespace, lane, slot_index),
  CONSTRAINT llm_gateway_admission_lane_check CHECK (lane IN ('interactive', 'batch')),
  CONSTRAINT llm_gateway_admission_slot_positive CHECK (slot_index > 0)
);

CREATE INDEX IF NOT EXISTS idx_llm_gateway_admission_active
  ON llm_gateway_admission_leases (namespace, lane, lease_expires_at DESC);

CREATE TABLE IF NOT EXISTS llm_gateway_cache_entries (
  cache_key_sha256 text PRIMARY KEY,
  namespace text NOT NULL,
  route text NOT NULL,
  model text NOT NULL,
  response_json jsonb NOT NULL,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT llm_gateway_cache_route_check CHECK (route IN ('local', 'remote'))
);

CREATE INDEX IF NOT EXISTS idx_llm_gateway_cache_namespace_updated
  ON llm_gateway_cache_entries (namespace, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_llm_gateway_cache_expires
  ON llm_gateway_cache_entries (expires_at);
