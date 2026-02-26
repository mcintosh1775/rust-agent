#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: upload_release_assets.sh <tag_name> [release_dir] [repo]

Examples:
  upload_release_assets.sh v0.1.93
  upload_release_assets.sh v0.1.93 ./dist/release/v0.1.93 mcintosh1775/rust-agent
EOF
}

resolve_repo_from_origin() {
  local origin
  origin="$(git config --get remote.origin.url || true)"
  if [ -z "${origin}" ]; then
    return 1
  fi

  if [[ "${origin}" == git@github.com:* ]]; then
    echo "${origin#git@github.com:}" | sed 's/\.git$//'
    return 0
  fi

  if [[ "${origin}" == https://github.com/* ]]; then
    echo "${origin#https://github.com/}" | sed 's/\.git$//'
    return 0
  fi

  return 1
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

print_release_permission_hint() {
  local msg="$1"
  echo "failed to fetch/create release for ${TAG_NAME}" >&2
  echo "${msg}" >&2
  echo >&2
  echo "If this looks like a token permission error, create/use a token with Releases: write scope." >&2
  echo "For a classic token, enable the 'repo' scope for private repository access." >&2
  echo "For a fine-grained token, enable repository write access and Release permissions." >&2
}

if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
  usage
  exit 1
fi

TAG_NAME="${1}"
RELEASE_DIR="${2:-dist/local-release/${TAG_NAME}}"
REPO="${3:-}"
if [ -z "${REPO}" ]; then
  REPO="$(resolve_repo_from_origin || true)"
fi

if [ -z "${REPO}" ]; then
  echo "unable to resolve repo. pass as third arg or set git remote.origin.url" >&2
  exit 1
fi

require_cmd curl
require_cmd jq
require_cmd sed
require_cmd find

API_TOKEN="${GITHUB_TOKEN:-}"
if [ -z "${API_TOKEN}" ] && command -v gh >/dev/null 2>&1; then
  API_TOKEN="$(gh auth token --hostname github.com 2>/dev/null || true)"
fi

if [ -z "${API_TOKEN}" ]; then
  echo "set GITHUB_TOKEN (or run gh auth login) before running this script" >&2
  exit 1
fi

if [ ! -d "${RELEASE_DIR}" ]; then
  echo "release directory not found: ${RELEASE_DIR}" >&2
  exit 1
fi

manifest_path="${RELEASE_DIR}/release-manifest.sha256"
if [ ! -f "${manifest_path}" ]; then
  echo "[release-upload] warning: manifest not found at ${manifest_path}"
fi

release_api="https://api.github.com/repos/${REPO}/releases/tags/${TAG_NAME}"
release_resp="$(curl -sS -H "Authorization: token ${API_TOKEN}" "${release_api}")"

if ! echo "${release_resp}" | jq -e '.id' >/dev/null 2>&1; then
  if jq -e '.message' >/dev/null 2>&1 <<< "${release_resp}"; then
    msg="$(echo "${release_resp}" | jq -r '.message // \"\"')"
  else
    msg="non-JSON response from GitHub API"
  fi

  create_payload="$(jq -nc --arg tag "${TAG_NAME}" --arg name "${TAG_NAME}" '{tag_name:$tag,name:$name,prerelease:false}')"
  create_resp="$(curl -sS -X POST \
    -H "Authorization: token ${API_TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    -H "Content-Type: application/json" \
    -d "${create_payload}" \
    "https://api.github.com/repos/${REPO}/releases")"

  if ! echo "${create_resp}" | jq -e '.id' >/dev/null 2>&1; then
    if echo "${create_resp}" | jq -e '.message' >/dev/null 2>&1; then
      create_msg="$(echo "${create_resp}" | jq -r '.message // \"\"')"
    else
      create_msg="non-JSON response from GitHub API"
    fi
    print_release_permission_hint "${create_msg}"
    echo "Original error:" >&2
    echo "${create_resp}" >&2
    exit 1
  fi
  release_resp="${create_resp}"
fi

release_id="$(echo "${release_resp}" | jq -r '.id')"
upload_url="$(echo "${release_resp}" | jq -r '.upload_url' | sed 's/{?name,label}//')"
assets_json="$(curl -sS -H "Authorization: token ${API_TOKEN}" "https://api.github.com/repos/${REPO}/releases/${release_id}/assets")"

if ! echo "${assets_json}" | jq -e 'type=="array"' >/dev/null 2>&1; then
  assets_json='[]'
fi

echo "[release-upload] uploading artifacts from ${RELEASE_DIR} to ${REPO} release ${TAG_NAME}"

mapfile -t release_files < <(find "${RELEASE_DIR}" -maxdepth 1 -type f | sort)
if [ "${#release_files[@]}" -eq 0 ]; then
  echo "no files found in ${RELEASE_DIR}" >&2
  exit 1
fi

for file_path in "${release_files[@]}"; do
  file_name="$(basename "${file_path}")"
  case "${file_name}" in
    secureagnt-api-*|secureagntd-*|agntctl-*|secureagnt-nostr-keygen-*|secureagnt-solo-lite-installer*|secureagnt_*.deb|release-manifest.sha256)
      : ;;
    *)
      continue
      ;;
  esac

  existing_id="$(echo "${assets_json}" | jq -r --arg name "${file_name}" '.[] | select(.name == $name) | .id' | head -n 1)"
  if [ -n "${existing_id}" ] && [ "${existing_id}" != "null" ]; then
    echo "[release-upload] removing existing asset: ${file_name} (#${existing_id})"
    curl -sS -X DELETE \
      -H "Authorization: token ${API_TOKEN}" \
      "https://api.github.com/repos/${REPO}/releases/assets/${existing_id}" >/dev/null
  fi

  echo "[release-upload] uploading ${file_name}"
  curl -sS --fail \
    -H "Authorization: token ${API_TOKEN}" \
    -H "Content-Type: application/octet-stream" \
    --data-binary "@${file_path}" \
    "${upload_url}?name=${file_name}" >/tmp/release-upload-response.json
  rm -f /tmp/release-upload-response.json
done

echo "[release-upload] done"
