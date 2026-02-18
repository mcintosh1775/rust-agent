#!/usr/bin/env bash
set -euo pipefail

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

run() {
  echo "[m5c-signoff] $*"
  "$@"
}

# API payment bundle and ledger visibility coverage
run cargo test -p api --test api_integration create_run_payments_bundle_grants_payment_send -- --nocapture
run cargo test -p api --test api_integration create_run_payments_cashu_bundle_grants_cashu_scope -- --nocapture
run cargo test -p api --test api_integration get_payments_returns_tenant_scoped_ledger_with_latest_result -- --nocapture
run cargo test -p api --test api_integration get_payment_summary_returns_counts_and_spend -- --nocapture

# DB idempotency lineage coverage
run cargo test -p core --test db_integration payment_request_idempotency_returns_existing_request -- --nocapture
run cargo test -p core --test db_integration payment_request_idempotency_key_is_scoped_by_tenant -- --nocapture

# Worker allow/deny + budget + replay + NWC transport coverage
run cargo test -p worker --test worker_integration worker_process_once_executes_payment_send_with_nwc_mock -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_denies_payment_send_without_capability -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_blocks_payment_send_when_run_budget_exceeded -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_blocks_payment_send_when_tenant_budget_exceeded -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_blocks_payment_send_when_agent_budget_exceeded -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_reuses_payment_result_on_idempotent_replay -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_executes_payment_send_with_nwc_relay -- --nocapture

echo "[m5c-signoff] pass"
