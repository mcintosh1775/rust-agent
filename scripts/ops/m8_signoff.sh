#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

run() {
  echo "[m8-signoff] $*"
  "$@"
}

# API ops telemetry endpoints and role guardrails
run cargo test -p api --test api_integration get_ops_summary_returns_counts_and_enforces_role -- --nocapture
run cargo test -p api --test api_integration get_ops_latency_histogram_returns_bucket_counts_and_enforces_role -- --nocapture
run cargo test -p api --test api_integration get_ops_latency_traces_returns_recent_run_durations_and_enforces_role -- --nocapture
run cargo test -p api --test api_integration get_ops_action_latency_returns_action_metrics_and_enforces_role -- --nocapture
run cargo test -p api --test api_integration get_ops_action_latency_traces_returns_recent_actions_and_enforces_role -- --nocapture

# Runbook checklist validation
run make -C "${REPO_ROOT}" runbook-validate

# Fixture-backed perf and soak threshold gates
run env \
  BASELINE_SUMMARY_JSON="agntctl/fixtures/ops_summary_ok.json" \
  BASELINE_HISTOGRAM_JSON="agntctl/fixtures/ops_latency_histogram_baseline.json" \
  BASELINE_TRACES_JSON="agntctl/fixtures/ops_latency_traces_baseline.json" \
  CANDIDATE_SUMMARY_JSON="agntctl/fixtures/ops_summary_candidate_ok.json" \
  CANDIDATE_HISTOGRAM_JSON="agntctl/fixtures/ops_latency_histogram_candidate_ok.json" \
  CANDIDATE_TRACES_JSON="agntctl/fixtures/ops_latency_traces_candidate_ok.json" \
  make -C "${REPO_ROOT}" perf-gate

run env \
  ITERATIONS=1 \
  SLEEP_SECS=0 \
  SUMMARY_JSON="agntctl/fixtures/ops_summary_candidate_ok.json" \
  ACTION_LATENCY_JSON="agntctl/fixtures/ops_action_latency_candidate_ok.json" \
  MAX_QUEUED_RUNS=25 \
  MAX_FAILED_RUNS_WINDOW=5 \
  MAX_DEAD_LETTER_EVENTS_WINDOW=0 \
  MAX_P95_RUN_DURATION_MS=5000 \
  MAX_ACTION_P95_MS=1500 \
  MAX_ACTION_FAILED_RATE_PCT=5 \
  MAX_ACTION_DENIED_RATE_PCT=10 \
  make -C "${REPO_ROOT}" soak-gate

echo "[m8-signoff] pass"
