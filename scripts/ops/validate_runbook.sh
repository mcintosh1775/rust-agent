#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUNBOOK_FILE="${ROOT_DIR}/docs/RUNBOOK.md"

if [[ ! -f "${RUNBOOK_FILE}" ]]; then
  echo "missing runbook file: ${RUNBOOK_FILE}" >&2
  exit 1
fi

required_sections=(
  "## Incident checklist (first 15 minutes)"
  "## Backup and restore drill (Postgres)"
  "## Migration rollback workflow"
  "## Soak check baseline"
  "## Perf baseline capture"
  "## Compliance replay signing-key rotation"
)

for section in "${required_sections[@]}"; do
  if ! rg --fixed-strings --quiet "${section}" "${RUNBOOK_FILE}"; then
    echo "runbook validation failed: missing section '${section}'" >&2
    exit 1
  fi
done

echo "runbook validation: ok"
