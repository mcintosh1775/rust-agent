#!/usr/bin/env bash
set -euo pipefail

API_BASE_URL="${API_BASE_URL:-http://localhost:3000}"
TENANT_ID="${TENANT_ID:-single}"
USER_ROLE="${USER_ROLE:-operator}"
WINDOW_SECS="${WINDOW_SECS:-3600}"
ITERATIONS="${ITERATIONS:-30}"
SLEEP_SECS="${SLEEP_SECS:-60}"

MAX_QUEUED_RUNS="${MAX_QUEUED_RUNS:-25}"
MAX_FAILED_RUNS_WINDOW="${MAX_FAILED_RUNS_WINDOW:-5}"
MAX_DEAD_LETTER_EVENTS_WINDOW="${MAX_DEAD_LETTER_EVENTS_WINDOW:-0}"
MAX_P95_RUN_DURATION_MS="${MAX_P95_RUN_DURATION_MS:-5000}"
MAX_AVG_RUN_DURATION_MS="${MAX_AVG_RUN_DURATION_MS:-}"
MAX_ACTION_P95_MS="${MAX_ACTION_P95_MS:-}"
MAX_ACTION_FAILED_RATE_PCT="${MAX_ACTION_FAILED_RATE_PCT:-}"
MAX_ACTION_DENIED_RATE_PCT="${MAX_ACTION_DENIED_RATE_PCT:-}"
ACTION_LATENCY_JSON="${ACTION_LATENCY_JSON:-}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required for soak gate automation" >&2
  exit 1
fi

cargo build -p agntctl >/dev/null
AGNTCTL_BIN="${AGNTCTL_BIN:-./target/debug/agntctl}"

if [[ ! -x "${AGNTCTL_BIN}" ]]; then
  echo "agntctl binary not found or not executable: ${AGNTCTL_BIN}" >&2
  exit 1
fi

echo "Running soak gate for ${ITERATIONS} iterations (sleep=${SLEEP_SECS}s)"
for i in $(seq 1 "${ITERATIONS}"); do
  echo "Iteration ${i}/${ITERATIONS}"
  cmd=(
    "${AGNTCTL_BIN}" ops soak-gate
    --api-base-url "${API_BASE_URL}"
    --tenant-id "${TENANT_ID}"
    --user-role "${USER_ROLE}"
    --window-secs "${WINDOW_SECS}"
    --max-queued-runs "${MAX_QUEUED_RUNS}"
    --max-failed-runs-window "${MAX_FAILED_RUNS_WINDOW}"
    --max-dead-letter-events-window "${MAX_DEAD_LETTER_EVENTS_WINDOW}"
    --max-p95-run-duration-ms "${MAX_P95_RUN_DURATION_MS}"
  )
  if [[ -n "${MAX_AVG_RUN_DURATION_MS}" ]]; then
    cmd+=(--max-avg-run-duration-ms "${MAX_AVG_RUN_DURATION_MS}")
  fi
  if [[ -n "${MAX_ACTION_P95_MS}" ]]; then
    cmd+=(--max-action-p95-ms "${MAX_ACTION_P95_MS}")
  fi
  if [[ -n "${MAX_ACTION_FAILED_RATE_PCT}" ]]; then
    cmd+=(--max-action-failed-rate-pct "${MAX_ACTION_FAILED_RATE_PCT}")
  fi
  if [[ -n "${MAX_ACTION_DENIED_RATE_PCT}" ]]; then
    cmd+=(--max-action-denied-rate-pct "${MAX_ACTION_DENIED_RATE_PCT}")
  fi
  if [[ -n "${ACTION_LATENCY_JSON}" ]]; then
    cmd+=(--action-latency-json "${ACTION_LATENCY_JSON}")
  fi

  "${cmd[@]}"

  if [[ "${i}" -lt "${ITERATIONS}" ]]; then
    sleep "${SLEEP_SECS}"
  fi
done

echo "Soak gate completed successfully"
