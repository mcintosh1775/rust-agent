ALTER TABLE triggers
  ADD COLUMN IF NOT EXISTS jitter_seconds integer NOT NULL DEFAULT 0;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conname = 'triggers_jitter_seconds_check'
      AND conrelid = 'triggers'::regclass
  ) THEN
    ALTER TABLE triggers
      ADD CONSTRAINT triggers_jitter_seconds_check
      CHECK (jitter_seconds >= 0 AND jitter_seconds <= 3600);
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS scheduler_leases (
  lease_name text PRIMARY KEY,
  lease_owner text NOT NULL,
  lease_expires_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL DEFAULT now()
);
