CREATE TABLE IF NOT EXISTS compliance_audit_events (
  id uuid PRIMARY KEY,
  source_audit_event_id uuid NOT NULL UNIQUE REFERENCES audit_events(id) ON DELETE CASCADE,
  run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  step_id uuid REFERENCES steps(id) ON DELETE SET NULL,
  tenant_id text NOT NULL,
  agent_id uuid REFERENCES agents(id),
  user_id uuid REFERENCES users(id),
  actor text NOT NULL,
  event_type text NOT NULL,
  payload_json jsonb NOT NULL,
  created_at timestamptz NOT NULL,
  recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_compliance_audit_events_tenant_created
  ON compliance_audit_events (tenant_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_compliance_audit_events_run_created
  ON compliance_audit_events (run_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_compliance_audit_events_event_type_created
  ON compliance_audit_events (event_type, created_at, id);

CREATE OR REPLACE FUNCTION should_route_to_compliance_audit(
  event_type text,
  payload jsonb
) RETURNS boolean AS $$
BEGIN
  IF event_type IN ('action.denied', 'action.failed') THEN
    RETURN true;
  END IF;

  IF event_type IN ('action.requested', 'action.allowed', 'action.executed')
     AND payload->>'action_type' IN ('payment.send', 'message.send')
  THEN
    RETURN true;
  END IF;

  IF event_type = 'run.failed' THEN
    RETURN true;
  END IF;

  RETURN false;
END;
$$ LANGUAGE plpgsql IMMUTABLE;

CREATE OR REPLACE FUNCTION route_audit_event_to_compliance()
RETURNS trigger AS $$
BEGIN
  IF should_route_to_compliance_audit(NEW.event_type, NEW.payload_json) THEN
    INSERT INTO compliance_audit_events (
      id,
      source_audit_event_id,
      run_id,
      step_id,
      tenant_id,
      agent_id,
      user_id,
      actor,
      event_type,
      payload_json,
      created_at
    )
    VALUES (
      NEW.id,
      NEW.id,
      NEW.run_id,
      NEW.step_id,
      NEW.tenant_id,
      NEW.agent_id,
      NEW.user_id,
      NEW.actor,
      NEW.event_type,
      NEW.payload_json,
      NEW.created_at
    )
    ON CONFLICT (source_audit_event_id) DO NOTHING;
  END IF;

  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_route_audit_event_to_compliance ON audit_events;

CREATE TRIGGER trg_route_audit_event_to_compliance
AFTER INSERT ON audit_events
FOR EACH ROW
EXECUTE FUNCTION route_audit_event_to_compliance();
