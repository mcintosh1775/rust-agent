#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

MANIFEST_PATH="${RELEASE_MANIFEST_INPUT:-${REPO_ROOT}/dist/release-manifest.sha256}"

if [[ ! -f "${MANIFEST_PATH}" ]]; then
  echo "[release-manifest] manifest not found: ${MANIFEST_PATH}" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  (
    cd "${REPO_ROOT}"
    sha256sum --check "${MANIFEST_PATH}"
  )
elif command -v shasum >/dev/null 2>&1; then
  (
    cd "${REPO_ROOT}"
    shasum -a 256 --check "${MANIFEST_PATH}"
  )
else
  echo "[release-manifest] missing hash tool: sha256sum or shasum required" >&2
  exit 1
fi

echo "[release-manifest] verified ${MANIFEST_PATH}"
