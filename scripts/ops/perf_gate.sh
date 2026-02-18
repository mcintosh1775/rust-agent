#!/usr/bin/env bash
set -euo pipefail

API_BASE_URL="${API_BASE_URL:-http://localhost:3000}"
TENANT_ID="${TENANT_ID:-single}"
USER_ROLE="${USER_ROLE:-operator}"
WINDOW_SECS="${WINDOW_SECS:-3600}"

BASELINE_SUMMARY_JSON="${BASELINE_SUMMARY_JSON:-agntctl/fixtures/ops_summary_ok.json}"
BASELINE_HISTOGRAM_JSON="${BASELINE_HISTOGRAM_JSON:-agntctl/fixtures/ops_latency_histogram_baseline.json}"
BASELINE_TRACES_JSON="${BASELINE_TRACES_JSON:-agntctl/fixtures/ops_latency_traces_baseline.json}"
CANDIDATE_SUMMARY_JSON="${CANDIDATE_SUMMARY_JSON:-}"
CANDIDATE_HISTOGRAM_JSON="${CANDIDATE_HISTOGRAM_JSON:-}"
CANDIDATE_TRACES_JSON="${CANDIDATE_TRACES_JSON:-}"
TRACE_LIMIT="${TRACE_LIMIT:-500}"

MAX_P95_REGRESSION_MS="${MAX_P95_REGRESSION_MS:-250}"
MAX_AVG_REGRESSION_MS="${MAX_AVG_REGRESSION_MS:-150}"
TAIL_BUCKET_LOWER_MS="${TAIL_BUCKET_LOWER_MS:-5000}"
MAX_TAIL_REGRESSION_PCT="${MAX_TAIL_REGRESSION_PCT:-25}"
MAX_TRACE_P99_REGRESSION_MS="${MAX_TRACE_P99_REGRESSION_MS:-300}"
MAX_TRACE_MAX_REGRESSION_MS="${MAX_TRACE_MAX_REGRESSION_MS:-1000}"
MAX_TRACE_TOP5_AVG_REGRESSION_MS="${MAX_TRACE_TOP5_AVG_REGRESSION_MS:-400}"
REQUIRE_DURATION_METRICS="${REQUIRE_DURATION_METRICS:-0}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required for perf gate automation" >&2
  exit 1
fi

if [[ ! -f "${BASELINE_SUMMARY_JSON}" ]]; then
  echo "baseline summary json not found: ${BASELINE_SUMMARY_JSON}" >&2
  exit 1
fi

if [[ ! -f "${BASELINE_HISTOGRAM_JSON}" ]]; then
  echo "baseline histogram json not found: ${BASELINE_HISTOGRAM_JSON}" >&2
  exit 1
fi

if [[ ! -f "${BASELINE_TRACES_JSON}" ]]; then
  echo "baseline traces json not found: ${BASELINE_TRACES_JSON}" >&2
  exit 1
fi

cargo build -p agntctl >/dev/null
AGNTCTL_BIN="${AGNTCTL_BIN:-./target/debug/agntctl}"

if [[ ! -x "${AGNTCTL_BIN}" ]]; then
  echo "agntctl binary not found or not executable: ${AGNTCTL_BIN}" >&2
  exit 1
fi

cmd=(
  "${AGNTCTL_BIN}" ops perf-gate
  --api-base-url "${API_BASE_URL}"
  --tenant-id "${TENANT_ID}"
  --user-role "${USER_ROLE}"
  --window-secs "${WINDOW_SECS}"
  --baseline-summary-json "${BASELINE_SUMMARY_JSON}"
  --baseline-histogram-json "${BASELINE_HISTOGRAM_JSON}"
  --baseline-traces-json "${BASELINE_TRACES_JSON}"
  --trace-limit "${TRACE_LIMIT}"
  --max-p95-regression-ms "${MAX_P95_REGRESSION_MS}"
  --max-avg-regression-ms "${MAX_AVG_REGRESSION_MS}"
  --tail-bucket-lower-ms "${TAIL_BUCKET_LOWER_MS}"
  --max-tail-regression-pct "${MAX_TAIL_REGRESSION_PCT}"
  --max-trace-p99-regression-ms "${MAX_TRACE_P99_REGRESSION_MS}"
  --max-trace-max-regression-ms "${MAX_TRACE_MAX_REGRESSION_MS}"
  --max-trace-top5-avg-regression-ms "${MAX_TRACE_TOP5_AVG_REGRESSION_MS}"
)

if [[ -n "${CANDIDATE_SUMMARY_JSON}" ]]; then
  cmd+=(--candidate-summary-json "${CANDIDATE_SUMMARY_JSON}")
fi

if [[ -n "${CANDIDATE_HISTOGRAM_JSON}" ]]; then
  cmd+=(--candidate-histogram-json "${CANDIDATE_HISTOGRAM_JSON}")
fi

if [[ -n "${CANDIDATE_TRACES_JSON}" ]]; then
  cmd+=(--candidate-traces-json "${CANDIDATE_TRACES_JSON}")
fi

if [[ "${REQUIRE_DURATION_METRICS}" == "1" ]]; then
  cmd+=(--require-duration-metrics)
fi

"${cmd[@]}"
