ALTER TABLE compliance_audit_events
  ADD COLUMN IF NOT EXISTS tamper_chain_seq bigint,
  ADD COLUMN IF NOT EXISTS tamper_prev_hash text,
  ADD COLUMN IF NOT EXISTS tamper_hash text;

CREATE OR REPLACE FUNCTION compute_compliance_tamper_hash(
  prev_hash text,
  event_id uuid,
  source_event_id uuid,
  run_id uuid,
  step_id uuid,
  tenant_id text,
  agent_id uuid,
  user_id uuid,
  actor text,
  event_type text,
  payload jsonb,
  created_at timestamptz
) RETURNS text AS $$
  SELECT md5(
    concat_ws(
      '|',
      COALESCE(prev_hash, ''),
      event_id::text,
      source_event_id::text,
      run_id::text,
      COALESCE(step_id::text, ''),
      tenant_id,
      COALESCE(agent_id::text, ''),
      COALESCE(user_id::text, ''),
      actor,
      event_type,
      payload::text,
      created_at::text
    )
  );
$$ LANGUAGE sql IMMUTABLE;

DO $$
DECLARE
  tenant_record record;
  event_record record;
  previous_hash text;
  current_hash text;
  current_seq bigint;
BEGIN
  FOR tenant_record IN
    SELECT DISTINCT tenant_id
    FROM compliance_audit_events
    ORDER BY tenant_id
  LOOP
    previous_hash := NULL;
    current_seq := 0;

    FOR event_record IN
      SELECT id,
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
      FROM compliance_audit_events
      WHERE tenant_id = tenant_record.tenant_id
      ORDER BY created_at ASC, id ASC
    LOOP
      current_seq := current_seq + 1;
      current_hash := compute_compliance_tamper_hash(
        previous_hash,
        event_record.id,
        event_record.source_audit_event_id,
        event_record.run_id,
        event_record.step_id,
        event_record.tenant_id,
        event_record.agent_id,
        event_record.user_id,
        event_record.actor,
        event_record.event_type,
        event_record.payload_json,
        event_record.created_at
      );

      UPDATE compliance_audit_events
      SET tamper_chain_seq = current_seq,
          tamper_prev_hash = previous_hash,
          tamper_hash = current_hash
      WHERE id = event_record.id;

      previous_hash := current_hash;
    END LOOP;
  END LOOP;
END;
$$;

ALTER TABLE compliance_audit_events
  ALTER COLUMN tamper_chain_seq SET NOT NULL,
  ALTER COLUMN tamper_hash SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_compliance_audit_events_tenant_chain_seq
  ON compliance_audit_events (tenant_id, tamper_chain_seq);

CREATE OR REPLACE FUNCTION route_audit_event_to_compliance()
RETURNS trigger AS $$
DECLARE
  previous_hash text;
  previous_seq bigint;
  current_hash text;
BEGIN
  IF should_route_to_compliance_audit(NEW.event_type, NEW.payload_json) THEN
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.tenant_id, 0));

    SELECT tamper_hash, tamper_chain_seq
    INTO previous_hash, previous_seq
    FROM compliance_audit_events
    WHERE tenant_id = NEW.tenant_id
    ORDER BY tamper_chain_seq DESC
    LIMIT 1;

    current_hash := compute_compliance_tamper_hash(
      previous_hash,
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
    );

    INSERT INTO compliance_audit_events (
      id,
      source_audit_event_id,
      tamper_chain_seq,
      tamper_prev_hash,
      tamper_hash,
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
      COALESCE(previous_seq, 0) + 1,
      previous_hash,
      current_hash,
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

CREATE OR REPLACE FUNCTION verify_compliance_audit_chain(p_tenant_id text)
RETURNS TABLE (
  tenant_id text,
  checked_events bigint,
  verified boolean,
  first_invalid_event_id uuid,
  latest_chain_seq bigint,
  latest_tamper_hash text
) AS $$
WITH ordered AS (
  SELECT id,
         source_audit_event_id,
         run_id,
         step_id,
         tenant_id,
         agent_id,
         user_id,
         actor,
         event_type,
         payload_json,
         created_at,
         tamper_chain_seq,
         tamper_prev_hash,
         tamper_hash,
         lag(tamper_hash) OVER (ORDER BY tamper_chain_seq ASC) AS expected_prev_hash,
         row_number() OVER (ORDER BY tamper_chain_seq ASC) AS expected_seq
  FROM compliance_audit_events
  WHERE tenant_id = p_tenant_id
),
validated AS (
  SELECT id,
         tamper_chain_seq,
         tamper_hash,
         (
           tamper_chain_seq = expected_seq
           AND COALESCE(tamper_prev_hash, '') = COALESCE(expected_prev_hash, '')
           AND tamper_hash = compute_compliance_tamper_hash(
             tamper_prev_hash,
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
         ) AS is_valid
  FROM ordered
),
summary AS (
  SELECT COUNT(*)::bigint AS checked_events,
         COALESCE(bool_and(is_valid), true) AS verified,
         (array_agg(id ORDER BY tamper_chain_seq ASC) FILTER (WHERE NOT is_valid))[1] AS first_invalid_event_id,
         MAX(tamper_chain_seq) AS latest_chain_seq,
         (array_agg(tamper_hash ORDER BY tamper_chain_seq DESC))[1] AS latest_tamper_hash
  FROM validated
)
SELECT p_tenant_id,
       checked_events,
       verified,
       first_invalid_event_id,
       latest_chain_seq,
       latest_tamper_hash
FROM summary;
$$ LANGUAGE sql STABLE;
