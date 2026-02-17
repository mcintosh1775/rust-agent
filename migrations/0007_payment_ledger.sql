CREATE TABLE IF NOT EXISTS payment_requests (
  id uuid PRIMARY KEY,
  action_request_id uuid NOT NULL UNIQUE REFERENCES action_requests(id) ON DELETE CASCADE,
  run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  tenant_id text NOT NULL,
  agent_id uuid NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  provider text NOT NULL,
  operation text NOT NULL,
  destination text NOT NULL,
  idempotency_key text NOT NULL,
  amount_msat bigint,
  request_json jsonb NOT NULL,
  status text NOT NULL DEFAULT 'requested',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT payment_requests_idempotency_key_not_empty CHECK (char_length(idempotency_key) > 0),
  CONSTRAINT payment_requests_provider_check CHECK (provider IN ('nwc')),
  CONSTRAINT payment_requests_operation_check CHECK (operation IN ('pay_invoice', 'make_invoice', 'get_balance')),
  CONSTRAINT payment_requests_amount_non_negative CHECK (amount_msat IS NULL OR amount_msat >= 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_payment_requests_tenant_idempotency
  ON payment_requests (tenant_id, idempotency_key);

CREATE INDEX IF NOT EXISTS idx_payment_requests_run
  ON payment_requests (run_id, created_at DESC);

CREATE TABLE IF NOT EXISTS payment_results (
  id uuid PRIMARY KEY,
  payment_request_id uuid NOT NULL REFERENCES payment_requests(id) ON DELETE CASCADE,
  status text NOT NULL,
  result_json jsonb,
  error_json jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_payment_results_request_created
  ON payment_results (payment_request_id, created_at DESC, id DESC);
