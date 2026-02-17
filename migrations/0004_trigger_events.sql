ALTER TABLE triggers
  ADD COLUMN IF NOT EXISTS misfire_policy text NOT NULL DEFAULT 'fire_now',
  ADD COLUMN IF NOT EXISTS max_attempts integer NOT NULL DEFAULT 3,
  ADD COLUMN IF NOT EXISTS consecutive_failures integer NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS dead_lettered_at timestamptz,
  ADD COLUMN IF NOT EXISTS dead_letter_reason text,
  ADD COLUMN IF NOT EXISTS webhook_secret_ref text;

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conname = 'triggers_trigger_type_check'
      AND conrelid = 'triggers'::regclass
  ) THEN
    ALTER TABLE triggers DROP CONSTRAINT triggers_trigger_type_check;
  END IF;
END $$;

ALTER TABLE triggers
  ADD CONSTRAINT triggers_trigger_type_check
  CHECK (trigger_type IN ('interval', 'webhook'));

ALTER TABLE triggers
  ALTER COLUMN interval_seconds DROP NOT NULL;

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conname = 'triggers_interval_seconds_check'
      AND conrelid = 'triggers'::regclass
  ) THEN
    ALTER TABLE triggers DROP CONSTRAINT triggers_interval_seconds_check;
  END IF;
END $$;

ALTER TABLE triggers
  ADD CONSTRAINT triggers_interval_seconds_check
  CHECK (
    (trigger_type = 'interval' AND interval_seconds IS NOT NULL AND interval_seconds > 0)
    OR (trigger_type = 'webhook' AND interval_seconds IS NULL)
  );

ALTER TABLE triggers
  ADD CONSTRAINT triggers_misfire_policy_check
  CHECK (misfire_policy IN ('fire_now', 'skip'));

ALTER TABLE triggers
  ADD CONSTRAINT triggers_max_attempts_check
  CHECK (max_attempts > 0);

ALTER TABLE triggers
  ADD CONSTRAINT triggers_consecutive_failures_check
  CHECK (consecutive_failures >= 0);

CREATE TABLE IF NOT EXISTS trigger_events (
  id uuid PRIMARY KEY,
  trigger_id uuid NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  tenant_id text NOT NULL,
  event_id text NOT NULL,
  payload_json jsonb NOT NULL,
  status text NOT NULL CHECK (status IN ('pending', 'processed', 'dead_lettered')),
  attempts integer NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  last_error_json jsonb,
  created_at timestamptz NOT NULL DEFAULT now(),
  processed_at timestamptz,
  dead_lettered_at timestamptz
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_trigger_events_trigger_event_id
  ON trigger_events (trigger_id, event_id);

CREATE INDEX IF NOT EXISTS idx_trigger_events_due
  ON trigger_events (status, next_attempt_at, created_at);
