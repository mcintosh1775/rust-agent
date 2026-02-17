ALTER TABLE triggers
  ADD COLUMN IF NOT EXISTS cron_expression text,
  ADD COLUMN IF NOT EXISTS schedule_timezone text NOT NULL DEFAULT 'UTC',
  ADD COLUMN IF NOT EXISTS max_inflight_runs integer NOT NULL DEFAULT 1;

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
  CHECK (trigger_type IN ('interval', 'webhook', 'cron'));

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
    (trigger_type = 'interval' AND interval_seconds IS NOT NULL AND interval_seconds > 0 AND cron_expression IS NULL)
    OR (trigger_type = 'webhook' AND interval_seconds IS NULL AND cron_expression IS NULL)
    OR (trigger_type = 'cron' AND interval_seconds IS NULL AND cron_expression IS NOT NULL)
  );

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conname = 'triggers_max_inflight_runs_check'
      AND conrelid = 'triggers'::regclass
  ) THEN
    ALTER TABLE triggers
      ADD CONSTRAINT triggers_max_inflight_runs_check
      CHECK (max_inflight_runs > 0);
  END IF;
END $$;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conname = 'triggers_schedule_timezone_check'
      AND conrelid = 'triggers'::regclass
  ) THEN
    ALTER TABLE triggers
      ADD CONSTRAINT triggers_schedule_timezone_check
      CHECK (char_length(schedule_timezone) > 0);
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS trigger_audit_events (
  id uuid PRIMARY KEY,
  trigger_id uuid NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
  tenant_id text NOT NULL,
  actor text NOT NULL,
  event_type text NOT NULL,
  payload_json jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_trigger_audit_events_trigger_created
  ON trigger_audit_events (trigger_id, created_at DESC, id DESC);

