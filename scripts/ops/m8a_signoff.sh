#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

run() {
  echo "[m8a-signoff] $*"
  "$@"
}

# Compliance-plane routing/classification + tamper/retention checks
run cargo test -p core --test db_integration compliance_audit_plane_routes_high_risk_events -- --nocapture
run cargo test -p core --test db_integration compliance_audit_tamper_verification_detects_payload_mutation -- --nocapture
run cargo test -p core --test db_integration compliance_audit_purge_respects_retention_and_legal_hold -- --nocapture

# API compliance export/verify/retention checks
run cargo test -p api --test api_integration get_compliance_audit_returns_high_risk_events -- --nocapture
run cargo test -p api --test api_integration get_compliance_audit_export_returns_ndjson -- --nocapture
run cargo test -p api --test api_integration get_compliance_audit_verify_returns_chain_status -- --nocapture
run cargo test -p api --test api_integration post_compliance_audit_purge_respects_legal_hold -- --nocapture
run cargo test -p api --test api_integration get_compliance_audit_siem_export_supports_adapter_formats -- --nocapture

# Runbook guardrail validation (includes incident checklist sections)
run make -C "${REPO_ROOT}" runbook-validate

echo "[m8a-signoff] pass"
