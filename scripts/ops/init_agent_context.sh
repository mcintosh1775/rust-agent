#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${ROOT_DIR:-agent_context}"
TENANT_ID="${TENANT_ID:-single}"
AGENT_ID="${AGENT_ID:-}"
AGENT_NAME="${AGENT_NAME:-secureagnt-agent}"
NOSTR_PUBKEY="${NOSTR_PUBKEY:-}"
USER_NOTES="${USER_NOTES:-}"
FORCE_OVERWRITE="${FORCE_OVERWRITE:-0}"

usage() {
  cat <<'USAGE'
Usage:
  bash scripts/ops/init_agent_context.sh --agent-id <uuid> [options]

Options:
  --root <path>          Context root directory (default: agent_context)
  --tenant <id>          Tenant id (default: single)
  --agent-id <uuid>      Agent UUID (required)
  --agent-name <name>    Agent display name for templates
  --nostr-pubkey <npub>  Optional agent Nostr public key to include in IDENTITY.md
  --user-notes <text>    Optional starter USER.md notes
  --force                Overwrite existing files
  -h, --help             Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      ROOT_DIR="$2"
      shift 2
      ;;
    --tenant)
      TENANT_ID="$2"
      shift 2
      ;;
    --agent-id)
      AGENT_ID="$2"
      shift 2
      ;;
    --agent-name)
      AGENT_NAME="$2"
      shift 2
      ;;
    --nostr-pubkey)
      NOSTR_PUBKEY="$2"
      shift 2
      ;;
    --user-notes)
      USER_NOTES="$2"
      shift 2
      ;;
    --force)
      FORCE_OVERWRITE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -z "${AGENT_ID}" ]]; then
  echo "--agent-id is required" >&2
  usage
  exit 1
fi

TARGET_DIR="${ROOT_DIR}/${TENANT_ID}/${AGENT_ID}"
mkdir -p "${TARGET_DIR}/memory" "${TARGET_DIR}/sessions"

write_file() {
  local path="$1"
  local content="$2"
  if [[ -f "${path}" && "${FORCE_OVERWRITE}" != "1" ]]; then
    echo "skip existing: ${path}"
    return 0
  fi
  printf "%s\n" "${content}" > "${path}"
  echo "wrote: ${path}"
}

write_file "${TARGET_DIR}/IDENTITY.md" "# IDENTITY

name: ${AGENT_NAME}
agent_id: ${AGENT_ID}
tenant_id: ${TENANT_ID}
nostr_pubkey: ${NOSTR_PUBKEY}
role: define-this-role
scope: define-this-scope
"

write_file "${TARGET_DIR}/SOUL.md" "# SOUL

Beliefs and values:
- secure-by-default behavior
- auditable actions
- clear communication

Boundaries:
- do not bypass policy
- do not invent authority
- escalate when uncertain for high-risk actions
"

write_file "${TARGET_DIR}/USER.md" "# USER

Preferred collaboration style:
- concise updates
- explicit tradeoffs

Notes:
${USER_NOTES}
"

write_file "${TARGET_DIR}/MEMORY.md" "# MEMORY

Verified long-term facts:
- (add verified facts only)
"

write_file "${TARGET_DIR}/AGENTS.md" "# AGENTS

Operational rules:
- follow workspace AGENTS.md and policy
- keep changes minimal and test-backed
- never bypass capability controls
"

write_file "${TARGET_DIR}/TOOLS.md" "# TOOLS

Tooling boundaries:
- use approved workspace tools only
- prefer deterministic commands
- avoid destructive operations without explicit approval
"

write_file "${TARGET_DIR}/HEARTBEAT.md" "# HEARTBEAT

Proactive intents:
- (add reminder/schedule intents)

Notes:
- intents are compiled into governed triggers
- intents are not direct privileged execution
"

write_file "${TARGET_DIR}/BOOTSTRAP.md" "# BOOTSTRAP

Use this file for first-run setup. After completing bootstrap, write finalized content into:
- IDENTITY.md
- SOUL.md
- USER.md
- HEARTBEAT.md

Suggested prompts:
1. What is this agent's primary role and scope?
2. What tone and collaboration style should it use?
3. What should it always avoid?
4. What proactive heartbeat tasks should it run?

Completion:
- record completion through API:
  POST /v1/agents/{agent_id}/bootstrap/complete
- completion status is appended to:
  sessions/bootstrap.status.jsonl
"

echo
echo "Agent context scaffold ready:"
echo "  ${TARGET_DIR}"
echo
echo "To enable worker loading:"
echo "  export WORKER_AGENT_CONTEXT_ENABLED=1"
echo "  export WORKER_AGENT_CONTEXT_REQUIRED=1"
echo "  export WORKER_AGENT_CONTEXT_ROOT=${ROOT_DIR}"
