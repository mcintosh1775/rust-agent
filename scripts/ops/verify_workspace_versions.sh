#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${SCRIPT_DIR}/../.."

cd "${REPO_ROOT}"

WORKSPACE_VERSION="$(awk '
  /^\[workspace\.package\]/{in_ws=1; next}
  /^\[/{if (in_ws) exit}
  in_ws && /^version[[:space:]]*=/{gsub(/"/, "", $3); print $3; exit}
' Cargo.toml)"

if [[ -z "${WORKSPACE_VERSION}" ]]; then
  echo "[version-check] ERROR: workspace version not found"
  exit 1
fi

FAILED=0

for manifest in core/Cargo.toml api/Cargo.toml worker/Cargo.toml skillrunner/Cargo.toml agntctl/Cargo.toml; do
  MANIFEST_PATH="${manifest}"
  RAW_VERSION="$(awk '
    /^\[package\]/{in_package=1; next}
    /^\[/{if (in_package) exit}
    in_package && /^version[[:space:]]*=/{print $0; exit}
    in_package && /^version\.workspace[[:space:]]*=/{print $0; exit}
  ' "${MANIFEST_PATH}")"

  if [[ "${RAW_VERSION}" == version.workspace* ]]; then
    CRATE_VERSION="${WORKSPACE_VERSION}"
  else
    CRATE_VERSION="$(echo "${RAW_VERSION}" | awk -F '=' '{gsub(/[[:space:]]/, "", $2); gsub(/"/, "", $2); print $2}')"
  fi

  CRATE_NAME="$(awk '
    /^\[package\]/{in_package=1; next}
    /^\[/{if (in_package) exit}
    in_package && /^name[[:space:]]*=/{gsub(/"/, "", $3); print $3; exit}
  ' "${MANIFEST_PATH}")"

  if [[ "${CRATE_VERSION}" != "${WORKSPACE_VERSION}" ]]; then
    echo "[version-check] drift detected: ${CRATE_NAME} (${MANIFEST_PATH}) => ${CRATE_VERSION}, expected ${WORKSPACE_VERSION}"
    FAILED=1
  fi
done

if [[ "${FAILED}" -ne 0 ]]; then
  echo "[version-check] FAIL: one or more crate versions drift from workspace version"
  echo "[version-check] set crate versions to workspace version inheritance or match workspace version"
  exit 1
fi

echo "[version-check] pass: all tracked crate versions match workspace version ${WORKSPACE_VERSION}"
