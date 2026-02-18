#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"

run() {
  echo "[isolation-gate] $*"
  "$@"
}

# Run full DB-backed API/worker integration suites so tenant-isolation assertions are
# always exercised in the same execution mode used by normal CI and release gates.
run make -C "${REPO_ROOT}" test-api-db
run make -C "${REPO_ROOT}" test-worker-db

echo "[isolation-gate] pass"
