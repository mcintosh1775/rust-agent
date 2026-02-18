#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

run() {
  echo "[m7-signoff] $*"
  "$@"
}

# API tenant-isolation and tenant-quota coverage
run cargo test -p api --test api_integration run_and_audit_endpoints_are_tenant_isolated -- --nocapture
run cargo test -p api --test api_integration trigger_mutation_endpoints_are_tenant_isolated -- --nocapture
run cargo test -p api --test api_integration memory_retrieve_enforces_scope_role_and_tenant_isolation -- --nocapture
run cargo test -p api --test api_integration compliance_endpoints_are_tenant_isolated -- --nocapture
run cargo test -p api --test api_integration create_memory_record_enforces_tenant_memory_capacity_limit -- --nocapture

# Worker artifact/message tenant isolation coverage
run cargo test -p worker --test worker_integration worker_process_once_isolates_artifacts_by_tenant -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_isolates_message_outbox_by_tenant -- --nocapture

echo "[m7-signoff] pass"
