ALTER TABLE runs
  ADD COLUMN IF NOT EXISTS attempts integer NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS lease_owner text,
  ADD COLUMN IF NOT EXISTS lease_expires_at timestamptz;

CREATE INDEX IF NOT EXISTS idx_runs_queue_claim
  ON runs (status, lease_expires_at, created_at);

CREATE INDEX IF NOT EXISTS idx_runs_lease_owner
  ON runs (lease_owner, lease_expires_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_action_results_unique_action_request
  ON action_results (action_request_id);
