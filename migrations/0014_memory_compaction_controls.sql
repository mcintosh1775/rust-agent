ALTER TABLE memory_records
  ADD COLUMN IF NOT EXISTS compacted_at timestamptz;

CREATE INDEX IF NOT EXISTS idx_memory_records_tenant_compacted_created
  ON memory_records (tenant_id, compacted_at, created_at DESC, id DESC);
