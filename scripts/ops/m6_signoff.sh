#!/usr/bin/env bash
set -euo pipefail

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

run() {
  echo "[m6-signoff] $*"
  "$@"
}

# Core policy and redaction invariants
run cargo test -p core policy::tests -- --nocapture
run cargo test -p core redaction::tests -- --nocapture

# Skillrunner containment boundaries
run cargo test -p skillrunner --test runner_integration invoke_scrubs_env_by_default_and_supports_allowlist -- --nocapture
run cargo test -p skillrunner --test runner_integration invoke_timeout_kills_skill -- --nocapture

# Worker deny/containment/redaction boundaries
run cargo test -p worker --test worker_integration worker_process_once_denies_out_of_scope_action_and_fails_run -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_blocks_whitenoise_message_send_when_target_not_allowlisted -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_redacts_sensitive_message_payloads_in_db -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_executes_local_exec_template_with_scoped_roots -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_fails_local_exec_for_out_of_scope_path -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_denies_llm_remote_when_only_local_scope_granted -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_blocks_llm_remote_when_egress_disabled -- --nocapture

echo "[m6-signoff] pass"
