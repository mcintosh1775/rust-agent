ALTER TABLE runs
  ADD COLUMN semantic_dedupe_key TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_runs_tenant_semantic_dedupe_active
  ON runs (tenant_id, semantic_dedupe_key)
  WHERE status IN ('queued', 'running');
