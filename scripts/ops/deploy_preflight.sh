#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

REQUIRED_FILES=(
  "infra/config/secureagnt.yaml"
  "infra/systemd/secureagnt.service"
  "infra/systemd/secureagnt-api.service"
  "infra/launchd/secureagnt.plist"
  "infra/launchd/secureagnt-api.plist"
)

for rel_path in "${REQUIRED_FILES[@]}"; do
  if [[ ! -f "${REPO_ROOT}/${rel_path}" ]]; then
    echo "[deploy-preflight] missing required file: ${rel_path}" >&2
    exit 1
  fi
done

if [[ "${DEPLOY_PREFLIGHT_VERIFY_MANIFEST:-0}" == "1" ]]; then
  RELEASE_MANIFEST_INPUT="${RELEASE_MANIFEST_INPUT:-${REPO_ROOT}/dist/release-manifest.sha256}" \
    bash "${SCRIPT_DIR}/verify_release_manifest.sh"
else
  echo "[deploy-preflight] skipping manifest verification (set DEPLOY_PREFLIGHT_VERIFY_MANIFEST=1 to enable)"
fi

echo "[deploy-preflight] pass"
