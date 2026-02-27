#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

OUTPUT_PATH="${RELEASE_MANIFEST_OUTPUT:-${REPO_ROOT}/dist/release-manifest.sha256}"

if [[ -n "${RELEASE_MANIFEST_FILES:-}" ]]; then
  # shellcheck disable=SC2206
  FILES=(${RELEASE_MANIFEST_FILES})
else
  FILES=(
    "infra/config/secureagnt.yaml"
    "infra/systemd/secureagnt.service"
    "infra/systemd/secureagnt-api.service"
    "infra/systemd/secureagnt-slack-events-bridge.service"
    "infra/launchd/secureagnt.plist"
    "infra/launchd/secureagnt-api.plist"
    "scripts/ops/deploy_preflight.sh"
  )
fi

if command -v sha256sum >/dev/null 2>&1; then
  HASH_CMD=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  HASH_CMD=(shasum -a 256)
else
  echo "[release-manifest] missing hash tool: sha256sum or shasum required" >&2
  exit 1
fi

mkdir -p "$(dirname "${OUTPUT_PATH}")"
: >"${OUTPUT_PATH}"

for rel_path in "${FILES[@]}"; do
  abs_path="${REPO_ROOT}/${rel_path}"
  if [[ ! -f "${abs_path}" ]]; then
    echo "[release-manifest] missing file: ${rel_path}" >&2
    exit 1
  fi
  (
    cd "${REPO_ROOT}"
    "${HASH_CMD[@]}" "${rel_path}"
  ) >>"${OUTPUT_PATH}"
done

echo "[release-manifest] wrote ${OUTPUT_PATH}"
