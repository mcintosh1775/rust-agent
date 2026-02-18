#!/usr/bin/env bash
set -euo pipefail

API_BASE_URL="${API_BASE_URL:-http://localhost:3000}"
TENANT_ID="${TENANT_ID:-single}"
USER_ROLE="${USER_ROLE:-operator}"
WINDOW_SECS="${WINDOW_SECS:-3600}"

VERIFY_JSON="${VERIFY_JSON:-}"
SLO_JSON="${SLO_JSON:-}"
TARGETS_JSON="${TARGETS_JSON:-}"
MAX_HARD_FAILURE_RATE_PCT="${MAX_HARD_FAILURE_RATE_PCT:-0}"
MAX_DEAD_LETTER_RATE_PCT="${MAX_DEAD_LETTER_RATE_PCT:-0}"
MAX_OLDEST_PENDING_AGE_SECS="${MAX_OLDEST_PENDING_AGE_SECS:-}"
MAX_TARGET_HARD_FAILURE_RATE_PCT="${MAX_TARGET_HARD_FAILURE_RATE_PCT:-}"
MAX_TARGET_DEAD_LETTER_RATE_PCT="${MAX_TARGET_DEAD_LETTER_RATE_PCT:-}"
MAX_TARGET_PENDING_COUNT="${MAX_TARGET_PENDING_COUNT:-}"
ALLOW_CHAIN_GAPS="${ALLOW_CHAIN_GAPS:-0}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required for compliance gate automation" >&2
  exit 1
fi

cargo build -p agntctl >/dev/null
AGNTCTL_BIN="${AGNTCTL_BIN:-./target/debug/agntctl}"

if [[ ! -x "${AGNTCTL_BIN}" ]]; then
  echo "agntctl binary not found or not executable: ${AGNTCTL_BIN}" >&2
  exit 1
fi

cmd=(
  "${AGNTCTL_BIN}" ops compliance-gate
  --api-base-url "${API_BASE_URL}"
  --tenant-id "${TENANT_ID}"
  --user-role "${USER_ROLE}"
  --window-secs "${WINDOW_SECS}"
  --max-hard-failure-rate-pct "${MAX_HARD_FAILURE_RATE_PCT}"
  --max-dead-letter-rate-pct "${MAX_DEAD_LETTER_RATE_PCT}"
)

if [[ -n "${VERIFY_JSON}" ]]; then
  cmd+=(--verify-json "${VERIFY_JSON}")
fi

if [[ -n "${SLO_JSON}" ]]; then
  cmd+=(--slo-json "${SLO_JSON}")
fi

if [[ -n "${TARGETS_JSON}" ]]; then
  cmd+=(--targets-json "${TARGETS_JSON}")
fi

if [[ -n "${MAX_OLDEST_PENDING_AGE_SECS}" ]]; then
  cmd+=(--max-oldest-pending-age-secs "${MAX_OLDEST_PENDING_AGE_SECS}")
fi

if [[ -n "${MAX_TARGET_HARD_FAILURE_RATE_PCT}" ]]; then
  cmd+=(--max-target-hard-failure-rate-pct "${MAX_TARGET_HARD_FAILURE_RATE_PCT}")
fi

if [[ -n "${MAX_TARGET_DEAD_LETTER_RATE_PCT}" ]]; then
  cmd+=(--max-target-dead-letter-rate-pct "${MAX_TARGET_DEAD_LETTER_RATE_PCT}")
fi

if [[ -n "${MAX_TARGET_PENDING_COUNT}" ]]; then
  cmd+=(--max-target-pending-count "${MAX_TARGET_PENDING_COUNT}")
fi

if [[ "${ALLOW_CHAIN_GAPS}" == "1" ]]; then
  cmd+=(--allow-chain-gaps)
fi

"${cmd[@]}"
