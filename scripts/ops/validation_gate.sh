#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

RUN_DB_SUITES="${VALIDATION_GATE_RUN_DB_SUITES:-0}"
RUN_COVERAGE="${VALIDATION_GATE_RUN_COVERAGE:-0}"
RUN_COMPLIANCE="${VALIDATION_GATE_RUN_COMPLIANCE:-1}"

echo "[validation-gate] runbook validation"
make -C "${REPO_ROOT}" runbook-validate

echo "[validation-gate] workspace build+test verify"
make -C "${REPO_ROOT}" verify

echo "[validation-gate] security integration gate"
make -C "${REPO_ROOT}" security-gate

if [[ "${RUN_COMPLIANCE}" == "1" ]]; then
  echo "[validation-gate] compliance durability gate"
  VERIFY_JSON="${VERIFY_JSON:-agntctl/fixtures/compliance_verify_ok.json}" \
  SLO_JSON="${SLO_JSON:-agntctl/fixtures/compliance_slo_ok.json}" \
  make -C "${REPO_ROOT}" compliance-gate
else
  echo "[validation-gate] skipping compliance gate (set VALIDATION_GATE_RUN_COMPLIANCE=1 to enable)"
fi

if [[ "${RUN_DB_SUITES}" == "1" ]]; then
  echo "[validation-gate] DB integration suites"
  make -C "${REPO_ROOT}" test-db
  make -C "${REPO_ROOT}" test-api-db
  make -C "${REPO_ROOT}" test-worker-db
else
  echo "[validation-gate] skipping explicit DB suite re-run (set VALIDATION_GATE_RUN_DB_SUITES=1 to enable)"
fi

if [[ "${RUN_COVERAGE}" == "1" ]]; then
  echo "[validation-gate] coverage gate"
  make -C "${REPO_ROOT}" coverage
else
  echo "[validation-gate] skipping coverage gate (set VALIDATION_GATE_RUN_COVERAGE=1 to enable)"
fi

echo "[validation-gate] perf regression gate (fixture-backed)"
CANDIDATE_SUMMARY_JSON="${CANDIDATE_SUMMARY_JSON:-agntctl/fixtures/ops_summary_candidate_ok.json}" \
CANDIDATE_HISTOGRAM_JSON="${CANDIDATE_HISTOGRAM_JSON:-agntctl/fixtures/ops_latency_histogram_candidate_ok.json}" \
CANDIDATE_TRACES_JSON="${CANDIDATE_TRACES_JSON:-agntctl/fixtures/ops_latency_traces_candidate_ok.json}" \
make -C "${REPO_ROOT}" perf-gate

echo "[validation-gate] complete"
