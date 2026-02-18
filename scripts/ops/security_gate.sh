#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
RUN_DB_SECURITY="${RUN_DB_SECURITY:-}"

is_true() {
  case "${1,,}" in
    1|true|yes|on) return 0 ;;
    *) return 1 ;;
  esac
}

run() {
  echo "[security-gate] $*"
  "$@"
}

# Deterministic policy and redaction behavior.
run cargo test -p core policy::tests
run cargo test -p core redaction::tests

# Skill containment checks (timeout + env scrubbing boundary).
run cargo test -p skillrunner --test runner_integration invoke_scrubs_env_by_default_and_supports_allowlist
run cargo test -p skillrunner --test runner_integration invoke_timeout_kills_skill

if [[ -z "${RUN_DB_SECURITY}" ]]; then
  if is_true "${RUN_DB_TESTS:-0}"; then
    RUN_DB_SECURITY="1"
  else
    RUN_DB_SECURITY="0"
  fi
fi

if is_true "${RUN_DB_SECURITY}"; then
  export RUN_DB_TESTS=1
  export TEST_DATABASE_URL

  # Worker integration checks for deny-by-default boundaries and redaction persistence.
  # Use explicit opt-in because worker DB tests spawn subprocesses and require
  # a less-restricted host than some CI/sandbox environments provide.
  run make -C "${REPO_ROOT}" test-worker-db
else
  echo "[security-gate] skipping DB-backed worker security tests (set RUN_DB_SECURITY=1 or RUN_DB_TESTS=1 to enable)"
fi

echo "[security-gate] pass"
