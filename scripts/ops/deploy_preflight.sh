#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

detect_compose_cmd() {
  if command -v podman >/dev/null 2>&1; then
    echo "podman compose"
  elif command -v podman-compose >/dev/null 2>&1; then
    echo "podman-compose"
  elif command -v docker >/dev/null 2>&1; then
    echo "docker compose"
  else
    echo ""
  fi
}

COMPOSE_CMD="${COMPOSE_CMD:-$(detect_compose_cmd)}"
COMPOSE_FILE="${COMPOSE_FILE:-infra/containers/compose.yml}"
if [[ "${COMPOSE_FILE}" = /* ]]; then
  COMPOSE_FILE_ABS="${COMPOSE_FILE}"
else
  COMPOSE_FILE_ABS="${REPO_ROOT}/${COMPOSE_FILE}"
fi

REQUIRED_FILES=(
  "infra/config/secureagnt.yaml"
  "infra/systemd/secureagnt.service"
  "infra/systemd/secureagnt-api.service"
  "infra/launchd/secureagnt.plist"
  "infra/launchd/secureagnt-api.plist"
  "infra/containers/compose.yml"
)

for rel_path in "${REQUIRED_FILES[@]}"; do
  if [[ ! -f "${REPO_ROOT}/${rel_path}" ]]; then
    echo "[deploy-preflight] missing required file: ${rel_path}" >&2
    exit 1
  fi
done

if [[ "${DEPLOY_PREFLIGHT_VALIDATE_COMPOSE:-0}" == "1" ]]; then
  if [[ -z "${COMPOSE_CMD}" ]]; then
    echo "[deploy-preflight] compose validation requested but no compose runtime detected" >&2
    exit 1
  fi
  if [[ ! -f "${COMPOSE_FILE_ABS}" ]]; then
    echo "[deploy-preflight] compose file not found: ${COMPOSE_FILE_ABS}" >&2
    exit 1
  fi

  read -r -a COMPOSE_CMD_PARTS <<<"${COMPOSE_CMD}"
  "${COMPOSE_CMD_PARTS[@]}" -f "${COMPOSE_FILE_ABS}" --profile stack config >/dev/null
  echo "[deploy-preflight] compose config validated (${COMPOSE_CMD} -f ${COMPOSE_FILE_ABS} --profile stack config)"
else
  echo "[deploy-preflight] skipping compose config validation (set DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 to enable)"
fi

if [[ "${DEPLOY_PREFLIGHT_VERIFY_MANIFEST:-0}" == "1" ]]; then
  RELEASE_MANIFEST_INPUT="${RELEASE_MANIFEST_INPUT:-${REPO_ROOT}/dist/release-manifest.sha256}" \
    bash "${SCRIPT_DIR}/verify_release_manifest.sh"
else
  echo "[deploy-preflight] skipping manifest verification (set DEPLOY_PREFLIGHT_VERIFY_MANIFEST=1 to enable)"
fi

echo "[deploy-preflight] pass"
