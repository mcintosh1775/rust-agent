#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

TAG_NAME="${1:?usage: release_distribution_check.sh <tag_name> [platform_tag] [release_dir] [workflow_file]}"
PLATFORM_TAG="${2:-linux-x86_64}"
RELEASE_DIR="${3:-${REPO_ROOT}/dist/local-release/${TAG_NAME}}"
WORKFLOW_FILE="${4:-${REPO_ROOT}/.github/workflows/release.yml}"

safe_tag="${TAG_NAME//\//-}"
release_version="${TAG_NAME#v}"
if [[ -z "${release_version}" ]]; then
  echo "[release-distribution-check] invalid tag name: ${TAG_NAME}" >&2
  exit 1
fi
safe_version="${release_version//[^a-zA-Z0-9.+~:-]/-}"

if [[ ! -d "${RELEASE_DIR}" ]]; then
  echo "[release-distribution-check] release directory not found: ${RELEASE_DIR}" >&2
  exit 1
fi

if [[ ! -f "${WORKFLOW_FILE}" ]]; then
  echo "[release-distribution-check] workflow file not found: ${WORKFLOW_FILE}" >&2
  exit 1
fi

required_artifacts=(
  "secureagnt-api-${PLATFORM_TAG}-${safe_tag}"
  "secureagntd-${PLATFORM_TAG}-${safe_tag}"
  "agntctl-${PLATFORM_TAG}-${safe_tag}"
  "secureagnt-nostr-keygen-${PLATFORM_TAG}-${safe_tag}"
  "secureagnt-solo-lite-installer-${safe_tag}.sh"
  "secureagnt-solo-lite-installer.sh"
  "secureagnt-api-${PLATFORM_TAG}-${safe_tag}.tar.gz"
  "secureagntd-${PLATFORM_TAG}-${safe_tag}.tar.gz"
  "agntctl-${PLATFORM_TAG}-${safe_tag}.tar.gz"
  "secureagnt-nostr-keygen-${PLATFORM_TAG}-${safe_tag}.tar.gz"
  "release-manifest.sha256"
  "secureagnt_${safe_version}_amd64.deb"
)

manifest_path="${RELEASE_DIR}/release-manifest.sha256"
if [[ ! -f "${manifest_path}" ]]; then
  echo "[release-distribution-check] manifest not found: ${manifest_path}" >&2
  exit 1
fi

workflow_required_patterns=(
  "secureagnt-nostr-keygen-${PLATFORM_TAG}-${safe_tag}.tar.gz|secureagnt-nostr-keygen-\${{ env.PLATFORM_TAG }}-\${{ steps.release_meta.outputs.safe_tag_name }}.tar.gz"
  "secureagnt-nostr-keygen-${PLATFORM_TAG}-${safe_tag}|secureagnt-nostr-keygen-\${{ env.PLATFORM_TAG }}-\${{ steps.release_meta.outputs.safe_tag_name }}"
)

status=0
missing_count=0

for artifact in "${required_artifacts[@]}"; do
  if [[ ! -f "${RELEASE_DIR}/${artifact}" ]]; then
    echo "[release-distribution-check] missing artifact: ${artifact}" >&2
    status=1
    ((missing_count+=1))
    continue
  fi

  if ! awk '{print $2}' "${manifest_path}" \
    | sed 's#.*/##' \
    | grep -Fx -- "${artifact}" >/dev/null; then
    echo "[release-distribution-check] manifest missing entry for: ${artifact}" >&2
    status=1
  fi
done

for pattern in "${workflow_required_patterns[@]}"; do
  literal_pattern="${pattern%%|*}"
  template_pattern="${pattern##*|}"
  if ! grep -F -- "${literal_pattern}" "${WORKFLOW_FILE}" >/dev/null && \
     ! grep -F -- "${template_pattern}" "${WORKFLOW_FILE}" >/dev/null; then
    echo "[release-distribution-check] workflow missing artifact pattern: ${literal_pattern}" >&2
    status=1
  fi
done

if [[ ${status} -ne 0 ]]; then
  echo "[release-distribution-check] failed (missing artifacts: ${missing_count})" >&2
  exit 1
fi

echo "[release-distribution-check] passed for tag=${TAG_NAME} platform=${PLATFORM_TAG} dir=${RELEASE_DIR}"
