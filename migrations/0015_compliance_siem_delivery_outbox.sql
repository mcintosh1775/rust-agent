CREATE TABLE IF NOT EXISTS compliance_siem_delivery_outbox (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  run_id uuid REFERENCES runs(id) ON DELETE SET NULL,
  adapter text NOT NULL CHECK (adapter IN ('secureagnt_ndjson', 'splunk_hec', 'elastic_bulk')),
  delivery_target text NOT NULL,
  content_type text NOT NULL DEFAULT 'application/x-ndjson',
  payload_ndjson text NOT NULL,
  status text NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'processing', 'failed', 'delivered', 'dead_lettered')),
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  max_attempts integer NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  leased_by text,
  lease_expires_at timestamptz,
  last_error text,
  last_http_status integer,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  delivered_at timestamptz
);

CREATE INDEX IF NOT EXISTS idx_compliance_siem_delivery_outbox_status_next_attempt
  ON compliance_siem_delivery_outbox (status, next_attempt_at, created_at, id);

CREATE INDEX IF NOT EXISTS idx_compliance_siem_delivery_outbox_tenant_created
  ON compliance_siem_delivery_outbox (tenant_id, created_at DESC, id DESC);
