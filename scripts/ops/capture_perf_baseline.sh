#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

API_BASE_URL="${AGNTCTL_API_BASE_URL:-http://localhost:3000}"
TENANT_ID="${AGNTCTL_TENANT_ID:-single}"
USER_ROLE="${AGNTCTL_USER_ROLE:-operator}"
WINDOW_SECS="${WINDOW_SECS:-3600}"
OUTPUT_DIR="${CAPTURE_BASELINE_OUTPUT_DIR:-${REPO_ROOT}/agntctl/fixtures/generated}"
BASELINE_PREFIX="${CAPTURE_BASELINE_PREFIX:-ops_baseline_$(date -u +%Y%m%dT%H%M%SZ)}"

echo "[capture-baseline] api=${API_BASE_URL} tenant=${TENANT_ID} role=${USER_ROLE} window_secs=${WINDOW_SECS}"
echo "[capture-baseline] output_dir=${OUTPUT_DIR} prefix=${BASELINE_PREFIX}"

cargo run -p agntctl -- ops capture-baseline \
  --api-base-url "${API_BASE_URL}" \
  --tenant-id "${TENANT_ID}" \
  --user-role "${USER_ROLE}" \
  --window-secs "${WINDOW_SECS}" \
  --out-dir "${OUTPUT_DIR}" \
  --file-prefix "${BASELINE_PREFIX}"

SUMMARY_PATH="${OUTPUT_DIR}/${BASELINE_PREFIX}_summary.json"
HISTOGRAM_PATH="${OUTPUT_DIR}/${BASELINE_PREFIX}_latency_histogram.json"
echo "[capture-baseline] summary=${SUMMARY_PATH}"
echo "[capture-baseline] histogram=${HISTOGRAM_PATH}"
