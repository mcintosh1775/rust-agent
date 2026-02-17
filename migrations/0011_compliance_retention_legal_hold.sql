CREATE TABLE IF NOT EXISTS compliance_audit_policies (
  tenant_id text PRIMARY KEY,
  compliance_hot_retention_days integer NOT NULL DEFAULT 180,
  compliance_archive_retention_days integer NOT NULL DEFAULT 2555,
  legal_hold boolean NOT NULL DEFAULT false,
  legal_hold_reason text,
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT compliance_hot_retention_days_check CHECK (compliance_hot_retention_days > 0),
  CONSTRAINT compliance_archive_retention_days_check CHECK (compliance_archive_retention_days > 0),
  CONSTRAINT compliance_archive_not_less_than_hot
    CHECK (compliance_archive_retention_days >= compliance_hot_retention_days)
);

CREATE OR REPLACE FUNCTION purge_expired_compliance_audit_events(
  p_tenant_id text,
  p_as_of timestamptz DEFAULT now()
) RETURNS TABLE (
  tenant_id text,
  deleted_count bigint,
  legal_hold boolean,
  cutoff_at timestamptz,
  compliance_hot_retention_days integer,
  compliance_archive_retention_days integer
) AS $$
DECLARE
  v_hot_retention_days integer := 180;
  v_archive_retention_days integer := 2555;
  v_legal_hold boolean := false;
  v_cutoff timestamptz;
  v_deleted_count bigint := 0;
BEGIN
  SELECT COALESCE(policy.compliance_hot_retention_days, 180),
         COALESCE(policy.compliance_archive_retention_days, 2555),
         COALESCE(policy.legal_hold, false)
  INTO v_hot_retention_days,
       v_archive_retention_days,
       v_legal_hold
  FROM (SELECT 1) seed
  LEFT JOIN compliance_audit_policies policy
    ON policy.tenant_id = p_tenant_id;

  v_cutoff := p_as_of - make_interval(days => v_hot_retention_days);

  IF v_legal_hold THEN
    RETURN QUERY
      SELECT p_tenant_id,
             0::bigint,
             true,
             v_cutoff,
             v_hot_retention_days,
             v_archive_retention_days;
    RETURN;
  END IF;

  DELETE FROM compliance_audit_events events
  WHERE events.tenant_id = p_tenant_id
    AND events.created_at < v_cutoff;

  GET DIAGNOSTICS v_deleted_count = ROW_COUNT;

  RETURN QUERY
    SELECT p_tenant_id,
           v_deleted_count,
           false,
           v_cutoff,
           v_hot_retention_days,
           v_archive_retention_days;
END;
$$ LANGUAGE plpgsql;
