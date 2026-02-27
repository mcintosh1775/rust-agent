#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SESSION_HANDOFF="${REPO_ROOT}/docs/SESSION_HANDOFF.md"
TASK_LEDGER="${REPO_ROOT}/docs/task-ledger.md"

HANDOFF_LANE="${HANDOFF_LANE:-context-control}"
HANDOFF_OWNER="${HANDOFF_OWNER:-$(git -C "${REPO_ROOT}" config --get user.name)}"
HANDOFF_STATUS="${HANDOFF_STATUS:-done}"
HANDOFF_GOAL="${HANDOFF_GOAL:-}"
HANDOFF_COMPLETED="${HANDOFF_COMPLETED:-}"
HANDOFF_RISKS="${HANDOFF_RISKS:-none}"
HANDOFF_NEXT="${HANDOFF_NEXT:-none}"

if [[ -z "${HANDOFF_LANE}" || -z "${HANDOFF_GOAL}" || -z "${HANDOFF_COMPLETED}" || -z "${HANDOFF_NEXT}" ]]; then
  echo "ERROR: required env vars are missing."
  echo "Set HANDOFF_LANE, HANDOFF_GOAL, HANDOFF_COMPLETED, and HANDOFF_NEXT."
  exit 1
fi

if [[ -z "${HANDOFF_OWNER}" ]]; then
  HANDOFF_OWNER="Codex"
fi

sanitize() {
  local value="${1}"
  value="${value//$'\n'/ }"
  value="${value//$'\r'/ }"
  echo "${value}"
}

NOW_UTC="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"

if [[ ! -f "${TASK_LEDGER}" ]]; then
  cat <<EOF > "${TASK_LEDGER}"
# Task Ledger (append-only)

Track high-signal work transitions for future Codex sessions.

## Entry format

- Timestamp:
- Lane:
- Owner:
- Status:
- Goal:
- Completed:
- Risks:
- Next:

## Entries
EOF
fi

{
  echo
  echo "- Timestamp: ${NOW_UTC}"
  echo "  - Lane: ${HANDOFF_LANE}"
  echo "  - Owner: ${HANDOFF_OWNER}"
  echo "  - Status: ${HANDOFF_STATUS}"
  echo "  - Goal: $(sanitize "${HANDOFF_GOAL}")"
  echo "  - Completed: $(sanitize "${HANDOFF_COMPLETED}")"
  echo "  - Risks: $(sanitize "${HANDOFF_RISKS}")"
  echo "  - Next: $(sanitize "${HANDOFF_NEXT}")"
} >> "${TASK_LEDGER}"

python3 - "${SESSION_HANDOFF}" "${NOW_UTC}" "${HANDOFF_LANE}" "${HANDOFF_OWNER}" "${HANDOFF_STATUS}" "${HANDOFF_GOAL}" "${HANDOFF_COMPLETED}" "${HANDOFF_RISKS}" "${HANDOFF_NEXT}" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
now_utc, lane, owner, status, goal, completed, risks, next_step = sys.argv[2:10]
marker = "## Archived sessions"
text = path.read_text()

if marker not in text:
    raise SystemExit(f"marker '{marker}' not found in {path}")

_, archived = text.split(marker, 1)
archived = archived.lstrip("\n")

live_lines = [
    "# SESSION_HANDOFF",
    "",
    "## Purpose",
    "- Keep live handoff state short and deterministic.",
    "- Keep historical handoff context in an archived section so context windows stay manageable.",
    "",
    "## Live checkpoint (canonical)",
    "",
    f"- Updated: {now_utc}",
    f"- Lane: **{lane}**",
    f"- Owner: {owner}",
    f"- Status: {status}",
    f"- Goal: {goal}",
    f"- Completed: {completed}",
    f"- Risks: {risks}",
    f"- Next: {next_step}",
    "",
    marker,
    "",
    archived.rstrip() + "\n",
]

path.write_text("\n".join(live_lines))
PY

echo "Handoff recorded:"
echo "  - lane: ${HANDOFF_LANE}"
echo "  - goal: ${HANDOFF_GOAL}"
echo "  - next: ${HANDOFF_NEXT}"
echo "  - session handoff: ${SESSION_HANDOFF}"
echo "  - task ledger: ${TASK_LEDGER}"
