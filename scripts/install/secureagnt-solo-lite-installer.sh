#!/usr/bin/env bash

set -euo pipefail

install_home="${SECUREAGNT_INSTALL_HOME:-${HOME}/.secureagnt}"
binary_dir="${SECUREAGNT_BINARY_DIR:-${install_home}/bin}"
worktree_dir="${SECUREAGNT_WORKTREE:-${install_home}/source}"
release_repo="${SECUREAGNT_RELEASE_REPO:-nearai/secureagnt}"
release_version="${SECUREAGNT_RELEASE_VERSION:-latest}"
release_base_url="https://github.com/${release_repo}/releases/download"
source_repo_url="${SECUREAGNT_SOURCE_REPO_URL:-https://github.com/${release_repo}.git}"
source_branch="${SECUREAGNT_SOURCE_BRANCH:-main}"
platform_tag="${SECUREAGNT_PLATFORM_TAG:-linux-x86_64}"
download_binaries="${SECUREAGNT_DOWNLOAD_BINARIES:-1}"
non_interactive="${SECUREAGNT_NON_INTERACTIVE:-0}"
sandbox_root="${SECUREAGNT_SANDBOX_ROOT:-/opt/agent}"
worker_artifact_root="${WORKER_ARTIFACT_ROOT:-}"
worker_local_exec_read_roots="${WORKER_LOCAL_EXEC_READ_ROOTS:-}"
worker_local_exec_write_roots="${WORKER_LOCAL_EXEC_WRITE_ROOTS:-}"
tenant_id="single"

api_base_url="http://localhost:18080"
repo_dir=""
install_state_file=""
agent_name=""
agent_role=""
soul_style=""
soul_values=""
soul_boundaries=""

usage() {
  cat <<'USAGE'
Usage: secureagnt-solo-lite-installer [--help]

Environment variables:
  SECUREAGNT_INSTALL_HOME      Installer workspace (default: $HOME/.secureagnt)
  SECUREAGNT_BINARY_DIR        Installed binary directory (default: $SECUREAGNT_INSTALL_HOME/bin)
  SECUREAGNT_WORKTREE         Source checkout used for bootstrap scripts (default: $SECUREAGNT_INSTALL_HOME/source)
  SECUREAGNT_RELEASE_REPO      GitHub release org/repo (default: nearai/secureagnt)
  SECUREAGNT_RELEASE_VERSION   Release tag (default: latest)
  SECUREAGNT_SOURCE_REPO_URL   Source git URL fallback for local bootstrap scripts (default: https://github.com/<repo>.git)
  SECUREAGNT_SOURCE_BRANCH     Git source branch (default: main)
  SECUREAGNT_PLATFORM_TAG      Binary asset suffix (default: linux-x86_64)
  SECUREAGNT_DOWNLOAD_BINARIES Skip release-binary installation (0/1, default 1)
  SECUREAGNT_NON_INTERACTIVE    Non-interactive defaults (0/1)
  SECUREAGNT_SANDBOX_ROOT      Absolute worker sandbox root (default: /opt/agent)
  WORKER_ARTIFACT_ROOT         Absolute worker artifact root (default: <SECUREAGNT_SANDBOX_ROOT>/artifacts)
  WORKER_LOCAL_EXEC_READ_ROOTS  Comma-separated read allowlist for local.exec templates
  WORKER_LOCAL_EXEC_WRITE_ROOTS Comma-separated write allowlist for local.exec templates

Examples:
  SECUREAGNT_NON_INTERACTIVE=1 bash secureagnt-solo-lite-installer.sh
  SECUREAGNT_RELEASE_REPO=nearai/secureagnt SECUREAGNT_RELEASE_VERSION=v0.2.0 bash secureagnt-solo-lite-installer.sh
USAGE
}

if [[ "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

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

download_one_binary() {
  local binary="$1"
  local tmp_dir
  local archive
  local downloaded_file
  local tmp_file
  local final_path
  local archive_tag
  local tag_source
  local release_tag_payload

  tag_source="${release_version}"
  if [[ "${release_version}" == "latest" ]]; then
    if command -v curl >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
      release_tag_payload="$(curl -fsSL "https://api.github.com/repos/${release_repo}/releases/latest" | jq -r '.tag_name' || true)"
      if [[ -n "${release_tag_payload}" && "${release_tag_payload}" != "null" ]]; then
        tag_source="${release_tag_payload}"
      fi
    fi
  fi

  archive_tag="${tag_source//\//-}"

  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "${tmp_dir}"' RETURN

  for archive in \
    "${binary}-${platform_tag}-${archive_tag}.tar.gz" \
    "${binary}-${platform_tag}-${archive_tag}" \
    "${binary}-${platform_tag}.tar.gz" \
    "${binary}-${platform_tag}" \
    "${binary}.tar.gz" \
    "${binary}"; do
    local url="${release_base_url}/${release_version}/${archive}"
    downloaded_file="${tmp_dir}/${archive}"
    echo "attempting binary fetch: ${url}"

    if curl -fsSLo "${downloaded_file}" "${url}"; then
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
    echo "release download failed for ${binary}; falling back to local build" >&2
  fi

  return 1
}

build_binaries_from_source() {
  local binary="$1"

  if [[ "${binary}" == "secureagnt-api" ]]; then
    (cd "${repo_dir}" && cargo build --release -p api --bin secureagnt-api)
    cp "${repo_dir}/target/release/secureagnt-api" "${binary_dir}/secureagnt-api"
  elif [[ "${binary}" == "secureagntd" ]]; then
    (cd "${repo_dir}" && cargo build --release -p worker --bin secureagntd)
    cp "${repo_dir}/target/release/secureagntd" "${binary_dir}/secureagntd"
  elif [[ "${binary}" == "agntctl" ]]; then
    (cd "${repo_dir}" && cargo build --release -p agntctl)
    cp "${repo_dir}/target/release/agntctl" "${binary_dir}/agntctl"
  fi
  chmod +x "${binary_dir}/${binary}"
}

prepare_repo() {
  if [[ -d "${worktree_dir}/.git" ]]; then
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
  mkdir -p "${binary_dir}"
}

install_binaries() {
  local binary
  local binaries=("secureagnt-api" "secureagntd" "agntctl")
  local built_any=0

  for binary in "${binaries[@]}"; do
    if ! ensure_binary "${binary}"; then
      built_any=1
    fi
  done

  if [[ "${built_any}" == "1" ]]; then
    require_tool git
    require_tool cargo
    prepare_repo
    for binary in "${binaries[@]}"; do
      if [[ ! -x "${binary_dir}/${binary}" ]]; then
        build_binaries_from_source "${binary}"
      fi
    done
  fi
}

run_solo_lite_setup() {
  local run_log
  local agent_id
  local user_id
  local context_dir
  local soul_file
  local user_file
  if [[ -z "${sandbox_root}" ]]; then
    sandbox_root="/opt/agent"
  fi

  if [[ ! -f "${repo_dir}/scripts/ops/solo_lite_agent_run.py" ]]; then
    echo "cannot find bootstrap helper: ${repo_dir}/scripts/ops/solo_lite_agent_run.py" >&2
    exit 1
  fi

  if [[ ! -x "${repo_dir}/scripts/ops/solo_lite_agent_run.py" ]] && ! command -v python3 >/dev/null 2>&1; then
    echo "python3 is required for solo-lite agent bootstrap." >&2
    exit 1
  fi

  if ! command -v make >/dev/null 2>&1; then
    echo "make is required for solo-lite profile initialization." >&2
    exit 1
  fi
  if ! command -v uuidgen >/dev/null 2>&1; then
    echo "uuidgen is required for agent and user seeding." >&2
    exit 1
  fi

  if ! command -v jq >/dev/null 2>&1; then
    echo "warning: jq not found; install not required for bootstrap but recommended for debugging."
  fi
  if ! command -v compose >/dev/null 2>&1; then
    if ! command -v "podman" >/dev/null 2>&1 && ! command -v "podman-compose" >/dev/null 2>&1 && ! command -v "docker" >/dev/null 2>&1; then
      echo "container runtime required for solo-lite bootstrap (podman/docker)." >&2
      exit 1
    fi
  fi

  run_log="$(mktemp)"
  (
    cd "${repo_dir}"
    export SECUREAGNT_SANDBOX_ROOT="${sandbox_root}"
    export WORKER_ARTIFACT_ROOT="${worker_artifact_root}"
    export WORKER_LOCAL_EXEC_READ_ROOTS="${worker_local_exec_read_roots}"
    export WORKER_LOCAL_EXEC_WRITE_ROOTS="${worker_local_exec_write_roots}"
    set -a
    source infra/config/profile.solo-lite.env
    set +a
    make solo-lite-init
    python3 scripts/ops/solo_lite_agent_run.py \
      --agent-name "${agent_name}" \
      --context-root "agent_context" \
      --text "Set up single-agent persona for ${agent_name}: ${soul_style}" \
      --summary-style summary
  ) | tee "${run_log}"

  agent_id="$(grep '^export AGENT_ID=' "${run_log}" | tail -n 1 | cut -d '=' -f2- || true)"
  user_id="$(grep '^export USER_ID=' "${run_log}" | tail -n 1 | cut -d '=' -f2- || true)"
  rm -f "${run_log}"

  if [[ -z "${agent_id}" || -z "${user_id}" ]]; then
    echo "bootstrap did not emit AGENT_ID and USER_ID exports" >&2
    exit 1
  fi

  if ! install_state_file="$(mktemp)"; then
    echo "unable to allocate installer state file" >&2
    exit 1
  fi
  printf "agent_id=%s\nuser_id=%s\n" "${agent_id}" "${user_id}" > "${install_state_file}"

  context_dir="${repo_dir}/agent_context/${tenant_id}/${agent_id}"
  soul_file="${context_dir}/SOUL.md"
  user_file="${context_dir}/USER.md"
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

print_summary() {
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
}

require_tool curl
require_tool bash
require_linux_x86_64
require_tool python3
require_tool make
require_tool uuidgen
require_tool tar
trap cleanup EXIT

prepare_workspace
prepare_repo

prompt "agent_name" "Operator, what should the agent be called" "solo-lite-agent"
prompt "agent_role" "What is this agent's role?" "Personal coordinator and operations assistant for a single workspace"
prompt "soul_style" "Describe communication style / personality" "concise, practical, evidence-first"
prompt "soul_values" "What values should be in SOUL.md? (comma-separated)" "secure-by-default behavior, auditable actions, clear communication"
prompt "soul_boundaries" "Hard boundaries for SOUL.md? (comma-separated)" "do not bypass policy, do not invent authority, escalate uncertainty for high-risk actions"
prompt "sandbox_root" "What root directory should constrain agent filesystem access?" "/opt/agent"
prompt "worker_artifact_root" "Absolute worker artifact root (also local.exec default):" "${sandbox_root%/}/artifacts"
prompt "worker_local_exec_read_roots" "Comma-separated absolute local.exec read roots (blank to use worker artifact root)" "${worker_artifact_root}"
prompt "worker_local_exec_write_roots" "Comma-separated absolute local.exec write roots (blank to use worker artifact root)" "${worker_local_exec_write_roots:-${worker_local_exec_read_roots:-${worker_artifact_root}}}"

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

install_binaries
run_solo_lite_setup
print_summary
