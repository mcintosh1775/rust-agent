CREATE INDEX IF NOT EXISTS idx_runs_tenant_status_created_at
  ON runs (tenant_id, status, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_trigger_events_tenant_status_due
  ON trigger_events (tenant_id, status, next_attempt_at, created_at);

CREATE INDEX IF NOT EXISTS idx_payment_requests_tenant_created
  ON payment_requests (tenant_id, created_at DESC, id DESC);
