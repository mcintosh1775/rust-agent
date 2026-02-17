CREATE TABLE IF NOT EXISTS memory_records (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  agent_id uuid NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  run_id uuid REFERENCES runs(id) ON DELETE SET NULL,
  step_id uuid REFERENCES steps(id) ON DELETE SET NULL,
  memory_kind text NOT NULL CHECK (memory_kind IN ('session', 'semantic', 'procedural', 'handoff')),
  scope text NOT NULL CHECK (char_length(scope) > 0),
  content_json jsonb NOT NULL,
  summary_text text,
  source text NOT NULL DEFAULT 'worker',
  redaction_applied boolean NOT NULL DEFAULT false,
  expires_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_agent_kind_created
  ON memory_records (tenant_id, agent_id, memory_kind, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_scope_created
  ON memory_records (tenant_id, scope, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_expires
  ON memory_records (tenant_id, expires_at)
  WHERE expires_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS memory_compactions (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  agent_id uuid REFERENCES agents(id) ON DELETE CASCADE,
  memory_kind text NOT NULL CHECK (memory_kind IN ('session', 'semantic', 'procedural', 'handoff')),
  scope text NOT NULL CHECK (char_length(scope) > 0),
  source_count integer NOT NULL CHECK (source_count > 0),
  source_entry_ids jsonb NOT NULL,
  summary_json jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_memory_compactions_tenant_agent_kind_created
  ON memory_compactions (tenant_id, agent_id, memory_kind, created_at DESC, id DESC);

CREATE OR REPLACE FUNCTION purge_expired_memory_records(
  p_tenant_id text,
  p_as_of timestamptz
)
RETURNS TABLE(
  tenant_id text,
  deleted_count bigint,
  as_of timestamptz
) AS $$
DECLARE
  v_deleted bigint;
BEGIN
  DELETE FROM memory_records
  WHERE memory_records.tenant_id = p_tenant_id
    AND memory_records.expires_at IS NOT NULL
    AND memory_records.expires_at <= p_as_of;

  GET DIAGNOSTICS v_deleted = ROW_COUNT;

  RETURN QUERY
  SELECT p_tenant_id, v_deleted, p_as_of;
END;
$$ LANGUAGE plpgsql;
