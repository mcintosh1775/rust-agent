#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

SKIP_SOAK="${RELEASE_GATE_SKIP_SOAK:-1}"
RUN_DB_SUITES="${RELEASE_GATE_RUN_DB_SUITES:-0}"

echo "[release-gate] runbook validation"
make -C "${REPO_ROOT}" runbook-validate

echo "[release-gate] workspace build+test verify"
make -C "${REPO_ROOT}" verify

if [[ "${RUN_DB_SUITES}" == "1" ]]; then
  echo "[release-gate] DB integration suites"
  make -C "${REPO_ROOT}" test-db
  make -C "${REPO_ROOT}" test-api-db
  make -C "${REPO_ROOT}" test-worker-db
else
  echo "[release-gate] skipping explicit DB suite re-run (set RELEASE_GATE_RUN_DB_SUITES=1 to enable)"
fi

echo "[release-gate] perf regression gate (fixture-backed)"
CANDIDATE_SUMMARY_JSON="${CANDIDATE_SUMMARY_JSON:-agntctl/fixtures/ops_summary_candidate_ok.json}" \
CANDIDATE_HISTOGRAM_JSON="${CANDIDATE_HISTOGRAM_JSON:-agntctl/fixtures/ops_latency_histogram_candidate_ok.json}" \
make -C "${REPO_ROOT}" perf-gate

if [[ "${SKIP_SOAK}" == "1" ]]; then
  echo "[release-gate] skipping soak gate (set RELEASE_GATE_SKIP_SOAK=0 to enable)"
else
  echo "[release-gate] soak gate"
  make -C "${REPO_ROOT}" soak-gate
fi

echo "[release-gate] complete"
