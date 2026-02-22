#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

TAG_NAME="${1:?usage: package_release_assets.sh <tag_name> [platform_tag] [output_dir]}"
PLATFORM_TAG="${2:-linux-x86_64}"
OUTPUT_DIR="${3:-${REPO_ROOT}/dist/local-release}"

release_dir="${OUTPUT_DIR}/${TAG_NAME}"
safe_tag="${TAG_NAME//\//-}"
api_name="secureagnt-api-${PLATFORM_TAG}-${safe_tag}"
worker_name="secureagntd-${PLATFORM_TAG}-${safe_tag}"
ctl_name="agntctl-${PLATFORM_TAG}-${safe_tag}"
mkdir -p "${release_dir}"

echo "[release-package] building release binaries"
cargo build --release -p api --bin secureagnt-api
cargo build --release -p worker --bin secureagntd
cargo build --release -p agntctl

echo "[release-package] packaging binaries for ${PLATFORM_TAG} into ${release_dir}"
cp target/release/secureagnt-api "${release_dir}/${api_name}"
cp target/release/secureagntd "${release_dir}/${worker_name}"
cp target/release/agntctl "${release_dir}/${ctl_name}"

chmod +x \
  "${release_dir}/${api_name}" \
  "${release_dir}/${worker_name}" \
  "${release_dir}/${ctl_name}"

tar -czf "${release_dir}/${api_name}.tar.gz" \
  -C "${release_dir}" "${api_name}"
tar -czf "${release_dir}/${worker_name}.tar.gz" \
  -C "${release_dir}" "${worker_name}"
tar -czf "${release_dir}/${ctl_name}.tar.gz" \
  -C "${release_dir}" "${ctl_name}"

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
  "${release_dir}/${api_name}.tar.gz" \
  "${release_dir}/${worker_name}.tar.gz" \
  "${release_dir}/${ctl_name}.tar.gz" \
  >>"${manifest_file}"

echo "[release-package] done: ${release_dir}"
echo "[release-package] files:"
ls -l "${release_dir}"
