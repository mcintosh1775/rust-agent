ALTER TABLE trigger_events
  ADD COLUMN IF NOT EXISTS semantic_dedupe_key text;

CREATE UNIQUE INDEX IF NOT EXISTS idx_trigger_events_trigger_semantic_dedupe_active
  ON trigger_events (trigger_id, semantic_dedupe_key)
  WHERE semantic_dedupe_key IS NOT NULL;
