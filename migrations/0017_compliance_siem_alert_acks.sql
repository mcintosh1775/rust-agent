CREATE TABLE IF NOT EXISTS compliance_siem_delivery_alert_acks (
  id uuid PRIMARY KEY,
  tenant_id text NOT NULL,
  run_scope text NOT NULL,
  delivery_target text NOT NULL,
  acknowledged_by_user_id uuid NOT NULL,
  acknowledged_by_role text NOT NULL CHECK (acknowledged_by_role IN ('owner', 'operator')),
  note text,
  created_at timestamptz NOT NULL DEFAULT now(),
  acknowledged_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, run_scope, delivery_target)
);

CREATE INDEX IF NOT EXISTS idx_compliance_siem_delivery_alert_acks_tenant_scope_ack
  ON compliance_siem_delivery_alert_acks (tenant_id, run_scope, acknowledged_at DESC, id DESC);
