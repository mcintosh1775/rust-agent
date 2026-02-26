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
replace_binaries="${SECUREAGNT_REPLACE_BINARIES:-0}"
api_run_migrations="${SECUREAGNT_API_RUN_MIGRATIONS:-}"
api_run_migrations_set="${SECUREAGNT_API_RUN_MIGRATIONS+x}"
preserve_existing_env="${SECUREAGNT_PRESERVE_EXISTING_ENV:-0}"
replace_binaries_set="${SECUREAGNT_REPLACE_BINARIES+x}"
preserve_existing_env_set="${SECUREAGNT_PRESERVE_EXISTING_ENV+x}"
service_protect_home_set="${SECUREAGNT_SERVICE_PROTECT_HOME+x}"
non_interactive="${SECUREAGNT_NON_INTERACTIVE:-0}"
resolved_release_tag=""
release_resolution_source=""
installed_release_tag=""
auto_upgrade_detected="0"
solo_light_existing_install="0"
solo_light_service_files_state="written"

sandbox_root="${SECUREAGNT_SANDBOX_ROOT:-}"
worker_artifact_root="${WORKER_ARTIFACT_ROOT:-}"
worker_local_exec_read_roots="${WORKER_LOCAL_EXEC_READ_ROOTS:-}"
worker_local_exec_write_roots="${WORKER_LOCAL_EXEC_WRITE_ROOTS:-}"
tenant_id="single"

nostr_signer_mode="${SECUREAGNT_NOSTR_SIGNER_MODE:-local_key}"
nostr_secret_key="${SECUREAGNT_NOSTR_SECRET_KEY:-}"
nostr_secret_key_file="${SECUREAGNT_NOSTR_SECRET_KEY_FILE:-}"
nostr_nip46_bunker_uri="${SECUREAGNT_NOSTR_NIP46_BUNKER_URI:-}"
nostr_nip46_public_key="${SECUREAGNT_NOSTR_NIP46_PUBLIC_KEY:-}"
nostr_nip46_client_secret_key="${SECUREAGNT_NOSTR_NIP46_CLIENT_SECRET_KEY:-}"
nostr_relays="${SECUREAGNT_NOSTR_RELAYS:-}"
nostr_publish_timeout_ms="${SECUREAGNT_NOSTR_PUBLISH_TIMEOUT_MS:-4000}"
nostr_key_root="${SECUREAGNT_NOSTR_KEY_ROOT:-${install_home}/agent_keys}"
nostr_key_id="${SECUREAGNT_NOSTR_KEY_ID:-}"
force_nostr_regenerate="${SECUREAGNT_FORCE_NOSTR_REGENERATE:-0}"
nostr_key_root_set="${SECUREAGNT_NOSTR_KEY_ROOT+x}"
nostr_key_id_set="${SECUREAGNT_NOSTR_KEY_ID+x}"
force_nostr_regenerate_set="${SECUREAGNT_FORCE_NOSTR_REGENERATE+x}"
resolved_nostr_secret_key=""
resolved_nostr_secret_key_file=""
resolved_nostr_npub=""
nostr_keypair_status="not-generated"
nostr_keypair_source="unknown"
slack_webhook_url="${SECUREAGNT_SLACK_WEBHOOK_URL:-${SLACK_WEBHOOK_URL:-}}"
slack_webhook_url_ref="${SECUREAGNT_SLACK_WEBHOOK_URL_REF:-${SLACK_WEBHOOK_URL_REF:-}}"
worker_message_slack_dest_allowlist="${WORKER_MESSAGE_SLACK_DEST_ALLOWLIST:-}"
worker_message_whitenoise_dest_allowlist="${WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST:-}"
slack_webhook_set="${SECUREAGNT_SLACK_WEBHOOK_URL+x}"
slack_webhook_ref_set="${SECUREAGNT_SLACK_WEBHOOK_URL_REF+x}"
worker_message_slack_dest_allowlist_set="${WORKER_MESSAGE_SLACK_DEST_ALLOWLIST+x}"
worker_message_whitenoise_dest_allowlist_set="${WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST+x}"
enable_slack_messaging="0"

dry_run="${SECUREAGNT_DRY_RUN:-0}"
setup_mode="${SECUREAGNT_SETUP_MODE:-bootstrap}"
start_services="${SECUREAGNT_START_SERVICES:-1}"
startup_message_debug="${SECUREAGNT_STARTUP_MESSAGE_DEBUG:-0}"
startup_message_trace_file="${SECUREAGNT_STARTUP_MESSAGE_TRACE_FILE:-/var/log/secureagnt/secureagnt-solo-lite-startup-message.log}"
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
service_log_dir="${SECUREAGNT_LOG_DIR:-/var/log/secureagnt}"
solo_light_api_bind="${SECUREAGNT_SOLO_LIGHT_API_BIND:-0.0.0.0:8080}"
solo_light_worker_id="${SECUREAGNT_SOLO_LIGHT_WORKER_ID:-worker-solo-light-1}"
solo_light_artifact_root="${SECUREAGNT_SOLO_LIGHT_ARTIFACT_ROOT:-}"
solo_light_local_exec_read_roots="${SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_READ_ROOTS:-}"
solo_light_local_exec_write_roots="${SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_WRITE_ROOTS:-}"
solo_light_database_url=""
solo_light_db_dir=""

agent_name="${SECUREAGNT_AGENT_NAME:-solo-lite-agent}"
agent_role="${SECUREAGNT_AGENT_ROLE:-Personal coordinator and operations assistant for a single workspace}"
soul_style="${SECUREAGNT_SOUL_STYLE:-concise, practical, evidence-first}"
soul_values="${SECUREAGNT_SOUL_VALUES:-secure-by-default behavior, auditable actions, clear communication}"
soul_boundaries="${SECUREAGNT_SOUL_BOUNDARIES:-do not bypass policy, do not invent authority, escalate uncertainty for high-risk actions}"

repo_dir=""
install_state_file=""
systemctl_scope_flags=""
startup_destinations=()

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
  SECUREAGNT_DOWNLOAD_BINARIES    Skip release-binary installation (boolean, default true)
  SECUREAGNT_REPLACE_BINARIES     Force binary replacement even when already present (boolean, default false). Auto-enabled when existing install detected.
  SECUREAGNT_PRESERVE_EXISTING_ENV Preserve existing env file (boolean, default false). Auto-enabled when upgrading.
  SECUREAGNT_NON_INTERACTIVE      Non-interactive defaults (boolean)
  SECUREAGNT_DRY_RUN              Print resolved config and exit (boolean)
  SECUREAGNT_START_SERVICES        Start service files after generation (default true; bootstrap and solo-light)
  SECUREAGNT_STARTUP_MESSAGE_DEBUG Enable verbose startup-notification diagnostics (default: false)
  SECUREAGNT_STARTUP_MESSAGE_TRACE_FILE Path for startup notification trace log (default: /var/log/secureagnt/secureagnt-solo-lite-startup-message.log)
  SECUREAGNT_API_RUN_MIGRATIONS    Run API SQLx migrations on startup (boolean, default: auto)
                                  - auto: 1 for first install, 0 for upgrade with existing SQLx history unless explicitly set
  SECUREAGNT_SERVICE_SCOPE         system|user (system default, requires root)
  SECUREAGNT_SERVICE_UNIT_DIR       systemd unit directory
  SECUREAGNT_SERVICE_PROTECT_HOME   Protects /root access for system services (boolean, default true; auto-disabled when install path is /root)
  SECUREAGNT_SOLO_LIGHT_SERVICE_TARGET         systemd install target (solo-light)
  SECUREAGNT_SOLO_LIGHT_ENV_PATH     env file path (solo-light)
  SECUREAGNT_SOLO_LIGHT_DATA_ROOT     data root path (solo-light)
  SECUREAGNT_SOLO_LIGHT_LOG_ROOT      log root path (solo-light)
  SECUREAGNT_LOG_DIR                   service log directory (solo-light, default: /var/log/secureagnt)
  SECUREAGNT_SOLO_LIGHT_DB_PATH       sqlite database path
  SECUREAGNT_SOLO_LIGHT_API_BIND      bind address
  SECUREAGNT_SOLO_LIGHT_WORKER_ID      worker id
  SECUREAGNT_SOLO_LIGHT_ARTIFACT_ROOT  local artifact root
  SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_READ_ROOTS   read allowlist
  SECUREAGNT_SOLO_LIGHT_LOCAL_EXEC_WRITE_ROOTS  write allowlist

  SECUREAGNT_NOSTR_SIGNER_MODE            signer mode (local_key|nip46_signer)
  SECUREAGNT_NOSTR_SECRET_KEY             local nsec/hex secret key override (optional)
  SECUREAGNT_NOSTR_SECRET_KEY_FILE        local key file override (optional)
  SECUREAGNT_NOSTR_NIP46_BUNKER_URI       NIP-46 bunker URI
  SECUREAGNT_NOSTR_NIP46_PUBLIC_KEY       NIP-46 signer public key
  SECUREAGNT_NOSTR_NIP46_CLIENT_SECRET_KEY NIP-46 client secret key
  SECUREAGNT_NOSTR_RELAYS                 Nostr relay CSV list
  SECUREAGNT_NOSTR_PUBLISH_TIMEOUT_MS      Nostr publish timeout in ms
  SECUREAGNT_NOSTR_KEY_ROOT               root directory for generated key files (default: ${SECUREAGNT_INSTALL_HOME}/agent_keys)
  SECUREAGNT_NOSTR_KEY_ID                 key id for generated key folder under key root
  SECUREAGNT_FORCE_NOSTR_REGENERATE       Force regeneration during local_key install/upgrade (boolean, default: false)
  SECUREAGNT_SLACK_WEBHOOK_URL            Slack incoming webhook URL (webhook-based auth) for slack message.send delivery. Treat as secret. Prefer SECUREAGNT_SLACK_WEBHOOK_URL_REF.
  SECUREAGNT_SLACK_WEBHOOK_URL_REF        Reference for Slack webhook URL secret
  WORKER_MESSAGE_SLACK_DEST_ALLOWLIST     Comma-separated Slack channel IDs (e.g. C1234...).
                                         Destination IDs are typically `C...` (public channels),
                                         `G...` (private channels), `D...` (DM), see docs for lookup.
  WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST Optional whitelist for whitenoise destinations

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

coerce_boolean_inputs() {
  download_binaries="${download_binaries:-1}"
  replace_binaries="${replace_binaries:-0}"
  preserve_existing_env="${preserve_existing_env:-0}"
  if [[ "${api_run_migrations_set}" == "1" ]]; then
    validate_bool_value "SECUREAGNT_API_RUN_MIGRATIONS" "${api_run_migrations}"
  fi
  non_interactive="${non_interactive:-0}"
  dry_run="${dry_run:-0}"
  start_services="${start_services:-1}"
  startup_message_debug="${startup_message_debug:-0}"
  force_nostr_regenerate="${force_nostr_regenerate:-0}"

  validate_bool_value "SECUREAGNT_DOWNLOAD_BINARIES" "${download_binaries}"
  validate_bool_value "SECUREAGNT_REPLACE_BINARIES" "${replace_binaries}"
  validate_bool_value "SECUREAGNT_PRESERVE_EXISTING_ENV" "${preserve_existing_env}"
  validate_bool_value "SECUREAGNT_NON_INTERACTIVE" "${non_interactive}"
  validate_bool_value "SECUREAGNT_DRY_RUN" "${dry_run}"
  validate_bool_value "SECUREAGNT_START_SERVICES" "${start_services}"
  validate_bool_value "SECUREAGNT_STARTUP_MESSAGE_DEBUG" "${startup_message_debug}"
  validate_bool_value "SECUREAGNT_FORCE_NOSTR_REGENERATE" "${force_nostr_regenerate}"
  if [[ "${service_protect_home_set}" == "1" ]]; then
    validate_bool_value "SECUREAGNT_SERVICE_PROTECT_HOME" "${service_protect_home}"
  fi
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

prompt_secret() {
  local var_name="$1"
  local prompt_text="$2"
  local default_value="$3"
  local response

  if [[ "${non_interactive}" == "1" ]]; then
    printf -v "$var_name" "%s" "$default_value"
    return 0
  fi

  read -r -s -p "${prompt_text} [${default_value}]: " response
  echo
  if [[ -z "${response}" ]]; then
    response="${default_value}"
  fi
  printf -v "$var_name" "%s" "$response"
}

prompt_bool_yn() {
  local var_name="$1"
  local prompt_text="$2"
  local default_value="$3"
  local response

  if [[ "${non_interactive}" == "1" ]]; then
    if [[ "${default_value}" == "yes" ]]; then
      printf -v "$var_name" "1"
    else
      printf -v "$var_name" "0"
    fi
    return 0
  fi

  while true; do
    read -r -p "${prompt_text} [${default_value}]: " response
    if [[ -z "${response}" ]]; then
      response="${default_value}"
    fi

    case "${response,,}" in
      yes)
        printf -v "$var_name" "1"
        return 0
        ;;
      no)
        printf -v "$var_name" "0"
        return 0
        ;;
      *)
        echo "Please answer yes or no."
        ;;
    esac
  done
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
    release_resolution_source="explicit-release-flag"
    echo "Release resolution: using configured tag '${resolved_release_tag}' (SECUREAGNT_RELEASE_VERSION)" >&2
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
  release_resolution_source="latest-release-metadata"
  echo "Release resolution: resolved latest release to '${resolved_release_tag}' for ${release_repo}" >&2
  return 0
}

normalize_tag() {
  local tag="$1"
  tag="${tag#v}"
  printf "%s" "${tag//[[:space:]]/}"
}

normalize_identifier_slug() {
  local value="$1"
  local slug
  slug="$(printf "%s" "${value}" | tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9._-]+/-/g' | sed -E 's/^-+|[._-]{2,}|-+$//g')"
  if [[ -z "${slug}" ]]; then
    printf "solo-lite-agent"
    return 0
  fi
  printf "%s" "${slug:0:48}"
}

validate_bool_value() {
  local name="$1"
  local value="$2"
  if [[ "${value}" != "0" && "${value}" != "1" ]]; then
    echo "${name} must be 0 or 1 (received: ${value})" >&2
    exit 1
  fi
}

resolve_nostr_key_id() {
  local fallback=""
  if [[ -n "${nostr_key_id}" ]]; then
    return 0
  fi
  fallback="${agent_name:-}"
  nostr_key_id="$(normalize_identifier_slug "${fallback}")"
  if [[ -z "${nostr_key_id}" ]]; then
    nostr_key_id="$(normalize_identifier_slug "${tenant_id}")"
  fi
  if [[ -z "${nostr_key_id}" ]]; then
    nostr_key_id="agent"
  fi
}

detect_installed_release_tag() {
  local cli_tag=""
  installed_release_tag=""

  if [[ -f "${solo_light_env_path}" ]]; then
    installed_release_tag="$(sed -n 's/^SECUREAGNT_RELEASE_TAG=//p' "${solo_light_env_path}" | head -n 1 || true)"
    installed_release_tag="$(printf '%s' "${installed_release_tag//[[:space:]]/}")"
  fi

  if [[ -z "${installed_release_tag}" && -x "${binary_dir}/agntctl" ]]; then
    cli_tag="$("${binary_dir}/agntctl" --version 2>/dev/null | awk 'NF>=2 {print $2}' | head -n 1 || true)"
    cli_tag="$(printf '%s' "${cli_tag//[[:space:]]/}")"
    if [[ -n "${cli_tag}" ]]; then
      installed_release_tag="${cli_tag}"
    fi
  fi
}

detect_existing_solo_light_install() {
  local existing=0
  local check

  for check in \
    "${solo_light_env_path}" \
    "${service_unit_dir}/${service_api_name}" \
    "${service_unit_dir}/${service_worker_name}" \
    "${binary_dir}/secureagnt-api" \
    "${binary_dir}/secureagntd" \
    "${binary_dir}/agntctl"; do
    if [[ -e "${check}" ]]; then
      existing=1
      break
    fi
  done

  printf "%s" "${existing}"
}

check_upgrade_requested_version() {
  local target_norm
  local installed_norm

  detect_installed_release_tag
  target_norm="$(normalize_tag "${resolved_release_tag}")"
  installed_norm="$(normalize_tag "${installed_release_tag}")"

  if [[ -n "${installed_norm}" && "${installed_norm}" == "${target_norm}" ]]; then
    if [[ "${replace_binaries_set}" != "1" ]]; then
      echo "No-op: installed SecureAgnt release is already ${resolved_release_tag}."
      echo "To refresh binaries/services, set SECUREAGNT_REPLACE_BINARIES=1."
      echo "To re-run bootstrap, use SECUREAGNT_SETUP_MODE=bootstrap."
      exit 0
    fi
    echo "Replace requested explicitly; proceeding despite same release tag: ${resolved_release_tag}."
  fi
}

has_sqlx_migration_history() {
  local db_file="${solo_light_db_path}"
  local history_present="0"

  if [[ ! -f "${db_file}" ]]; then
    echo "0"
    return 0
  fi

  if ! command -v python3 >/dev/null 2>&1; then
    echo "0"
    return 0
  fi

  history_present="$(python3 - "${db_file}" <<'PY'
import sqlite3
import sys

db_path = sys.argv[1]
try:
    conn = sqlite3.connect(db_path)
    row = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='__EFMigrationsHistory' LIMIT 1"
    ).fetchone()
    print(1 if row else 0)
except Exception:
    print(0)
finally:
    try:
        conn.close()
    except Exception:
        pass
PY
)"
  echo "${history_present:-0}"
}

resolve_api_run_migrations_setting() {
  local sqlx_history="0"

  if [[ "${api_run_migrations_set}" == "1" ]]; then
    return 0
  fi

  if [[ "${solo_light_existing_install}" != "1" ]]; then
    api_run_migrations="1"
    return 0
  fi

  if [[ -n "${solo_light_db_path}" ]]; then
    sqlx_history="$(has_sqlx_migration_history)"
  fi

  if [[ "${sqlx_history}" == "1" ]]; then
    api_run_migrations="0"
    echo "Detected SQLx migration history in existing DB; defaulting API_RUN_MIGRATIONS=0 for upgrade safety."
  else
    api_run_migrations="1"
  fi
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

  if [[ "${replace_binaries}" != "1" && -x "${target}" ]]; then
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

has_existing_nostr_identity() {
  local candidate_dir=""
  local candidate_nsec=""
  local candidate_npub=""
  local candidate_root=""
  local found_nsec=""
  local found_npub=""
  local found_dir=""

  if [[ -n "${resolved_nostr_secret_key}" ]]; then
    if [[ "${nostr_keypair_source}" == "unknown" ]]; then
      nostr_keypair_source="resolved-env-secret"
    fi
    if [[ "${nostr_keypair_status}" == "not-generated" ]]; then
      nostr_keypair_status="reused-existing"
    fi
    return 0
  fi

  if [[ -n "${resolved_nostr_secret_key_file}" ]]; then
    resolved_nostr_secret_key_file="$(to_abs_path "${resolved_nostr_secret_key_file}")"
    if [[ -f "${resolved_nostr_secret_key_file}" ]]; then
      resolved_nostr_secret_key="$(load_secret_from_file "${resolved_nostr_secret_key_file}" || true)"
      if [[ -n "${resolved_nostr_secret_key}" ]]; then
        if [[ "${nostr_keypair_source}" == "unknown" ]]; then
          nostr_keypair_source="env-file"
        fi
        if [[ "${nostr_keypair_status}" == "not-generated" ]]; then
          nostr_keypair_status="reused-existing"
        fi
        return 0
      fi
    fi
  fi

  if [[ "${force_nostr_regenerate}" == "1" || -z "${nostr_key_id}" ]]; then
    return 1
  fi

  candidate_dir="$(to_abs_path "${nostr_key_root}")/${nostr_key_id}"
  candidate_nsec="${candidate_dir}/nostr.nsec"
  candidate_npub="${candidate_dir}/nostr.npub"
  if [[ -s "${candidate_nsec}" && -s "${candidate_npub}" ]]; then
    resolved_nostr_secret_key_file="${candidate_nsec}"
    resolved_nostr_secret_key="$(load_secret_from_file "${candidate_nsec}" || true)"
    resolved_nostr_npub="$(load_secret_from_file "${candidate_npub}" || true)"
    nostr_keypair_source="reused-existing"
    nostr_keypair_status="reused-existing"
    return 0
  fi

  candidate_dir="$(to_abs_path "${nostr_key_root}")/${tenant_id}/${nostr_key_id}"
  candidate_nsec="${candidate_dir}/nostr.nsec"
  candidate_npub="${candidate_dir}/nostr.npub"
  if [[ -s "${candidate_nsec}" && -s "${candidate_npub}" ]]; then
    resolved_nostr_secret_key_file="${candidate_nsec}"
    resolved_nostr_secret_key="$(load_secret_from_file "${candidate_nsec}" || true)"
    resolved_nostr_npub="$(load_secret_from_file "${candidate_npub}" || true)"
    nostr_keypair_status="reused-existing"
    nostr_keypair_source="reused-existing-legacy"
    return 0
  fi

  candidate_root="$(to_abs_path "${nostr_key_root}")"
  if [[ -d "${candidate_root}" ]]; then
    while IFS= read -r -d '' found_nsec; do
      found_dir="$(dirname "${found_nsec}")"
      found_npub="${found_dir}/nostr.npub"
      if [[ -s "${found_nsec}" && -s "${found_npub}" ]]; then
        resolved_nostr_secret_key_file="${found_nsec}"
        resolved_nostr_secret_key="$(load_secret_from_file "${found_nsec}" || true)"
        resolved_nostr_npub="$(load_secret_from_file "${found_npub}" || true)"
        found_dir="$(basename "${found_dir}")"
        if [[ -n "${found_dir}" ]]; then
          nostr_key_id="${found_dir}"
        fi
        if [[ "${nostr_keypair_source}" == "unknown" ]]; then
          nostr_keypair_source="discovered-fallback"
        fi
        if [[ "${nostr_keypair_status}" == "not-generated" ]]; then
          nostr_keypair_status="reused-existing"
        fi
        return 0
      fi
    done < <(find "${candidate_root}" -type f -name "nostr.nsec" -print0)
  fi

  resolved_nostr_secret_key=""
  resolved_nostr_secret_key_file=""
  resolved_nostr_npub=""
  return 1
}

resolve_or_generate_nostr_identity() {
  local env_key_root=""
  local env_key_id=""

  if [[ -f "${solo_light_env_path}" ]]; then
    if [[ -z "${slack_webhook_url}" ]]; then
      slack_webhook_url="$(read_env_value "${solo_light_env_path}" "SLACK_WEBHOOK_URL" || true)"
    fi
    if [[ -z "${slack_webhook_url_ref}" ]]; then
      slack_webhook_url_ref="$(read_env_value "${solo_light_env_path}" "SLACK_WEBHOOK_URL_REF" || true)"
    fi
    if [[ -z "${worker_message_slack_dest_allowlist}" ]]; then
      worker_message_slack_dest_allowlist="$(read_env_value "${solo_light_env_path}" "WORKER_MESSAGE_SLACK_DEST_ALLOWLIST" || true)"
    fi
    if [[ -z "${worker_message_whitenoise_dest_allowlist}" ]]; then
      worker_message_whitenoise_dest_allowlist="$(read_env_value "${solo_light_env_path}" "WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST" || true)"
    fi
    env_key_root="$(read_env_value "${solo_light_env_path}" "NOSTR_KEY_ROOT" || true)"
    if [[ "${nostr_key_root_set}" != "1" && -n "${env_key_root}" ]]; then
      nostr_key_root="${env_key_root}"
    fi
    env_key_id="$(read_env_value "${solo_light_env_path}" "NOSTR_KEY_ID" || true)"
    if [[ "${nostr_key_id_set}" != "1" && -n "${env_key_id}" ]]; then
      nostr_key_id="${env_key_id}"
    fi
    resolved_nostr_secret_key_file="$(read_env_value "${solo_light_env_path}" "NOSTR_SECRET_KEY_FILE" || true)"
    resolved_nostr_secret_key="$(read_env_value "${solo_light_env_path}" "NOSTR_SECRET_KEY" || true)"
    resolved_nostr_npub="$(read_env_value "${solo_light_env_path}" "NOSTR_NPUB" || true)"
    if has_existing_nostr_identity; then
      if [[ -z "${nostr_keypair_source}" || "${nostr_keypair_source}" == "unknown" ]]; then
        if [[ -n "${resolved_nostr_secret_key}" ]]; then
          nostr_keypair_source="env-secret"
        elif [[ -n "${resolved_nostr_secret_key_file}" ]]; then
          nostr_keypair_source="env-file"
        else
          nostr_keypair_source="env-discovered"
        fi
      fi
      nostr_keypair_status="preserved"
      return 0
    fi
  fi

  resolve_nostr_key_id

  if ! ensure_nostr_keypair; then
    echo "failed to prepare nostr key material." >&2
    return 1
  fi
  return 0
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
  local binaries=("secureagnt-api" "secureagntd" "agntctl" "secureagnt-nostr-keygen")
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

load_secret_from_file() {
  local source_path="$1"
  if [[ ! -f "${source_path}" ]]; then
    return 1
  fi

  local value
  value="$(cat "${source_path}" | tr -d '\r' | tr -d '\n' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  if [[ -z "${value}" ]]; then
    return 1
  fi
  printf "%s" "${value}"
}

read_env_value() {
  local source_path="$1"
  local key="$2"
  local value
  if [[ ! -f "${source_path}" ]]; then
    return 1
  fi
  value="$(sed -n "s/^[[:space:]]*${key}=//p" "${source_path}" | head -n 1 || true)"
  if [[ -z "${value}" ]]; then
    value="$(sed -n "s/^[[:space:]]*export[[:space:]]*${key}=//p" "${source_path}" | head -n 1 || true)"
  fi
  if [[ -z "${value}" ]]; then
    return 1
  fi
  value="$(printf '%s' "${value}" | tr -d '\r' | tr -d '\n' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  if [[ "${value}" == '"'* && "${value}" == *'"' ]]; then
    value="${value#\"}"
    value="${value%\"}"
  fi
  if [[ "${value}" == "'"* && "${value}" == *"'" ]]; then
    value="${value#\'}"
    value="${value%\'}"
  fi
  if [[ -z "${value}" ]]; then
    return 1
  fi
  printf "%s" "${value}"
}

sync_solo_light_env_file() {
  local source_path="$1"
  local tmp_path=""

  tmp_path="$(mktemp)"
  if [[ -f "${source_path}" ]]; then
    grep -vE '^[[:space:]]*(export[[:space:]]*)?(API_RUN_MIGRATIONS|NOSTR_SIGNER_MODE|NOSTR_SECRET_KEY|NOSTR_SECRET_KEY_FILE|NOSTR_NIP46_BUNKER_URI|NOSTR_NIP46_PUBLIC_KEY|NOSTR_NIP46_CLIENT_SECRET_KEY|NOSTR_KEY_ROOT|NOSTR_KEY_ID|NOSTR_RELAYS|NOSTR_PUBLISH_TIMEOUT_MS|SLACK_WEBHOOK_URL|SLACK_WEBHOOK_URL_REF|WORKER_MESSAGE_SLACK_DEST_ALLOWLIST|WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST)=' \
      "${source_path}" > "${tmp_path}" || true
  fi

  {
    echo "API_RUN_MIGRATIONS=${api_run_migrations}"
    echo "NOSTR_SIGNER_MODE=${nostr_signer_mode}"
    echo "NOSTR_SECRET_KEY=${resolved_nostr_secret_key}"
    echo "NOSTR_SECRET_KEY_FILE=${resolved_nostr_secret_key_file}"
    echo "NOSTR_NIP46_BUNKER_URI=${nostr_nip46_bunker_uri}"
    echo "NOSTR_NIP46_PUBLIC_KEY=${nostr_nip46_public_key}"
    echo "NOSTR_NIP46_CLIENT_SECRET_KEY=${nostr_nip46_client_secret_key}"
    echo "NOSTR_KEY_ROOT=${nostr_key_root}"
    echo "NOSTR_KEY_ID=${nostr_key_id}"
    echo "NOSTR_RELAYS=${nostr_relays}"
    echo "NOSTR_PUBLISH_TIMEOUT_MS=${nostr_publish_timeout_ms}"
    echo "SLACK_WEBHOOK_URL=${slack_webhook_url}"
    echo "SLACK_WEBHOOK_URL_REF=${slack_webhook_url_ref}"
    echo "WORKER_MESSAGE_SLACK_DEST_ALLOWLIST=${worker_message_slack_dest_allowlist}"
    echo "WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST=${worker_message_whitenoise_dest_allowlist}"
  } >> "${tmp_path}"

  mv "${tmp_path}" "${source_path}"
}

resolve_solo_light_db_path_from_env() {
  local db_path_value=""
  local database_url=""

  if [[ ! -f "${solo_light_env_path}" ]]; then
    return 1
  fi

  db_path_value="$(read_env_value "${solo_light_env_path}" "SOLO_LITE_DB_PATH" || true)"
  if [[ -n "${db_path_value}" ]]; then
    if [[ "${db_path_value}" == /* ]]; then
      solo_light_db_path="${db_path_value}"
    else
      solo_light_db_path="$(to_abs_path "${db_path_value}")"
    fi
    return 0
  fi

  database_url="$(read_env_value "${solo_light_env_path}" "DATABASE_URL" || true)"
  if [[ "${database_url}" != sqlite:* ]]; then
    return 1
  fi

  if [[ "${database_url}" == "sqlite:///"* ]]; then
    db_path_value="${database_url#sqlite:///}"
    db_path_value="/${db_path_value}"
  elif [[ "${database_url}" == "sqlite://"* ]]; then
    db_path_value="${database_url#sqlite://}"
  else
    db_path_value="${database_url#sqlite:}"
  fi
  db_path_value="${db_path_value%%\?*}"
  if [[ "${db_path_value}" == "/"* ]]; then
    solo_light_db_path="${db_path_value}"
  else
    solo_light_db_path="$(to_abs_path "${db_path_value}")"
  fi

  if [[ "${solo_light_db_path}" == "memory" ]]; then
    return 1
  fi

  return 0
}

trim_csv_value() {
  local value="$1"
  value="$(printf "%s" "${value}" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  printf "%s" "${value}"
}

startup_message_debug_log() {
  if [[ "${startup_message_debug}" == "1" ]]; then
    echo "startup-message-debug: $1" >&2
  fi
}

startup_message_trace() {
  local line="$1"
  printf "%s startup-message-trace: %s\n" "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" "${line}" >> "${startup_message_trace_file}" || true
}

collect_solo_light_startup_destinations() {
  local destination
  local normalized
  local value
  local -a entries

  startup_destinations=()
  startup_message_debug_log "collecting startup destinations"
  startup_message_debug_log "raw slack allowlist: ${worker_message_slack_dest_allowlist:-<not-set>}"
  startup_message_debug_log "raw whitenoise allowlist: ${worker_message_whitenoise_dest_allowlist:-<not-set>}"

  if [[ -n "${worker_message_slack_dest_allowlist}" ]]; then
    IFS="," read -r -a entries <<< "${worker_message_slack_dest_allowlist}"
    for destination in "${entries[@]}"; do
      value="$(trim_csv_value "${destination}")"
      if [[ -z "${value}" ]]; then
        continue
      fi
      if [[ "${value}" == *:* ]]; then
        normalized="${value}"
      else
        normalized="slack:${value}"
      fi
      startup_message_debug_log "normalized destination '${value}' -> '${normalized}'"
      startup_destinations+=("${normalized}")
    done
  fi

  if [[ -n "${worker_message_whitenoise_dest_allowlist}" ]]; then
    IFS="," read -r -a entries <<< "${worker_message_whitenoise_dest_allowlist}"
    for destination in "${entries[@]}"; do
      value="$(trim_csv_value "${destination}")"
      if [[ -z "${value}" ]]; then
        continue
      fi
      if [[ "${value}" == *:* ]]; then
        normalized="${value}"
      else
        normalized="whitenoise:${value}"
      fi
      startup_message_debug_log "normalized destination '${value}' -> '${normalized}'"
      startup_destinations+=("${normalized}")
    done
  fi
  startup_message_debug_log "total startup destinations: ${#startup_destinations[@]}"
}

resolve_solo_light_startup_ids() {
  startup_agent_id=""
  startup_user_id=""
  if [[ -f "${install_state_file}" ]]; then
    startup_message_debug_log "reading startup ids from install state file: ${install_state_file}"
    startup_agent_id="$(sed -n 's/^agent_id=//p' "${install_state_file}" | head -n 1 | tr -d '\r' | tr -d '\n' || true)"
    startup_user_id="$(sed -n 's/^user_id=//p' "${install_state_file}" | head -n 1 | tr -d '\r' | tr -d '\n' || true)"
    startup_message_debug_log "install-state startup ids: agent_id=${startup_agent_id:-<missing>} user_id=${startup_user_id:-<missing>}"
    if [[ -z "${startup_agent_id}" || -z "${startup_user_id}" ]]; then
      startup_message_trace "install-state startup ids incomplete: agent_id=${startup_agent_id:-<missing>} user_id=${startup_user_id:-<missing>}"
    fi
  fi

  if [[ -n "${startup_agent_id}" && -n "${startup_user_id}" ]]; then
    return 0
  fi

  startup_message_debug_log "install state did not provide both ids; probing database path: ${solo_light_db_path}"
  if [[ -z "${solo_light_db_path}" || ! -f "${solo_light_db_path}" ]]; then
    startup_message_debug_log "database path missing or not a file: ${solo_light_db_path:-<missing>}"
    return 1
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    startup_message_debug_log "python3 unavailable for startup id lookup"
    return 1
  fi

  local lookup_json
  if ! lookup_json="$(python3 - "${solo_light_db_path}" "${tenant_id}" <<'PY'
import sqlite3
import sys

db_path = sys.argv[1]
tenant_id = sys.argv[2]

with sqlite3.connect(db_path) as conn:
    conn.row_factory = sqlite3.Row
    cur = conn.cursor()

    agent_row = cur.execute(
        """
        SELECT id FROM agents
        WHERE tenant_id = ?
        ORDER BY created_at DESC
        LIMIT 1
        """,
        [tenant_id],
    ).fetchone()
    user_row = cur.execute(
        """
        SELECT id FROM users
        WHERE tenant_id = ?
        ORDER BY created_at DESC
        LIMIT 1
        """,
        [tenant_id],
    ).fetchone()

    agent_id = agent_row["id"] if agent_row is not None else ""
    user_id = user_row["id"] if user_row is not None else ""
    print(f"{agent_id}|{user_id}")
PY
)" ; then
    return 1
  fi

  startup_agent_id="${lookup_json%%|*}"
  startup_user_id="${lookup_json#*|}"
  startup_message_debug_log "resolved startup ids from database: agent_id=${startup_agent_id:-<missing>} user_id=${startup_user_id:-<missing>}"
  if [[ -z "${startup_agent_id}" || -z "${startup_user_id}" ]]; then
    startup_message_trace "database lookup for startup ids incomplete: agent_id=${startup_agent_id:-<missing>} user_id=${startup_user_id:-<missing>}"
    startup_message_debug_log "startup id resolution failed; missing agent or user id"
    return 1
  fi

  return 0
}

wait_for_solo_light_api() {
  local api_port="8080"
  local attempt
  if [[ "${solo_light_api_bind}" == *:* ]]; then
    api_port="${solo_light_api_bind##*:}"
  fi

  for attempt in $(seq 1 30); do
    if curl -fsS -X GET \
      -H "x-tenant-id: ${tenant_id}" \
      -H "x-user-role: operator" \
      "http://127.0.0.1:${api_port}/v1/ops/summary?window_secs=60" >/dev/null; then
      return 0
    fi
    sleep 1
  done

  return 1
}

wait_for_systemd_service_active() {
  local service="$1"
  local attempt
  local service_cmd=(systemctl)

  if [[ "${service_scope}" == "user" ]]; then
    service_cmd+=(--user)
  fi

  for attempt in $(seq 1 30); do
    if "${service_cmd[@]}" is-active --quiet "${service}"; then
      return 0
    fi
    sleep 1
  done

  return 1
}

verify_solo_light_service_startup() {
  if [[ "${start_services}" != "1" ]]; then
    return 0
  fi

  local service
  local service_cmd=(systemctl)
  local api_port="8080"

  if [[ "${service_scope}" == "user" ]]; then
    service_cmd+=(--user)
  fi

  if [[ "${solo_light_api_bind}" == *:* ]]; then
    api_port="${solo_light_api_bind##*:}"
  fi

  for service in "${service_api_name}" "${service_worker_name}"; do
    if ! wait_for_systemd_service_active "${service}"; then
      echo "Service ${service} failed to become active during startup." >&2
      "${service_cmd[@]}" status "${service}" --no-pager || true
      return 1
    fi
  done

    if ! wait_for_solo_light_api; then
      echo "API health check did not become reachable after service startup." >&2
      echo "API health endpoint: http://127.0.0.1:${api_port}/v1/ops/summary?window_secs=60" >&2
      echo "service scope: ${service_scope}" >&2
      echo "Check logs:" >&2
      echo "  ${service_cmd[@]} -n 80 status ${service_api_name} --no-pager" >&2
      echo "  ${service_cmd[@]} -n 80 status ${service_worker_name} --no-pager" >&2
      echo "  tail -n 80 ${service_log_dir}/${service_api_name%.service}.log" >&2
      echo "  tail -n 80 ${service_log_dir}/${service_worker_name%.service}.log" >&2
      return 1
  fi

  return 0
}

emit_startup_message_for_solo_light() {
  local destination
  local summary_event
  local requested_capability_payload
  local notification_text
  local startup_destination_list
  local api_payload
  local first_destination
  local sent_count=0
  local api_port="8080"
  local api_url
  local startup_run_id
  local startup_destination_count
  local startup_destination_index=0
  local startup_message_reports=()
  local api_response
  local api_rc
  local response_snippet
  local status_line
  local run_headers=(
    -H "x-tenant-id: ${tenant_id}"
    -H "x-user-role: operator"
    -H "x-user-id: ${startup_user_id:-}"
    -H "content-type: application/json"
  )

  if [[ "${start_services}" != "1" ]]; then
    startup_message_trace "startup message skipped: start_services=${start_services}"
    startup_message_debug_log "startup message skipped: start_services=0"
    return 0
  fi
  startup_message_trace "startup message invoked for tenant=${tenant_id} release=${resolved_release_tag:-unknown} existing_install=${solo_light_existing_install}"
  startup_message_debug_log "startup message enabled; resolved_release_tag=${resolved_release_tag:-unknown}"

  if ! resolve_solo_light_startup_ids; then
    echo "Startup notification skipped: unable to resolve agent/user ids (startup_agent_id=${startup_agent_id:-<missing>}, startup_user_id=${startup_user_id:-<missing>})." >&2
    startup_message_trace "startup message skipped: unable to resolve startup ids"
    startup_message_debug_log "startup message skipped: unable to resolve startup ids"
    return 0
  fi
  startup_message_debug_log "startup ids resolved: agent=${startup_agent_id} user=${startup_user_id}"
  startup_message_trace "startup ids resolved: agent=${startup_agent_id} user=${startup_user_id}"

  collect_solo_light_startup_destinations
  startup_message_debug_log "startup destinations resolved to: ${startup_destinations[*]:-<none>}"
  startup_message_trace "startup destinations resolved: ${startup_destinations[*]:-<none>}"
  if [[ "${#startup_destinations[@]}" -eq 0 ]]; then
    startup_message_trace "startup message skipped: no startup destinations configured"
    startup_message_debug_log "startup message skipped: no destinations configured"
    return 0
  fi

  if ! command -v curl >/dev/null 2>&1; then
    echo "Startup notification skipped: curl not available." >&2
    startup_message_trace "startup message skipped: curl not available"
    startup_message_debug_log "startup message skipped: curl unavailable"
    return 0
  fi

  if ! wait_for_solo_light_api; then
    echo "Startup notification skipped: API not responding yet." >&2
    startup_message_trace "startup message skipped: API not responding"
    startup_message_debug_log "startup message skipped: API readiness check failed"
    return 0
  fi
  startup_message_debug_log "API readiness check passed"
  startup_message_trace "startup API readiness check passed on port ${api_port}"

  if [[ "${solo_light_existing_install}" == "1" ]]; then
    summary_event="upgraded to"
  else
    summary_event="first online using"
  fi
  if [[ -z "${resolved_release_tag}" ]]; then
    resolved_release_tag="unknown"
  fi
  first_destination=1
  for destination in "${startup_destinations[@]}"; do
    if [[ "${first_destination}" == "1" ]]; then
      startup_destination_list="${destination}"
      first_destination=0
    else
      startup_destination_list="${startup_destination_list}, ${destination}"
    fi
  done
  notification_text="agent '${agent_name}' is now ${summary_event} SecureAgnt v${resolved_release_tag} (destinations: ${startup_destination_list})."
  startup_message_debug_log "startup message text: ${notification_text}"

  if [[ "${solo_light_api_bind}" == *:* ]]; then
    api_port="${solo_light_api_bind##*:}"
  fi
  api_url="http://127.0.0.1:${api_port}/v1/runs"
  startup_destination_count="${#startup_destinations[@]}"
  startup_message_debug_log "sending startup notifications to ${api_url} for ${startup_destination_count} destination(s)"

  for destination in "${startup_destinations[@]}"; do
    startup_destination_index=$((startup_destination_index + 1))
    startup_message_debug_log "posting startup run ${startup_destination_index}/${startup_destination_count} to ${destination}"
    requested_capability_payload="$(python3 - "${destination}" <<'PY'
import json
import sys

destination = sys.argv[1]
payload = [
        {"capability": "message.send", "scope": destination}
]
print(json.dumps(payload))
PY
 )"
    api_payload="$(python3 - "${destination}" "${startup_agent_id}" "${startup_user_id}" "${notification_text}" "${requested_capability_payload}" <<'PY'
import json
import sys

destination = sys.argv[1]
agent_id = sys.argv[2]
user_id = sys.argv[3]
text = sys.argv[4]
requested = json.loads(sys.argv[5]) if len(sys.argv) > 5 else []
payload = {
    "agent_id": agent_id,
    "triggered_by_user_id": user_id,
    "recipe_id": "notify_v1",
    "input": {
        "text": text,
        "request_message": True,
        "destination": destination,
    },
    "requested_capabilities": requested
}
print(json.dumps(payload))
PY
)"

    api_response="$(curl --fail-with-body -fsS -X POST "${api_url}" "${run_headers[@]}" -d "${api_payload}" 2>&1 || true)"
    api_rc=$?
    startup_message_debug_log "api return code for ${destination}: ${api_rc}"
    startup_message_debug_log "api response for ${destination}: ${api_response:-<empty>}"
    startup_message_trace "destination=${destination} api_return_code=${api_rc}"

    response_snippet="$(printf "%s" "${api_response}" | tr '\n' ' ' | sed 's/[[:space:]]\+/ /g' | cut -c1-240)"
    response_snippet="${response_snippet:-<empty>}"

    status_line="destination=${destination}"
    if [[ "${api_rc}" -eq 0 ]]; then
      sent_count=$((sent_count + 1))
      startup_run_id="$(printf "%s" "${api_response}" | python3 - <<'PY'
import json
import sys

raw = sys.stdin.read()
try:
    body = json.loads(raw)
except Exception:
    sys.exit(0)

run_id = body.get("run_id") or body.get("id")
if run_id is None:
    sys.exit(0)
print(run_id)
PY
)" || startup_run_id=""
      if [[ -n "${startup_run_id}" ]]; then
        status_line+=" -> sent run_id=${startup_run_id}"
      else
        status_line+=" -> sent (run_id unavailable)"
      fi
      startup_message_trace "destination=${destination} accepted run_id=${startup_run_id:-<none>}"
      startup_message_debug_log "startup message accepted for ${destination}, run_id=${startup_run_id:-<none>}"
      startup_message_reports+=("${status_line}")
      continue
    fi
    status_line+=" -> failed (curl_rc=${api_rc}) response=${response_snippet}"
    startup_message_trace "destination=${destination} failed curl_rc=${api_rc} response=${response_snippet}"
    startup_message_reports+=("${status_line}")
    if [[ "${startup_message_debug}" == "1" ]]; then
      echo "Startup notification failed for destination '${destination}'. Response: ${api_response}" >&2
    fi
  done

  echo "Startup notification summary:"
  if [[ "${#startup_message_reports[@]}" -eq 0 ]]; then
    echo "  no destination notifications attempted."
  else
    local report
    for report in "${startup_message_reports[@]}"; do
      echo "  ${report}"
    done
  fi

  if [[ "${sent_count}" -eq 0 ]]; then
    startup_message_trace "startup notifications sent: ${sent_count}/${startup_destination_count}"
    echo "Startup notification not sent to any destination." >&2
    startup_message_debug_log "startup notifications sent: ${sent_count}/${startup_destination_count}"
  else
    startup_message_trace "startup notifications sent: ${sent_count}/${startup_destination_count}"
    startup_message_debug_log "startup notifications sent: ${sent_count}/${startup_destination_count}"
  fi
}

ensure_nostr_keypair() {
  local key_root_path
  local key_dir
  local nsec_path
  local npub_path
  local metadata_path
  local key_payload
  local nsec_value
  local npub_value

  if [[ "${nostr_signer_mode}" != "local_key" ]]; then
    if [[ "${nostr_signer_mode}" != "nip46_signer" ]]; then
      echo "SECUREAGNT_NOSTR_SIGNER_MODE must be local_key or nip46_signer (received: ${nostr_signer_mode})" >&2
      return 1
    fi
    resolved_nostr_secret_key=""
    resolved_nostr_secret_key_file=""
    resolved_nostr_npub=""
    nostr_keypair_status="disabled"
    return 0
  fi

  if [[ "${force_nostr_regenerate_set}" == "1" ]]; then
    validate_bool_value "SECUREAGNT_FORCE_NOSTR_REGENERATE" "${force_nostr_regenerate}"
  fi

  resolved_nostr_secret_key=""
  resolved_nostr_secret_key_file=""
  resolved_nostr_npub=""

  if [[ -n "${nostr_secret_key}" ]]; then
    resolved_nostr_secret_key="${nostr_secret_key}"
    nostr_keypair_source="explicit-env-secret"
    nostr_keypair_status="provided-secret"
    return 0
  fi

  if [[ -n "${nostr_secret_key_file}" ]]; then
    resolved_nostr_secret_key_file="${nostr_secret_key_file}"
    if [[ "${resolved_nostr_secret_key_file}" != /* ]]; then
      resolved_nostr_secret_key_file="$(to_abs_path "${resolved_nostr_secret_key_file}")"
    fi
    resolved_nostr_secret_key="$(load_secret_from_file "${resolved_nostr_secret_key_file}" || true)"
    if [[ -n "${resolved_nostr_secret_key}" ]]; then
      nostr_keypair_source="explicit-env-file"
      nostr_keypair_status="provided-file"
      return 0
    fi
    echo "provided NOSTR_SECRET_KEY_FILE missing/empty; generating keypair in installer defaults." >&2
  fi

  if [[ "${force_nostr_regenerate}" != "1" ]] && has_existing_nostr_identity; then
    return 0
  fi

  resolve_nostr_key_id
  key_root_path="$(to_abs_path "${nostr_key_root}")"
  key_dir="${key_root_path}/${nostr_key_id}"
  nsec_path="${key_dir}/nostr.nsec"
  npub_path="${key_dir}/nostr.npub"
  metadata_path="${key_dir}/keypair.json"

  if [[ "${force_nostr_regenerate}" != "1" ]]; then
    if [[ -s "${nsec_path}" && -s "${npub_path}" ]]; then
      resolved_nostr_secret_key_file="${nsec_path}"
      resolved_nostr_secret_key="$(load_secret_from_file "${nsec_path}" || true)"
      resolved_nostr_npub="$(load_secret_from_file "${npub_path}" || true)"
      nostr_keypair_source="existing-key-dir"
      nostr_keypair_status="reused-existing"
      return 0
    fi

    if [[ -s "${key_root_path}/${tenant_id}/${nostr_key_id}/nostr.nsec" && -s "${key_root_path}/${tenant_id}/${nostr_key_id}/nostr.npub" ]]; then
      mkdir -p "${key_dir}"
      cp "${key_root_path}/${tenant_id}/${nostr_key_id}/nostr.nsec" "${nsec_path}"
      cp "${key_root_path}/${tenant_id}/${nostr_key_id}/nostr.npub" "${npub_path}"
      if [[ -f "${key_root_path}/${tenant_id}/${nostr_key_id}/keypair.json" ]]; then
        cp "${key_root_path}/${tenant_id}/${nostr_key_id}/keypair.json" "${metadata_path}" || true
      fi
      chmod 700 "${key_dir}"
      resolved_nostr_secret_key="$(load_secret_from_file "${nsec_path}" || true)"
      resolved_nostr_secret_key_file="${nsec_path}"
      resolved_nostr_npub="$(load_secret_from_file "${npub_path}" || true)"
      nostr_keypair_source="existing-key-dir-legacy-migrated"
      nostr_keypair_status="reused-existing"
      return 0
    fi
  fi

  if ! ensure_binary "secureagnt-nostr-keygen"; then
    return 1
  fi

  mkdir -p "${key_dir}"
  chmod 700 "${key_dir}"

  key_payload="$("${binary_dir}/secureagnt-nostr-keygen" --json)"
  if [[ -z "${key_payload}" ]]; then
    echo "failed to run secureagnt-nostr-keygen" >&2
    return 1
  fi

  if command -v jq >/dev/null 2>&1; then
    nsec_value="$(printf '%s' "${key_payload}" | jq -r '.nsec' || true)"
    npub_value="$(printf '%s' "${key_payload}" | jq -r '.npub' || true)"
  else
    nsec_value="$(printf '%s' "${key_payload}" | tr -d '\r' | tr -d '\n' | sed -n 's/.*\"nsec\"[[:space:]]*:[[:space:]]*\"\\([^\\\"]*\\)\".*/\\1/p' | head -n 1 || true)"
    npub_value="$(printf '%s' "${key_payload}" | tr -d '\r' | tr -d '\n' | sed -n 's/.*\"npub\"[[:space:]]*:[[:space:]]*\"\\([^\\\"]*\\)\".*/\\1/p' | head -n 1 || true)"
  fi

  if [[ -z "${nsec_value}" || -z "${npub_value}" ]]; then
    echo "secureagnt-nostr-keygen output missing nsec/npub" >&2
    return 1
  fi

  printf '%s\n' "${nsec_value}" > "${nsec_path}"
  printf '%s\n' "${npub_value}" > "${npub_path}"
  cat > "${metadata_path}" <<EOF
{
  "tenant_id": "${tenant_id}",
  "key_id": "${nostr_key_id}",
  "npub": "${npub_value}",
  "nsec_file": "${nsec_path}",
  "generated_by": "installer"
  }
EOF
  chmod 600 "${nsec_path}"
  chmod 600 "${npub_path}"
  chmod 600 "${metadata_path}"
  resolved_nostr_secret_key_file="${nsec_path}"
  resolved_nostr_npub="${npub_value}"
  resolved_nostr_secret_key="${nsec_value}"
  nostr_keypair_source="generated"
  nostr_keypair_status="generated"
  return 0
}

prompt_bootstrap() {
  prompt "agent_name" "Operator, what should the agent be called" "${agent_name}"
  prompt "agent_role" "What is this agent's role?" "${agent_role}"
  prompt "soul_style" "Describe communication style / personality" "${soul_style}"
  prompt "soul_values" "What values should be in SOUL.md? (comma-separated)" "${soul_values}"
  prompt "soul_boundaries" "Hard boundaries for SOUL.md? (comma-separated)" "${soul_boundaries}"

  sandbox_root="${sandbox_root:-${install_home}}"
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
  local db_path_override=""
  local db_url_override=""

  if [[ -n "${solo_light_db_path}" ]]; then
    db_path_override="${solo_light_db_path}"
    db_url_override="sqlite:///${solo_light_db_path}"
  fi

  (
    cd "${repo_dir}"
    set -a
    source infra/config/profile.solo-lite.env
    set +a
    if [[ -n "${db_path_override}" ]]; then
      export SOLO_LITE_DB_PATH="${db_path_override}"
      if [[ -n "${db_url_override}" ]]; then
        export SOLO_LITE_DATABASE_URL="${db_url_override}"
      fi
    fi
    make solo-lite-init
  )
}

seed_solo_lite_identity() {
  local input_agent_id="${agent_id}"
  local input_user_id="${user_id}"
  local user_subject="${tenant_id}-operator"
  local user_display_name="${agent_name} Operator"
  local seed_output
  local seeded_agent_id
  local seeded_user_id

  if [[ -z "${solo_light_db_path}" ]]; then
    echo "solo-lite db path unavailable; cannot seed agent identity rows." >&2
    return 1
  fi

  if ! seed_output="$(python3 - "${solo_light_db_path}" "${tenant_id}" "${input_agent_id}" "${agent_name}" "${input_user_id}" "${user_subject}" "${user_display_name}" <<'PY'
import sqlite3
import sys

db_path, tenant_id, agent_id, agent_name, user_id, user_subject, user_display_name = sys.argv[1:]

conn = sqlite3.connect(db_path)

agent_row = conn.execute(
    """
    INSERT INTO agents (id, tenant_id, name, status)
    VALUES (?, ?, ?, 'active')
    ON CONFLICT(tenant_id, name) DO UPDATE
      SET status = excluded.status
    RETURNING id
    """,
    (agent_id, tenant_id, agent_name),
).fetchone()

user_row = conn.execute(
    """
    INSERT INTO users (id, tenant_id, external_subject, display_name, status)
    VALUES (?, ?, ?, ?, 'active')
    ON CONFLICT(tenant_id, external_subject) DO UPDATE
      SET display_name = excluded.display_name,
          status = excluded.status
    RETURNING id
    """,
    (user_id, tenant_id, user_subject, user_display_name),
).fetchone()

conn.commit()
conn.close()

print(f"agent_id={agent_row[0]}")
print(f"user_id={user_row[0]}")
PY
)"; then
    echo "failed to seed sqlite identity rows for tenant ${tenant_id}." >&2
    return 1
  fi

  seeded_agent_id="$(printf '%s\n' "${seed_output}" | awk -F= '/^agent_id=/{print $2}')"
  seeded_user_id="$(printf '%s\n' "${seed_output}" | awk -F= '/^user_id=/{print $2}')"
  if [[ -z "${seeded_agent_id}" || -z "${seeded_user_id}" ]]; then
    echo "failed to parse seeded identity ids from sqlite init output." >&2
    return 1
  fi

  agent_id="${seeded_agent_id}"
  user_id="${seeded_user_id}"
}

run_solo_lite_bootstrap() {
  local agent_id
  local user_id
  local context_dir
  if [[ -z "${sandbox_root}" ]]; then
    sandbox_root="${install_home}"
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

  if [[ -z "${solo_light_db_path}" ]]; then
    echo "solo-light db path is not configured; cannot write bootstrap identities." >&2
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
  if ! seed_solo_lite_identity; then
    echo "failed to seed agent/user identity rows in sqlite." >&2
    exit 1
  fi
  resolve_nostr_key_id
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

  if [[ "${nostr_signer_mode}" != "local_key" && "${nostr_signer_mode}" != "nip46_signer" ]]; then
    echo "SECUREAGNT_NOSTR_SIGNER_MODE must be local_key or nip46_signer (received: ${nostr_signer_mode})" >&2
    exit 1
  fi

  if [[ "${nostr_signer_mode}" == "local_key" ]]; then
    validate_bool_value "SECUREAGNT_FORCE_NOSTR_REGENERATE" "${force_nostr_regenerate}"
    nostr_key_root="$(to_abs_path "${nostr_key_root}")"
    resolve_nostr_key_id
  else
    nostr_secret_key=""
    nostr_secret_key_file=""
  fi

  if [[ -z "${nostr_publish_timeout_ms}" || ! "${nostr_publish_timeout_ms}" =~ ^[0-9]+$ ]]; then
    nostr_publish_timeout_ms="4000"
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

  if [[ "${service_protect_home_set}" == "1" ]]; then
    if [[ "${service_protect_home}" == "1" ]]; then
      service_protect_home="true"
    else
      service_protect_home="false"
    fi
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
  if [[ -z "${service_log_dir}" ]]; then
    service_log_dir="/var/log/secureagnt"
  fi
  if [[ -z "${solo_light_artifact_root}" ]]; then
    solo_light_artifact_root="${solo_light_data_root}/artifacts"
  fi
  if [[ -z "${solo_light_db_path}" ]]; then
    solo_light_db_path="${solo_light_data_root}/secureagnt.sqlite3"
  fi
  if [[ "${solo_light_existing_install}" == "1" && "${preserve_existing_env}" == "1" ]]; then
    if resolve_solo_light_db_path_from_env; then
      if [[ -z "${solo_light_db_path}" || "${solo_light_db_path}" == "sqlite:"* || "${solo_light_db_path}" == "memory" ]]; then
        solo_light_db_path=""
      fi
    fi
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
  service_log_dir="$(to_abs_path "${service_log_dir}")"
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
  if [[ "${service_log_dir}" != /* ]]; then
    service_log_dir="${install_home%/}/${service_log_dir}"
  fi

  solo_light_data_root="${solo_light_data_root%/}"
  solo_light_log_root="${solo_light_log_root%/}"
  solo_light_db_path="${solo_light_db_path%/}"
  if [[ -n "${solo_light_db_path}" ]]; then
    solo_light_db_dir="$(dirname "${solo_light_db_path}")"
  else
    solo_light_db_dir="${solo_light_data_root}"
  fi
  solo_light_artifact_root="${solo_light_artifact_root%/}"
  service_log_dir="${service_log_dir%/}"
  solo_light_local_exec_read_roots="${solo_light_local_exec_read_roots%/}"
  solo_light_local_exec_write_roots="${solo_light_local_exec_write_roots%/}"
  sandbox_root="${sandbox_root%/}"

  solo_light_database_url="sqlite:///${solo_light_db_path}"
}

write_solo_light_env() {
  cat > "${solo_light_env_path}" <<EOF
API_BIND=${solo_light_api_bind}
API_RUN_MIGRATIONS=${api_run_migrations}
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
NOSTR_SIGNER_MODE=${nostr_signer_mode}
NOSTR_SECRET_KEY=${resolved_nostr_secret_key}
NOSTR_SECRET_KEY_FILE=${resolved_nostr_secret_key_file}
NOSTR_NIP46_BUNKER_URI=${nostr_nip46_bunker_uri}
NOSTR_NIP46_PUBLIC_KEY=${nostr_nip46_public_key}
NOSTR_NIP46_CLIENT_SECRET_KEY=${nostr_nip46_client_secret_key}
NOSTR_KEY_ROOT=${nostr_key_root}
NOSTR_KEY_ID=${nostr_key_id}
NOSTR_RELAYS=${nostr_relays}
NOSTR_PUBLISH_TIMEOUT_MS=${nostr_publish_timeout_ms}
SLACK_WEBHOOK_URL=${slack_webhook_url}
SLACK_WEBHOOK_URL_REF=${slack_webhook_url_ref}
WORKER_MESSAGE_SLACK_DEST_ALLOWLIST=${worker_message_slack_dest_allowlist}
WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST=${worker_message_whitenoise_dest_allowlist}
SECUREAGNT_RELEASE_TAG=${resolved_release_tag}
EOF
}

prompt_communication_config() {
  if [[ -f "${solo_light_env_path}" ]]; then
    if [[ "${slack_webhook_set}" != "1" && -z "${slack_webhook_url}" ]]; then
      slack_webhook_url="$(read_env_value "${solo_light_env_path}" "SLACK_WEBHOOK_URL" || true)"
    fi
    if [[ "${slack_webhook_ref_set}" != "1" && -z "${slack_webhook_url_ref}" ]]; then
      slack_webhook_url_ref="$(read_env_value "${solo_light_env_path}" "SLACK_WEBHOOK_URL_REF" || true)"
    fi
    if [[ "${worker_message_slack_dest_allowlist_set}" != "1" && -z "${worker_message_slack_dest_allowlist}" ]]; then
      worker_message_slack_dest_allowlist="$(read_env_value "${solo_light_env_path}" "WORKER_MESSAGE_SLACK_DEST_ALLOWLIST" || true)"
    fi
    if [[ "${worker_message_whitenoise_dest_allowlist_set}" != "1" && -z "${worker_message_whitenoise_dest_allowlist}" ]]; then
      worker_message_whitenoise_dest_allowlist="$(read_env_value "${solo_light_env_path}" "WORKER_MESSAGE_WHITENOISE_DEST_ALLOWLIST" || true)"
    fi
  fi

  local slack_default="0"
  if [[ -n "${slack_webhook_url}" || -n "${slack_webhook_url_ref}" || -n "${worker_message_slack_dest_allowlist}" ]]; then
    slack_default="1"
  fi
  if [[ "${slack_default}" == "1" ]]; then
    prompt_bool_yn "enable_slack_messaging" "Enable Slack integration? (yes/no)" "yes"
  else
    prompt_bool_yn "enable_slack_messaging" "Enable Slack integration? (yes/no)" "no"
  fi
  validate_bool_value "enable_slack_messaging" "${enable_slack_messaging}"

  if [[ "${enable_slack_messaging}" != "1" ]]; then
    slack_webhook_url=""
    slack_webhook_url_ref=""
    worker_message_slack_dest_allowlist=""
    return 0
  fi

  if [[ -z "${slack_webhook_url}" ]]; then
    if [[ -n "${slack_webhook_url_ref}" ]]; then
      prompt "slack_webhook_url" "Slack webhook URL (required unless using SLACK_WEBHOOK_URL_REF; keep secret)" ""
      if [[ -z "${slack_webhook_url}" && -z "${slack_webhook_url_ref}" ]]; then
        echo "SLACK_WEBHOOK_URL_REF is not set; Slack messages will remain queued in local outbox." >&2
      fi
    else
      prompt "slack_webhook_url" "Slack webhook URL (required to send to slack destinations)" "${slack_webhook_url}"
    fi
  else
    prompt "slack_webhook_url" "Slack webhook URL (optional override)" "${slack_webhook_url}"
  fi

  prompt "worker_message_slack_dest_allowlist" "Slack destination allowlist (comma-separated channel ids)" "${worker_message_slack_dest_allowlist}"

  if [[ -n "${worker_message_slack_dest_allowlist}" ]]; then
    echo "Slack destinations will be allowlisted to: ${worker_message_slack_dest_allowlist}"
  fi
}

write_solo_light_service_file() {
  local unit_name="$1"
  local description="$2"
  local exec_path="$3"
  local log_file
  local unit_file="${service_unit_dir}/${unit_name}"
  local user_line=""
  local group_line=""
  local rw_paths="${solo_light_data_root} ${solo_light_log_root} ${solo_light_artifact_root} ${solo_light_db_dir}"
  local unit_basename="${unit_name%.service}"
  if [[ -n "${solo_light_db_path}" && "${solo_light_db_path}" != "${solo_light_db_dir}" ]]; then
    rw_paths="${rw_paths} ${solo_light_db_path}"
  fi
  if [[ "${unit_basename}" == "${unit_name}" ]]; then
    unit_basename="secureagnt-lite"
  fi
  log_file="${service_log_dir}/${unit_basename}.log"
  rw_paths="${rw_paths} ${service_log_dir}"

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
StandardOutput=append:${log_file}
StandardError=append:${log_file}
Restart=on-failure
RestartSec=2s
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=${service_protect_home}
ReadWritePaths=${rw_paths}
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

  local service

  "${systemctl_cmd[@]}" daemon-reload
  for service in "${service_api_name}" "${service_worker_name}"; do
    if "${systemctl_cmd[@]}" is-active --quiet "${service}"; then
      "${systemctl_cmd[@]}" restart "${service}"
    elif "${systemctl_cmd[@]}" is-enabled --quiet "${service}"; then
      "${systemctl_cmd[@]}" start "${service}"
    else
      "${systemctl_cmd[@]}" enable --now "${service}"
    fi
  done
}

stop_services_if_running() {
  if [[ "${replace_binaries}" != "1" ]]; then
    return
  fi

  if ! command -v systemctl >/dev/null 2>&1; then
    echo "[service] systemctl not found; cannot stop running services before binary replacement." >&2
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

  local service
  for service in "${service_api_name}" "${service_worker_name}"; do
    if "${systemctl_cmd[@]}" is-active --quiet "${service}"; then
      "${systemctl_cmd[@]}" stop "${service}"
    fi
  done
}

run_solo_light_setup() {
  resolve_solo_light_defaults
  mkdir -p "${solo_light_data_root}" "${solo_light_log_root}" "${solo_light_artifact_root}" "${service_log_dir}" "$(dirname "${solo_light_db_path}")" "$(dirname "${solo_light_env_path}")" "${service_unit_dir}"
  local write_env="0"
  local write_service_files="1"
  solo_light_service_files_state="written"

  if [[ "${solo_light_existing_install}" != "1" ]]; then
    write_env="1"
    if ! resolve_or_generate_nostr_identity; then
      exit 1
    fi
  else
    if ! resolve_or_generate_nostr_identity; then
      exit 1
    fi
    if [[ "${preserve_existing_env}" == "0" ]]; then
      write_env="1"
    fi
    if [[ "${preserve_existing_env}" == "1" && ! has_existing_nostr_identity ]]; then
      write_env="1"
    fi
    if [[ ! -f "${solo_light_env_path}" ]]; then
      write_env="1"
    fi
    if [[ "${solo_light_existing_install}" == "1" && "${replace_binaries}" == "1" && "${preserve_existing_env}" == "1" ]]; then
      write_service_files="0"
      solo_light_service_files_state="preserved-existing"
    fi
  fi

  if [[ "${write_env}" == "1" ]]; then
    write_solo_light_env
  elif [[ "${preserve_existing_env}" == "1" ]]; then
    sync_solo_light_env_file "${solo_light_env_path}"
  fi

  if [[ "${write_service_files}" == "1" ]]; then
    solo_light_service_files_state="written"
    write_solo_light_service_file \
      "${service_api_name}" \
      "SecureAgnt Solo-Light API" \
      "${binary_dir}/secureagnt-api"
    write_solo_light_service_file \
      "${service_worker_name}" \
      "SecureAgnt Solo-Light Worker" \
      "${binary_dir}/secureagntd"
  else
    solo_light_service_files_state="preserved-existing"
    echo "Preserving existing service files (${service_api_name}, ${service_worker_name}) due upgrade mode." >&2
  fi

  apply_service_permissions
  start_services_if_requested
  if ! verify_solo_light_service_startup; then
    return 1
  fi
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
- NOSTR_SIGNER_MODE=${nostr_signer_mode}
- NOSTR_KEYPAIR_STATUS=${nostr_keypair_status}
- NOSTR_KEY_ID=${nostr_key_id}
- NOSTR_KEY_ROOT=${nostr_key_root}
- NOSTR_KEYPAIR_SOURCE=${nostr_keypair_source}
- NOSTR_SECRET_KEY_FILE=${resolved_nostr_secret_key_file}

Next (interactive operator run):
cd "${repo_dir}" && python3 scripts/ops/solo_lite_chat.py --agent-id "${agent_id}" --user-id "${user_id}"

You can also run ad-hoc one-shot checks:
cd "${repo_dir}" && python3 scripts/ops/solo_lite_agent_run.py --agent-id "${agent_id}" --user-id "${user_id}" --agent-name "${agent_name}" --text "Check in and verify setup."
EOF

echo "Bootstrap completed sqlite identity seeding, sqlite initialization, and generated SOUL/USER context files."
}

print_solo_light_summary() {
  local api_host="127.0.0.1"
  local api_port="8080"
  local api_log_file="${service_log_dir}/${service_api_name%.service}.log"
  local worker_log_file="${service_log_dir}/${service_worker_name%.service}.log"
  local slack_configured="no"
  local protect_home_message=""
  if [[ -n "${slack_webhook_url}" || -n "${slack_webhook_url_ref}" || -n "${worker_message_slack_dest_allowlist}" || -n "${worker_message_whitenoise_dest_allowlist}" ]]; then
    slack_configured="yes"
  fi

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
- service logs: ${service_log_dir}
- startup install trace: ${startup_message_trace_file}
- config: ${solo_light_env_path}

Services:
- scope: ${service_scope}
- unit directory: ${service_unit_dir}
- API service: ${service_api_name}
- worker service: ${service_worker_name}
- preserve_existing_env=${preserve_existing_env}
- Nostr signer mode: ${nostr_signer_mode}
- Nostr key pair status: ${nostr_keypair_status}
- Nostr key pair source: ${nostr_keypair_source}
- Nostr key file: ${resolved_nostr_secret_key_file:-<not-set>}
- Nostr public key: ${resolved_nostr_npub:-<not-set>}
- Nostr key root: ${nostr_key_root}
- Nostr key id: ${nostr_key_id}
- Slack messaging configured: ${slack_configured}
- Slack webhook: ${slack_webhook_url:+<set>} ${slack_webhook_url_ref:+(ref)}
- Slack webhook ref: ${slack_webhook_url_ref:+<set>}
- Slack destination allowlist: ${worker_message_slack_dest_allowlist:-<not-set>}
- Startup message debug: ${startup_message_debug}
- White Noise destination allowlist: ${worker_message_whitenoise_dest_allowlist:-<not-set>}
- API migrations on startup: ${api_run_migrations:-<unknown>}
- Startup message: sent on startup when destinations are configured

Service file setup:
  - status: ${solo_light_service_files_state}
  - API bind: ${solo_light_api_bind}
EOF
if [[ "${solo_light_service_files_state}" != "written" ]]; then
  cat <<EOF
  - existing unit files were preserved during upgrade.
EOF
fi
cat <<EOF
${protect_home_message}

To check service status:
  systemctl ${systemctl_scope_flags} status ${service_api_name}
  systemctl ${systemctl_scope_flags} status ${service_worker_name}

To check API health:
  curl -sf -H 'x-tenant-id: single' http://${api_host}:${api_port}/v1/ops/summary?window_secs=60

Post-install verification:
  systemctl ${systemctl_scope_flags} is-active ${service_api_name} ${service_worker_name}
  curl -sf -H 'x-tenant-id: single' -H 'x-user-role: operator' http://${api_host}:${api_port}/v1/ops/summary?window_secs=60
  tail -n 40 ${api_log_file}
  tail -n 40 ${worker_log_file}

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
  echo "release_resolution_source=${release_resolution_source}"
  echo "installed_release_tag=${installed_release_tag}"
  echo "platform_tag=${platform_tag}"
  echo "download_binaries=${download_binaries}"
  echo "replace_binaries=${replace_binaries}"
  echo "preserve_existing_env=${preserve_existing_env}"
  echo "startup_message_debug=${startup_message_debug}"
  echo "api_run_migrations=${api_run_migrations:-<auto>}"
  echo "auto_upgrade_detected=${auto_upgrade_detected}"
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
    echo "service_log_dir=${service_log_dir}"
    echo "solo_light_db_path=${solo_light_db_path}"
    echo "solo_light_api_bind=${solo_light_api_bind}"
    echo "solo_light_worker_id=${solo_light_worker_id}"
    echo "solo_light_artifact_root=${solo_light_artifact_root}"
    echo "solo_light_local_exec_read_roots=${solo_light_local_exec_read_roots}"
    echo "solo_light_local_exec_write_roots=${solo_light_local_exec_write_roots}"
    echo "nostr_signer_mode=${nostr_signer_mode}"
    echo "nostr_secret_key_file=${resolved_nostr_secret_key_file}"
    echo "nostr_nip46_bunker_uri=${nostr_nip46_bunker_uri}"
    echo "nostr_publish_timeout_ms=${nostr_publish_timeout_ms}"
    echo "nostr_key_root=${nostr_key_root}"
    echo "nostr_key_id=${nostr_key_id}"
    echo "force_nostr_regenerate=${force_nostr_regenerate}"
    echo "nostr_keypair_status=${nostr_keypair_status}"
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
    echo "nostr_signer_mode=${nostr_signer_mode}"
    echo "nostr_secret_key_file=${resolved_nostr_secret_key_file}"
    echo "nostr_key_root=${nostr_key_root}"
    echo "nostr_key_id=${nostr_key_id}"
    echo "force_nostr_regenerate=${force_nostr_regenerate}"
    echo "nostr_publish_timeout_ms=${nostr_publish_timeout_ms}"
    echo "nostr_keypair_status=${nostr_keypair_status}"
    echo "enable_slack_messaging=${enable_slack_messaging}"
    echo "slack_webhook_url=${slack_webhook_url}"
    echo "worker_message_slack_dest_allowlist=${worker_message_slack_dest_allowlist}"
    echo "worker_message_whitenoise_dest_allowlist=${worker_message_whitenoise_dest_allowlist}"
  fi

  echo ""
  echo "No changes made."
}

parse_args "$@"
coerce_boolean_inputs
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
  resolve_solo_light_defaults
  prompt_bootstrap
  prompt_communication_config
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

if [[ "${setup_mode}" == "solo-light" ]]; then
  if [[ "$(detect_existing_solo_light_install)" == "1" ]]; then
    solo_light_existing_install="1"
  else
    solo_light_existing_install="0"
  fi

  if [[ "${replace_binaries_set}" == "" && "${preserve_existing_env_set}" == "" ]]; then
    if [[ "${solo_light_existing_install}" == "1" ]]; then
      auto_upgrade_detected="1"
      replace_binaries="1"
      preserve_existing_env="1"
      echo "Detected existing secureagnt solo-light install; enabling upgrade defaults:"
      echo "  - SECUREAGNT_REPLACE_BINARIES=1"
      echo "  - SECUREAGNT_PRESERVE_EXISTING_ENV=1"
    fi
  fi
  check_upgrade_requested_version

  if ! stop_services_if_running; then
    exit 1
  fi
  resolve_api_run_migrations_setting
elif [[ "${setup_mode}" == "bootstrap" ]]; then
  if [[ "$(detect_existing_solo_light_install)" == "1" ]]; then
    solo_light_existing_install="1"
  else
    solo_light_existing_install="0"
  fi

  if [[ "${replace_binaries_set}" == "" ]]; then
    if [[ "${solo_light_existing_install}" == "1" ]]; then
      replace_binaries="1"
      echo "Detected existing secureagnt solo-light install; enabling bootstrap binary replacement default (SECUREAGNT_REPLACE_BINARIES=1)."
    fi
  fi

  check_upgrade_requested_version

  if ! stop_services_if_running; then
    exit 1
  fi
  resolve_api_run_migrations_setting
fi

install_binaries

if [[ "${setup_mode}" == "bootstrap" ]]; then
  run_solo_lite_bootstrap

  if [[ "${start_services}" == "1" ]]; then
    run_solo_light_setup
    print_solo_light_summary
    emit_startup_message_for_solo_light
  else
  if ! resolve_or_generate_nostr_identity; then
      echo "failed to prepare nostr key material." >&2
      exit 1
    fi
    print_bootstrap_summary
  fi
else
  run_solo_light_setup
  print_solo_light_summary
  emit_startup_message_for_solo_light
fi
 
