#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

RUN_DEPLOY_PREFLIGHT="${GOVERNANCE_GATE_RUN_DEPLOY_PREFLIGHT:-1}"

echo "[governance-gate] generate release manifest"
make -C "${REPO_ROOT}" release-manifest

echo "[governance-gate] verify release manifest"
make -C "${REPO_ROOT}" release-manifest-verify

if [[ "${RUN_DEPLOY_PREFLIGHT}" == "1" ]]; then
  echo "[governance-gate] deploy preflight with manifest verification"
  DEPLOY_PREFLIGHT_VERIFY_MANIFEST=1 make -C "${REPO_ROOT}" deploy-preflight
else
  echo "[governance-gate] skipping deploy preflight (set GOVERNANCE_GATE_RUN_DEPLOY_PREFLIGHT=1 to enable)"
fi

echo "[governance-gate] pass"
