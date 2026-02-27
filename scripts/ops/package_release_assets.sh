#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

TAG_NAME="${1:?usage: package_release_assets.sh <tag_name> [platform_tag] [output_dir]}"
PLATFORM_TAG="${2:-linux-x86_64}"
OUTPUT_DIR="${3:-${REPO_ROOT}/dist/local-release}"

if [ "$(basename "${OUTPUT_DIR}")" = "${TAG_NAME}" ] || [ "$(basename "${OUTPUT_DIR}")" = "${TAG_NAME#v}" ]; then
  release_dir="${OUTPUT_DIR}"
else
  release_dir="${OUTPUT_DIR}/${TAG_NAME}"
fi

if [ "${release_dir}" != "${OUTPUT_DIR}/${TAG_NAME}" ] && [ "${release_dir}" = "${OUTPUT_DIR}" ]; then
  echo "[release-package] using provided release directory (already tag-aware): ${release_dir}"
else
  echo "[release-package] creating release directory: ${release_dir}"
fi

release_dir="${release_dir%/}"
safe_tag="${TAG_NAME//\//-}"
api_name="secureagnt-api-${PLATFORM_TAG}-${safe_tag}"
worker_name="secureagntd-${PLATFORM_TAG}-${safe_tag}"
ctl_name="agntctl-${PLATFORM_TAG}-${safe_tag}"
nostr_keygen_name="secureagnt-nostr-keygen-${PLATFORM_TAG}-${safe_tag}"
installer_name="secureagnt-solo-lite-installer-${safe_tag}.sh"
installer_source="${REPO_ROOT}/scripts/install/secureagnt-solo-lite-installer.sh"
mkdir -p "${release_dir}"

echo "[release-package] building release binaries"
cargo build --release -p api --bin secureagnt-api
cargo build --release -p worker --bin secureagntd
cargo build --release -p agntctl
cargo build --release -p worker --bin secureagnt-nostr-keygen

echo "[release-package] packaging binaries for ${PLATFORM_TAG} into ${release_dir}"
cp target/release/secureagnt-api "${release_dir}/${api_name}"
cp target/release/secureagntd "${release_dir}/${worker_name}"
cp target/release/agntctl "${release_dir}/${ctl_name}"
cp target/release/secureagnt-nostr-keygen "${release_dir}/${nostr_keygen_name}"
cp "${installer_source}" "${release_dir}/secureagnt-solo-lite-installer.sh"
cp "${installer_source}" "${release_dir}/${installer_name}"

chmod +x \
  "${release_dir}/${api_name}" \
  "${release_dir}/${worker_name}" \
  "${release_dir}/${ctl_name}" \
  "${release_dir}/${nostr_keygen_name}" \
  "${release_dir}/secureagnt-solo-lite-installer.sh" \
  "${release_dir}/${installer_name}"

tar -czf "${release_dir}/${api_name}.tar.gz" \
  -C "${release_dir}" "${api_name}"
tar -czf "${release_dir}/${worker_name}.tar.gz" \
  -C "${release_dir}" "${worker_name}"
tar -czf "${release_dir}/${ctl_name}.tar.gz" \
  -C "${release_dir}" "${ctl_name}"
tar -czf "${release_dir}/${nostr_keygen_name}.tar.gz" \
  -C "${release_dir}" "${nostr_keygen_name}"

if command -v sha256sum >/dev/null 2>&1; then
  HASH_CMD=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  HASH_CMD=(shasum -a 256)
else
  echo "[release-package] missing hash tool (sha256sum/shasum required)" >&2
  exit 1
fi

manifest_file="${release_dir}/release-manifest.sha256"
: >"${manifest_file}"
"${HASH_CMD[@]}" \
  "${release_dir}/${api_name}" \
  "${release_dir}/${worker_name}" \
  "${release_dir}/${ctl_name}" \
  "${release_dir}/${nostr_keygen_name}" \
  "${release_dir}/secureagnt-solo-lite-installer.sh" \
  "${release_dir}/${installer_name}" \
  "${release_dir}/${api_name}.tar.gz" \
  "${release_dir}/${worker_name}.tar.gz" \
  "${release_dir}/${ctl_name}.tar.gz" \
  "${release_dir}/${nostr_keygen_name}.tar.gz" \
  >>"${manifest_file}"

"${HASH_CMD[@]}" "${manifest_file}" >>"${manifest_file}"

echo "[release-package] done: ${release_dir}"
echo "[release-package] files:"
ls -l "${release_dir}"
