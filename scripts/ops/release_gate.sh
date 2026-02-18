#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

SKIP_SOAK="${RELEASE_GATE_SKIP_SOAK:-1}"
RUN_DB_SUITES="${RELEASE_GATE_RUN_DB_SUITES:-0}"
RUN_COVERAGE="${RELEASE_GATE_RUN_COVERAGE:-0}"
RUN_DB_SECURITY="${RELEASE_GATE_RUN_DB_SECURITY:-0}"
RUN_COMPLIANCE="${RELEASE_GATE_RUN_COMPLIANCE:-1}"
RUN_GOVERNANCE="${RELEASE_GATE_RUN_GOVERNANCE:-1}"

echo "[release-gate] validation gate"
VALIDATION_GATE_RUN_DB_SUITES="${RUN_DB_SUITES}" \
VALIDATION_GATE_RUN_COVERAGE="${RUN_COVERAGE}" \
VALIDATION_GATE_RUN_COMPLIANCE="${RUN_COMPLIANCE}" \
VALIDATION_GATE_RUN_GOVERNANCE="${RUN_GOVERNANCE}" \
RUN_DB_SECURITY="${RUN_DB_SECURITY}" \
make -C "${REPO_ROOT}" validation-gate

if [[ "${SKIP_SOAK}" == "1" ]]; then
  echo "[release-gate] skipping soak gate (set RELEASE_GATE_SKIP_SOAK=0 to enable)"
else
  echo "[release-gate] soak gate"
  make -C "${REPO_ROOT}" soak-gate
fi

echo "[release-gate] complete"
