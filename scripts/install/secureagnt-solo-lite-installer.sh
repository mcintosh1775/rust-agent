#!/usr/bin/env bash

set -euo pipefail

if [[ -z "${SECUREAGNT_INSTALL_HOME:-}" ]]; then
  if [[ "${EUID}" -eq 0 ]]; then
    install_home="/opt/secureagnt"
  else
    install_home="${HOME}/.secureagnt"
  fi
else
  install_home="${SECUREAGNT_INSTALL_HOME}"
fi

if [[ -z "${SECUREAGNT_BINARY_DIR:-}" ]]; then
  if [[ "${EUID}" -eq 0 ]]; then
    binary_dir="/usr/local/bin"
  else
    binary_dir="${install_home}/bin"
  fi
else
  binary_dir="${SECUREAGNT_BINARY_DIR}"
fi
worktree_dir="${SECUREAGNT_WORKTREE:-${install_home}/source}"
release_repo="mcintosh1775/rust-agent"
release_version="${SECUREAGNT_RELEASE_VERSION:-latest}"
release_base_url="https://github.com/${release_repo}/releases/download"
source_repo_url="${SECUREAGNT_SOURCE_REPO_URL:-https://github.com/${release_repo}.git}"
source_branch="${SECUREAGNT_SOURCE_BRANCH:-main}"
platform_tag="${SECUREAGNT_PLATFORM_TAG:-linux-x86_64}"
curls_auth_args=()
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  curls_auth_args=("-H" "Authorization: token ${GITHUB_TOKEN}")
fi
download_binaries="${SECUREAGNT_DOWNLOAD_BINARIES:-1}"
non_interactive="${SECUREAGNT_NON_INTERACTIVE:-0}"
resolved_release_tag=""

sandbox_root="${SECUREAGNT_SANDBOX_ROOT:-}"
worker_artifact_root="${WORKER_ARTIFACT_ROOT:-}"
worker_local_exec_read_roots="${WORKER_LOCAL_EXEC_READ_ROOTS:-}"
worker_local_exec_write_roots="${WORKER_LOCAL_EXEC_WRITE_ROOTS:-}"
tenant_id="single"

dry_run="${SECUREAGNT_DRY_RUN:-0}"
setup_mode="${SECUREAGNT_SETUP_MODE:-bootstrap}"
start_services="${SECUREAGNT_START_SERVICES:-1}"
service_scope="${SECUREAGNT_SERVICE_SCOPE:-system}"
service_unit_dir="${SECUREAGNT_SERVICE_UNIT_DIR:-}"
service_user="${SECUREAGNT_SERVICE_USER:-}"
service_group="${SECUREAGNT_SERVICE_GROUP:-}"
service_api_name="${SECUREAGNT_SOLO_LIGHT_API_SERVICE_NAME:-secureagnt-lite-api.service}"
service_worker_name="${SECUREAGNT_SOLO_LIGHT_WORKER_SERVICE_NAME:-secureagnt-lite.service}"
service_install_target="${SECUREAGNT_SOLO_LIGHT_SERVICE_TARGET:-}"
service_protect_home="${SECUREAGNT_SERVICE_PROTECT_HOME:-}"
service_unit_file_mode="${SECUREAGNT_SOLO_LIGHT_SERVICE_UNIT_FILE_MODE:-0644}"
service_env_file_mode="${SECUREAGNT_SOLO_LIGHT_SERVICE_ENV_FILE_MODE:-0600}"
solo_light_env_path="${SECUREAGNT_SOLO_LIGHT_ENV_PATH:-}"
solo_light_data_root="${SECUREAGNT_SOLO_LIGHT_DATA_ROOT:-}"
solo_light_log_root="${SECUREAGNT_SOLO_LIGHT_LOG_ROOT:-}"
solo_light_db_path="${SECUREAGNT_SOLO_LIGHT_DB_PATH:-}"
solo_light_api_bind="${SECUREAGNT_SOLO_LIGHT_API_BIND:-0.0.0.0:8080}"
solo_light_worker_id="${SECUREAGNT_SOLO_LIGHT_WORKER_ID:-worker-solo-light-1}"
solo_light_artifact_root="${SECUREAGNT_SOLO_LIGHT_ARTIFACT_ROOT:-}"
solo_light_local_exec_read_roots="${SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_READ_ROOTS:-}"
solo_light_local_exec_write_roots="${SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_WRITE_ROOTS:-}"
solo_light_database_url=""

agent_name="${SECUREAGNT_AGENT_NAME:-solo-lite-agent}"
agent_role="${SECUREAGNT_AGENT_ROLE:-Personal coordinator and operations assistant for a single workspace}"
soul_style="${SECUREAGNT_SOUL_STYLE:-concise, practical, evidence-first}"
soul_values="${SECUREAGNT_SOUL_VALUES:-secure-by-default behavior, auditable actions, clear communication}"
soul_boundaries="${SECUREAGNT_SOUL_BOUNDARIES:-do not bypass policy, do not invent authority, escalate uncertainty for high-risk actions}"

repo_dir=""
install_state_file=""
systemctl_scope_flags=""

usage() {
  cat <<'USAGE'
Usage: secureagnt-solo-lite-installer [--help] [--mode <solo-light|bootstrap>] [--bootstrap] [--solo-light] [--dry-run]

Modes:
  bootstrap   Install binaries, initialize sqlite via `make solo-lite-init`, walk through agent/SOUL setup, write service files, and start services.
  solo-light  Install binaries + write systemd service files + optionally start them.

Environment variables:
  SECUREAGNT_SETUP_MODE          Install mode (solo-light|bootstrap), default: bootstrap
  SECUREAGNT_INSTALL_HOME         Installer workspace (default: $HOME/.secureagnt, or /opt/secureagnt when running as root/system scope)
  SECUREAGNT_BINARY_DIR           Installed binary directory (default: /usr/local/bin when running as root, otherwise $SECUREAGNT_INSTALL_HOME/bin)
  SECUREAGNT_WORKTREE            Source checkout used for bootstrap scripts (default: $SECUREAGNT_INSTALL_HOME/source)
  SECUREAGNT_RELEASE_VERSION      Release tag (optional; defaults to latest)
  SECUREAGNT_SOURCE_REPO_URL      Source git URL fallback for bootstrap scripts (default: https://github.com/<repo>.git)
  SECUREAGNT_SOURCE_BRANCH        Source git branch (default: main)
  GITHUB_TOKEN                     GitHub token for private repo access to resolve releases/downloads
  SECUREAGNT_PLATFORM_TAG         Binary asset suffix (default: linux-x86_64)
  SECUREAGNT_DOWNLOAD_BINARIES    Skip release-binary installation (0/1, default 1)
  SECUREAGNT_NON_INTERACTIVE      Non-interactive defaults (0/1)
  SECUREAGNT_DRY_RUN              Print resolved config and exit (0/1)
  SECUREAGNT_START_SERVICES        Start service files after generation (default 1; bootstrap and solo-light)
  SECUREAGNT_SERVICE_SCOPE         system|user (system default, requires root)
  SECUREAGNT_SERVICE_UNIT_DIR       systemd unit directory
  SECUREAGNT_SERVICE_PROTECT_HOME   Protects /root access for system services (0/1, default 1; auto-disabled when install path is /root)
  SECUREAGNT_SOLO_LIGHT_SERVICE_TARGET         systemd install target (solo-light)
  SECUREAGNT_SOLO_LIGHT_ENV_PATH     env file path (solo-light)
  SECUREAGNT_SOLO_LIGHT_DATA_ROOT     data root path (solo-light)
  SECUREAGNT_SOLO_LIGHT_LOG_ROOT      log root path (solo-light)
  SECUREAGNT_SOLO_LIGHT_DB_PATH       sqlite database path
  SECUREAGNT_SOLO_LIGHT_API_BIND      bind address
  SECUREAGNT_SOLO_LIGHT_WORKER_ID      worker id
  SECUREAGNT_SOLO_LIGHT_ARTIFACT_ROOT  local artifact root
  SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_READ_ROOTS   read allowlist
  SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_WRITE_ROOTS  write allowlist

  SECUREAGNT_AGENT_NAME            Persona + bootstrap name (bootstrap only)
  SECUREAGNT_AGENT_ROLE            Persona role (bootstrap only)
  SECUREAGNT_SOUL_STYLE            Persona style (bootstrap only)
  SECUREAGNT_SOUL_VALUES           SOUL values (bootstrap only)
  SECUREAGNT_SOUL_BOUNDARIES       SOUL boundaries (bootstrap only)
  SECUREAGNT_SANDBOX_ROOT           worker sandbox root (bootstrap + solo-light defaults)
  WORKER_ARTIFACT_ROOT              bootstrap artifact root
  WORKER_LOCAL_EXEC_READ_ROOTS       bootstrap local.exec read roots
  WORKER_LOCAL_EXEC_WRITE_ROOTS      bootstrap local.exec write roots

Examples:
  SECUREAGNT_DRY_RUN=1 SECUREAGNT_NON_INTERACTIVE=1 bash scripts/install/secureagnt-solo-lite-installer.sh
  SECUREAGNT_NON_INTERACTIVE=1 SECUREAGNT_RELEASE_VERSION=v0.1.97 bash scripts/install/secureagnt-solo-lite-installer.sh
  SECUREAGNT_RELEASE_VERSION=v0.1.97 SECUREAGNT_SETUP_MODE=bootstrap bash scripts/install/secureagnt-solo-lite-installer.sh
  bash scripts/install/secureagnt-solo-lite-installer.sh --bootstrap
  SECUREAGNT_SERVICE_SCOPE=user bash scripts/install/secureagnt-solo-lite-installer.sh
USAGE
}

parse_args() {
  while [[ "$#" -gt 0 ]]; do
    case "$1" in
      --help)
        usage
        exit 0
        ;;
      --dry-run)
        dry_run="1"
        ;;
      --bootstrap)
        setup_mode="bootstrap"
        ;;
      --solo-light)
        setup_mode="solo-light"
        ;;
      --mode)
        shift
        if [[ "$#" -eq 0 ]]; then
          echo "--mode requires a value" >&2
          usage
          exit 1
        fi
        setup_mode="$1"
        ;;
      --mode=*)
        setup_mode="${1#--mode=}"
        ;;
      *)
        echo "unknown argument: $1" >&2
        usage
        exit 1
        ;;
    esac
    shift
  done
}

validate_mode() {
  case "${setup_mode}" in
    solo-light|bootstrap)
      ;;
    *)
      echo "unsupported setup mode: ${setup_mode} (expected solo-light|bootstrap)" >&2
      exit 1
      ;;
  esac
}

assert_root_for_system_scope() {
  if [[ "${service_scope}" == "system" && "${EUID}" -ne 0 ]]; then
    echo "SECUREAGNT_SERVICE_SCOPE=system requires root (run with sudo) or set SECUREAGNT_SERVICE_SCOPE=user." >&2
    exit 1
  fi

  if [[ "${service_scope}" == "user" && "${EUID}" -eq 0 ]]; then
    echo "SECUREAGNT_SERVICE_SCOPE=user is not valid when running this installer as root (omit root for user scope)." >&2
    exit 1
  fi
}

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required tool: $1" >&2
    exit 1
  fi
}

require_linux_x86_64() {
  local os
  local arch
  os="$(uname -s)"
  arch="$(uname -m)"
  if [[ "${os}" != "Linux" ]]; then
    echo "installer supports Linux only (current OS: ${os})" >&2
    exit 1
  fi
  if [[ "${arch}" != "x86_64" ]]; then
    echo "installer x86_64-only only (current arch: ${arch})" >&2
    exit 1
  fi
}

prompt() {
  local var_name="$1"
  local prompt_text="$2"
  local default_value="$3"
  local response

  if [[ "${non_interactive}" == "1" ]]; then
    printf -v "$var_name" "%s" "$default_value"
    return 0
  fi

  read -r -p "${prompt_text} [${default_value}]: " response
  if [[ -z "${response}" ]]; then
    response="${default_value}"
  fi
  printf -v "$var_name" "%s" "$response"
}

cleanup() {
  if [[ -n "${install_state_file}" && -f "${install_state_file}" ]]; then
    rm -f "${install_state_file}"
  fi
}

comma_list_to_bullets() {
  local value="$1"
  local item
  IFS="," read -r -a items <<< "${value}"
  for item in "${items[@]}"; do
    item="$(printf "%s" "${item}" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
    if [[ -n "${item}" ]]; then
      printf "  - %s\n" "${item}"
    fi
  done
}

resolve_release_tag() {
  if [[ "${release_version}" != "latest" ]]; then
    resolved_release_tag="${release_version}"
    return 0
  fi

  local tag_payload
  local api_url="https://api.github.com/repos/${release_repo}/releases/latest"
  if command -v jq >/dev/null 2>&1; then
    tag_payload="$(curl -fsSL "${curls_auth_args[@]}" "${api_url}" | jq -r '.tag_name' || true)"
  else
    tag_payload="$(curl -fsSL "${curls_auth_args[@]}" "${api_url}" | tr -d '\n\r' | sed -n 's/.*\"tag_name\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p' | head -n 1 || true)"
  fi
  if [[ -z "${tag_payload}" || "${tag_payload}" == "null" ]]; then
    if [[ -z "${GITHUB_TOKEN:-}" ]]; then
      echo "failed to resolve latest release tag from ${release_repo} (private repos/release assets may require GITHUB_TOKEN)" >&2
    else
      echo "failed to resolve latest release tag from ${release_repo}" >&2
    fi
    return 1
  fi

  resolved_release_tag="${tag_payload}"
  return 0
}

download_one_binary() {
  local binary="$1"
  local tmp_dir
  local archive
  local downloaded_file
  local final_path
  local archive_tag
  if [[ -z "${resolved_release_tag}" ]] && ! resolve_release_tag; then
    return 1
  fi

  local download_tag="${resolved_release_tag}"
  archive_tag="${download_tag//\//-}"

  tmp_dir="$(mktemp -d)"
  trap '[ -n "${tmp_dir-}" ] && rm -rf "${tmp_dir-}"' RETURN

  for archive in \
    "${binary}-${platform_tag}-${archive_tag}.tar.gz" \
    "${binary}-${platform_tag}-${archive_tag}" \
    "${binary}-${platform_tag}.tar.gz" \
    "${binary}-${platform_tag}" \
    "${binary}.tar.gz" \
    "${binary}"; do
    local url="${release_base_url}/${download_tag}/${archive}"
    downloaded_file="${tmp_dir}/${archive}"
    echo "attempting binary fetch: ${url}"
    if curl -fsSLo "${downloaded_file}" "${curls_auth_args[@]}" "${url}"; then
      if [[ "${archive}" == *.tar.gz ]]; then
        tar -xzf "${downloaded_file}" -C "${tmp_dir}" >/dev/null
        final_path="$(find "${tmp_dir}" -type f -name "${binary}" | head -n 1 || true)"
      else
        final_path="${downloaded_file}"
      fi

      if [[ -n "${final_path}" && -f "${final_path}" ]]; then
        chmod +x "${final_path}"
        cp "${final_path}" "${binary_dir}/${binary}"
        return 0
      fi
    fi
  done

  return 1
}

ensure_binary() {
  local binary="$1"
  local target="${binary_dir}/${binary}"

  if [[ -x "${target}" ]]; then
    return 0
  fi

  if [[ "${download_binaries}" == "1" ]]; then
    if download_one_binary "${binary}"; then
      return 0
    fi
    echo "release download failed for ${binary}" >&2
  fi

  return 1
}

prepare_repo() {
  if [[ -d "${worktree_dir}/.git" || -f "${worktree_dir}/.git" ]]; then
    repo_dir="${worktree_dir}"
    return 0
  fi

  mkdir -p "${install_home}"
  if [[ -d "${worktree_dir}" ]]; then
    rm -rf "${worktree_dir}"
  fi

  if command -v git >/dev/null 2>&1; then
    git clone --depth 1 --branch "${source_branch}" "${source_repo_url}" "${worktree_dir}"
    repo_dir="${worktree_dir}"
    return 0
  fi

  echo "git is required when the installer source workspace is not available." >&2
  exit 1
}

prepare_workspace() {
  mkdir -p "${install_home}" "${binary_dir}"
}

install_binaries() {
  local binary
  local binaries=("secureagnt-api" "secureagntd" "agntctl")
  for binary in "${binaries[@]}"; do
    if ! ensure_binary "${binary}"; then
      echo "unable to install required binary ${binary}. Ensure release assets exist for ${release_repo} tag ${release_version} and retry." >&2
      exit 1
    fi
  done
}

to_abs_path() {
  local path_value="$1"
  local root_path="${install_home}"
  if [[ -z "${path_value}" ]]; then
    printf ""
    return 0
  fi
  if [[ "${path_value}" == /* ]]; then
    printf "%s" "${path_value%/}"
    return 0
  fi
  printf "%s/%s" "${root_path%/}" "${path_value}"
}

prompt_bootstrap() {
  prompt "agent_name" "Operator, what should the agent be called" "${agent_name}"
  prompt "agent_role" "What is this agent's role?" "${agent_role}"
  prompt "soul_style" "Describe communication style / personality" "${soul_style}"
  prompt "soul_values" "What values should be in SOUL.md? (comma-separated)" "${soul_values}"
  prompt "soul_boundaries" "Hard boundaries for SOUL.md? (comma-separated)" "${soul_boundaries}"

  sandbox_root="${sandbox_root:-/opt/agent}"
  sandbox_root="${sandbox_root%/}"
  worker_artifact_root="${worker_artifact_root%/}"
  if [[ -z "${worker_artifact_root}" ]]; then
    worker_artifact_root="${sandbox_root}/artifacts"
  fi
  if [[ -z "${worker_local_exec_read_roots}" ]]; then
    worker_local_exec_read_roots="${worker_artifact_root}"
  fi
  if [[ -z "${worker_local_exec_write_roots}" ]]; then
    worker_local_exec_write_roots="${worker_artifact_root}"
  fi
}

write_bootstrap_context_files() {
  local context_dir="$1"
  local soul_file="${context_dir}/SOUL.md"
  local user_file="${context_dir}/USER.md"

  mkdir -p "${context_dir}"
  cat > "${soul_file}" <<EOF
# SOUL

Beliefs and values:
$(comma_list_to_bullets "${soul_values}")

Tone and style:
  - ${soul_style}

Role:
  - ${agent_role}

Name:
  - ${agent_name}

Boundaries:
$(comma_list_to_bullets "${soul_boundaries}")
EOF

cat > "${user_file}" <<EOF
# USER

Preferred collaboration style:
  - concise updates
  - explicit tradeoffs

Notes:
  - ${soul_style}
  - ${agent_role}
EOF
}

init_solo_lite_profile() {
  (
    cd "${repo_dir}"
    set -a
    source infra/config/profile.solo-lite.env
    set +a
    make solo-lite-init
  )
}

run_solo_lite_bootstrap() {
  local agent_id
  local user_id
  local context_dir
  if [[ -z "${sandbox_root}" ]]; then
    sandbox_root="/opt/agent"
  fi

  if ! command -v make >/dev/null 2>&1; then
    echo "make is required for solo-lite profile initialization." >&2
    exit 1
  fi
  if ! command -v uuidgen >/dev/null 2>&1; then
    echo "uuidgen is required for agent and user seeding." >&2
    exit 1
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required for context file generation." >&2
    exit 1
  fi

  if ! init_solo_lite_profile; then
    echo "failed to initialize solo-lite sqlite profile." >&2
    exit 1
  fi

  sandbox_root="$(to_abs_path "${sandbox_root}")"
  worker_artifact_root="${worker_artifact_root%/}"
  if [[ -z "${worker_artifact_root}" ]]; then
    worker_artifact_root="${sandbox_root}/artifacts"
  fi
  worker_local_exec_read_roots="${worker_local_exec_read_roots%/}"
  if [[ -z "${worker_local_exec_read_roots}" ]]; then
    worker_local_exec_read_roots="${worker_artifact_root}"
  fi
  worker_local_exec_write_roots="${worker_local_exec_write_roots%/}"
  if [[ -z "${worker_local_exec_write_roots}" ]]; then
    worker_local_exec_write_roots="${worker_artifact_root}"
  fi
  worker_artifact_root="$(to_abs_path "${worker_artifact_root}")"
  worker_local_exec_read_roots="$(to_abs_path "${worker_local_exec_read_roots}")"
  worker_local_exec_write_roots="$(to_abs_path "${worker_local_exec_write_roots}")"

  if ! install_state_file="$(mktemp)"; then
    echo "unable to allocate installer state file" >&2
    exit 1
  fi

  agent_id="$(uuidgen)"
  user_id="$(uuidgen)"
  printf "agent_id=%s\nuser_id=%s\n" "${agent_id}" "${user_id}" > "${install_state_file}"

  context_dir="${repo_dir}/agent_context/${tenant_id}/${agent_id}"
  write_bootstrap_context_files "${context_dir}"
}

resolve_solo_light_defaults() {
  if [[ "${service_scope}" == "" ]]; then
    service_scope="system"
  fi

  if [[ "${service_scope}" != "system" && "${service_scope}" != "user" ]]; then
    echo "SECUREAGNT_SERVICE_SCOPE must be system or user (received: ${service_scope})" >&2
    exit 1
  fi

  if [[ "${service_scope}" == "system" && "${EUID}" -ne 0 ]]; then
    echo "system service scope requires root privileges (run this script as root or use SECUREAGNT_SERVICE_SCOPE=user)." >&2
    exit 1
  fi

  if [[ -z "${service_unit_dir}" ]]; then
    if [[ "${service_scope}" == "system" ]]; then
      service_unit_dir="/etc/systemd/system"
    else
      service_unit_dir="${HOME}/.config/systemd/user"
    fi
  fi

  if [[ -z "${solo_light_env_path}" ]]; then
    if [[ "${service_scope}" == "system" ]]; then
      solo_light_env_path="/etc/secureagnt/secureagnt-solo-lite.env"
    else
      solo_light_env_path="${install_home}/secureagnt-solo-lite.env"
    fi
  fi

  if [[ "${service_install_target}" == "" ]]; then
    if [[ "${service_scope}" == "system" ]]; then
      service_install_target="multi-user.target"
    else
      service_install_target="default.target"
    fi
  fi

  if [[ "${service_protect_home}" == "0" || "${service_protect_home}" == "false" ]]; then
    service_protect_home="false"
  elif [[ "${service_protect_home}" == "1" || "${service_protect_home}" == "true" ]]; then
    service_protect_home="true"
  elif [[ "${binary_dir}" == /root/* ]]; then
    service_protect_home="false"
  else
    service_protect_home="true"
  fi

  if [[ "${service_scope}" == "user" ]]; then
    systemctl_scope_flags="--user"
  else
    systemctl_scope_flags=""
  fi

  if [[ -z "${sandbox_root}" ]]; then
    if [[ "${service_scope}" == "system" ]]; then
      sandbox_root="${install_home}"
    else
      sandbox_root="${install_home}"
    fi
  fi

  if [[ -z "${solo_light_data_root}" ]]; then
    if [[ "${service_scope}" == "system" ]]; then
      solo_light_data_root="${sandbox_root}"
    else
      solo_light_data_root="${sandbox_root}"
    fi
  fi
  if [[ -z "${solo_light_log_root}" ]]; then
    solo_light_log_root="${solo_light_data_root}/logs"
  fi
  if [[ -z "${solo_light_artifact_root}" ]]; then
    solo_light_artifact_root="${solo_light_data_root}/artifacts"
  fi
  if [[ -z "${solo_light_db_path}" ]]; then
    solo_light_db_path="${solo_light_data_root}/secureagnt.sqlite3"
  fi
  if [[ -z "${solo_light_local_exec_read_roots}" ]]; then
    solo_light_local_exec_read_roots="${solo_light_artifact_root}"
  fi
  if [[ -z "${solo_light_local_exec_write_roots}" ]]; then
    solo_light_local_exec_write_roots="${solo_light_artifact_root}"
  fi

  sandbox_root="$(to_abs_path "${sandbox_root}")"
  solo_light_data_root="$(to_abs_path "${solo_light_data_root}")"
  solo_light_log_root="$(to_abs_path "${solo_light_log_root}")"
  solo_light_artifact_root="$(to_abs_path "${solo_light_artifact_root}")"
  solo_light_db_path="$(to_abs_path "${solo_light_db_path}")"
  solo_light_local_exec_read_roots="$(to_abs_path "${solo_light_local_exec_read_roots}")"
  solo_light_local_exec_write_roots="$(to_abs_path "${solo_light_local_exec_write_roots}")"

  if [[ "${solo_light_db_path}" != /* ]]; then
    solo_light_db_path="${solo_light_data_root%/}/${solo_light_db_path}"
  fi
  if [[ "${solo_light_local_exec_read_roots}" != /* ]]; then
    solo_light_local_exec_read_roots="${solo_light_data_root%/}/${solo_light_local_exec_read_roots}"
  fi
  if [[ "${solo_light_local_exec_write_roots}" != /* ]]; then
    solo_light_local_exec_write_roots="${solo_light_data_root%/}/${solo_light_local_exec_write_roots}"
  fi
  if [[ "${solo_light_data_root}" != /* ]]; then
    solo_light_data_root="${install_home%/}/${solo_light_data_root}"
  fi
  if [[ "${solo_light_log_root}" != /* ]]; then
    solo_light_log_root="${install_home%/}/${solo_light_log_root}"
  fi
  if [[ "${solo_light_artifact_root}" != /* ]]; then
    solo_light_artifact_root="${install_home%/}/${solo_light_artifact_root}"
  fi

  solo_light_data_root="${solo_light_data_root%/}"
  solo_light_log_root="${solo_light_log_root%/}"
  solo_light_db_path="${solo_light_db_path%/}"
  solo_light_artifact_root="${solo_light_artifact_root%/}"
  solo_light_local_exec_read_roots="${solo_light_local_exec_read_roots%/}"
  solo_light_local_exec_write_roots="${solo_light_local_exec_write_roots%/}"
  sandbox_root="${sandbox_root%/}"

  solo_light_database_url="sqlite:///${solo_light_db_path}"
}

write_solo_light_env() {
  cat > "${solo_light_env_path}" <<EOF
API_BIND=${solo_light_api_bind}
API_RUN_MIGRATIONS=1
API_AGENT_BOOTSTRAP_ENABLED=1
API_TRUSTED_PROXY_AUTH_ENABLED=0
DATABASE_URL=${solo_light_database_url}
WORKER_ID=${solo_light_worker_id}
WORKER_ARTIFACT_ROOT=${solo_light_artifact_root}
WORKER_TRIGGER_SCHEDULER_ENABLED=0
WORKER_MEMORY_COMPACTION_ENABLED=0
WORKER_COMPLIANCE_SIEM_DELIVERY_ENABLED=0
WORKER_COMPLIANCE_SIEM_HTTP_ENABLED=0
WORKER_LOCAL_EXEC_ENABLED=0
WORKER_LOCAL_EXEC_READ_ROOTS=${solo_light_local_exec_read_roots}
WORKER_LOCAL_EXEC_WRITE_ROOTS=${solo_light_local_exec_write_roots}
PAYMENT_NWC_ENABLED=0
PAYMENT_CASHU_ENABLED=0
LLM_MODE=local_first
LLM_CHANNEL_DEFAULTS_JSON=
SOLO_LITE_DATABASE_URL=${solo_light_database_url}
SOLO_LITE_DB_PATH=${solo_light_db_path}
SOLO_LITE_SQLITE_JOURNAL_MODE=WAL
SOLO_LITE_SQLITE_SYNCHRONOUS=NORMAL
SOLO_LITE_SQLITE_BUSY_TIMEOUT_MS=5000
NOSTR_SIGNER_MODE=local_key
NOSTR_SECRET_KEY=
NOSTR_SECRET_KEY_FILE=
NOSTR_NIP46_BUNKER_URI=
NOSTR_NIP46_PUBLIC_KEY=
NOSTR_NIP46_CLIENT_SECRET_KEY=
NOSTR_RELAYS=
NOSTR_PUBLISH_TIMEOUT_MS=4000
EOF
}

write_solo_light_service_file() {
  local unit_name="$1"
  local description="$2"
  local exec_path="$3"
  local unit_file="${service_unit_dir}/${unit_name}"
  local user_line=""
  local group_line=""

  if [[ "${service_scope}" == "system" && -n "${service_user}" ]]; then
    user_line="User=${service_user}"
  fi
  if [[ "${service_scope}" == "system" && -n "${service_group}" ]]; then
    group_line="Group=${service_group}"
  fi

  cat > "${unit_file}" <<EOF
[Unit]
Description=${description}
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
${user_line}
${group_line}
WorkingDirectory=${solo_light_data_root}
EnvironmentFile=-${solo_light_env_path}
ExecStart=${exec_path}
Restart=on-failure
RestartSec=2s
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=${service_protect_home}
ReadWritePaths=${solo_light_data_root} ${solo_light_log_root} ${solo_light_artifact_root}
LimitNOFILE=65536

[Install]
WantedBy=${service_install_target}
EOF
}

apply_service_permissions() {
  chmod "${service_unit_file_mode}" "${service_unit_dir}/${service_api_name}" "${service_unit_dir}/${service_worker_name}"
  chmod "${service_env_file_mode}" "${solo_light_env_path}"
}

start_services_if_requested() {
  if [[ "${start_services}" != "1" ]]; then
    return
  fi

  if ! command -v systemctl >/dev/null 2>&1; then
    echo "[service] systemctl not found; cannot start services. Set SECUREAGNT_START_SERVICES=0 to skip auto-start." >&2
    return 1
  fi

  local systemctl_cmd=(systemctl)
  if [[ "${service_scope}" == "user" ]]; then
    systemctl_cmd+=(--user)
  fi

  if [[ "${service_scope}" == "system" && "${EUID}" -ne 0 ]]; then
    echo "system scope requires root privileges; run as root or set SECUREAGNT_SERVICE_SCOPE=user." >&2
    return 1
  fi

  if [[ "${service_scope}" == "user" && "${EUID}" -eq 0 ]]; then
    echo "user scope is not valid for root execution in this installer flow." >&2
    return 1
  fi

  "${systemctl_cmd[@]}" daemon-reload
  "${systemctl_cmd[@]}" enable --now "${service_api_name}" "${service_worker_name}"
}

run_solo_light_setup() {
  resolve_solo_light_defaults
  mkdir -p "${solo_light_data_root}" "${solo_light_log_root}" "${solo_light_artifact_root}" "$(dirname "${solo_light_db_path}")" "$(dirname "${solo_light_env_path}")" "${service_unit_dir}"
  write_solo_light_env

  write_solo_light_service_file \
    "${service_api_name}" \
    "SecureAgnt Solo-Light API" \
    "${binary_dir}/secureagnt-api"
  write_solo_light_service_file \
    "${service_worker_name}" \
    "SecureAgnt Solo-Light Worker" \
    "${binary_dir}/secureagntd"

  apply_service_permissions
  start_services_if_requested
}

print_bootstrap_summary() {
  local agent_id
  local user_id

  agent_id="$(sed -n 's/^agent_id=//p' "${install_state_file}" | head -n 1)"
  user_id="$(sed -n 's/^user_id=//p' "${install_state_file}" | head -n 1)"

  cat <<EOF
SecureAgnt single-agent bootstrap complete.

Installed binaries:
- ${binary_dir}/secureagnt-api
- ${binary_dir}/secureagntd
- ${binary_dir}/agntctl

Bootstrap workspace:
- ${repo_dir}
- agent context: ${repo_dir}/agent_context/${tenant_id}/${agent_id}
- generated SOUL: ${repo_dir}/agent_context/${tenant_id}/${agent_id}/SOUL.md
- sandbox root: ${sandbox_root}
- worker artifact root: ${worker_artifact_root}
- local exec read roots: ${worker_local_exec_read_roots}
- local exec write roots: ${worker_local_exec_write_roots}

Session identities:
- AGENT_ID=${agent_id}
- USER_ID=${user_id}

Next (interactive operator run):
cd "${repo_dir}" && python3 scripts/ops/solo_lite_chat.py --agent-id "${agent_id}" --user-id "${user_id}"

You can also run ad-hoc one-shot checks:
cd "${repo_dir}" && python3 scripts/ops/solo_lite_agent_run.py --agent-id "${agent_id}" --user-id "${user_id}" --agent-name "${agent_name}" --text "Check in and verify setup."
EOF

echo "Bootstrap completed sqlite initialization and generated SOUL/USER context files."
}

print_solo_light_summary() {
  local api_host="127.0.0.1"
  local api_port="8080"
  local protect_home_message=""

  if [[ "${solo_light_api_bind}" == *:* ]]; then
    api_port="${solo_light_api_bind##*:}"
  fi
  if [[ "${service_protect_home}" == "false" ]]; then
    protect_home_message="Note: ProtectHome=off was selected (system services); this is required when binaries live under ${binary_dir}."
  fi

  cat <<EOF
SecureAgnt single-operator (solo-light) install complete.

Installed binaries:
- ${binary_dir}/secureagnt-api
- ${binary_dir}/secureagntd
- ${binary_dir}/agntctl

Data and runtime paths:
- data root: ${solo_light_data_root}
- database: ${solo_light_db_path}
- artifacts: ${solo_light_artifact_root}
- logs: ${solo_light_log_root}
- config: ${solo_light_env_path}

Services:
- scope: ${service_scope}
- unit directory: ${service_unit_dir}
- API service: ${service_api_name}
- worker service: ${service_worker_name}

Service files were written and are configured to bind API at ${solo_light_api_bind}.
${protect_home_message}

To check service status:
  systemctl ${systemctl_scope_flags} status ${service_api_name}
  systemctl ${systemctl_scope_flags} status ${service_worker_name}

To check API health:
  curl -sf http://${api_host}:${api_port}/v1/ops/summary?window_secs=60

Manual control:
- Edit env: ${solo_light_env_path}
- Restart: systemctl ${systemctl_scope_flags} restart ${service_api_name} ${service_worker_name}
EOF
}

print_dry_run() {
  echo "SecureAgnt solo-light installer dry-run:"
  echo "setup_mode=${setup_mode}"
  echo "non_interactive=${non_interactive}"
  echo "dry_run=${dry_run}"
  echo "release_repo=${release_repo}"
  echo "release_version=${release_version}"
  echo "resolved_release_tag=${resolved_release_tag}"
  echo "platform_tag=${platform_tag}"
  echo "download_binaries=${download_binaries}"
  echo "install_home=${install_home}"
  echo "binary_dir=${binary_dir}"
  echo "worktree_dir=${worktree_dir}"
  echo "source_repo_url=${source_repo_url}"
  echo "source_branch=${source_branch}"

  if [[ "${setup_mode}" == "solo-light" ]]; then
    echo "service_scope=${service_scope}"
    echo "service_unit_dir=${service_unit_dir}"
    echo "service_install_target=${service_install_target}"
    echo "service_protect_home=${service_protect_home}"
    echo "service_unit_file_mode=${service_unit_file_mode}"
    echo "service_env_file_mode=${service_env_file_mode}"
    echo "start_services=${start_services}"
    echo "solo_light_env_path=${solo_light_env_path}"
    echo "solo_light_data_root=${solo_light_data_root}"
    echo "solo_light_log_root=${solo_light_log_root}"
    echo "solo_light_db_path=${solo_light_db_path}"
    echo "solo_light_api_bind=${solo_light_api_bind}"
    echo "solo_light_worker_id=${solo_light_worker_id}"
    echo "solo_light_artifact_root=${solo_light_artifact_root}"
    echo "solo_light_local_exec_read_roots=${solo_light_local_exec_read_roots}"
    echo "solo_light_local_exec_write_roots=${solo_light_local_exec_write_roots}"
  fi

  if [[ "${setup_mode}" == "bootstrap" ]]; then
    echo "sandbox_root=${sandbox_root}"
    echo "worker_artifact_root=${worker_artifact_root}"
    echo "worker_local_exec_read_roots=${worker_local_exec_read_roots}"
    echo "worker_local_exec_write_roots=${worker_local_exec_write_roots}"
    echo "agent_name=${agent_name}"
    echo "agent_role=${agent_role}"
    echo "soul_style=${soul_style}"
    echo "soul_values=${soul_values}"
    echo "soul_boundaries=${soul_boundaries}"
  fi

  echo ""
  echo "No changes made."
}

parse_args "$@"
validate_mode
assert_root_for_system_scope
require_tool curl
require_tool bash
require_linux_x86_64
trap cleanup EXIT

if ! resolve_release_tag; then
  echo "failed to resolve release tag" >&2
  exit 1
fi

if [[ "${setup_mode}" == "bootstrap" ]]; then
  prompt_bootstrap
elif [[ "${setup_mode}" == "solo-light" ]]; then
  resolve_solo_light_defaults
fi

if [[ "${dry_run}" == "1" ]]; then
  print_dry_run
  exit 0
fi

require_tool tar
prepare_workspace
if [[ "${setup_mode}" == "bootstrap" ]]; then
  prepare_repo
fi

install_binaries

if [[ "${setup_mode}" == "bootstrap" ]]; then
  run_solo_lite_bootstrap

  if [[ "${start_services}" == "1" ]]; then
    run_solo_light_setup
    print_solo_light_summary
  else
    print_bootstrap_summary
  fi
else
  run_solo_light_setup
  print_solo_light_summary
fi
 
