#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

run() {
  echo "[m9-signoff] $*"
  "$@"
}

# Governance supply-chain gate workflow
run make -C "${REPO_ROOT}" governance-gate

# Worker governance controls: approval gates + skill provenance digest checks
run cargo test -p worker --test worker_integration worker_process_once_denies_payment_send_when_governance_approval_required -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_allows_payment_send_when_governance_approval_present -- --nocapture
run cargo test -p worker --test worker_integration worker_process_once_fails_when_skill_script_digest_mismatch -- --nocapture

echo "[m9-signoff] pass"
