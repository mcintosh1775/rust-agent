#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

require_file() {
  local file="$1"
  if [[ ! -f "$file" ]]; then
    echo "m10-matrix-gate: missing required file: $file" >&2
    exit 1
  fi
}

require_pattern() {
  local file="$1"
  local pattern="$2"
  if ! rg -n "$pattern" "$file" >/dev/null 2>&1; then
    echo "m10-matrix-gate: missing required pattern in $file: $pattern" >&2
    exit 1
  fi
}

echo "m10-matrix-gate: running m10-signoff"
bash scripts/ops/m10_signoff.sh

echo "m10-matrix-gate: running deploy-preflight"
bash scripts/ops/deploy_preflight.sh

require_file "docs/M10_EXECUTION_CHECKLIST.md"
require_pattern "docs/M10_EXECUTION_CHECKLIST.md" "Ubuntu / Debian"
require_pattern "docs/M10_EXECUTION_CHECKLIST.md" "Fedora / RHEL-family"
require_pattern "docs/M10_EXECUTION_CHECKLIST.md" "Arch"
require_pattern "docs/M10_EXECUTION_CHECKLIST.md" "openSUSE"
require_pattern "docs/M10_EXECUTION_CHECKLIST.md" "macOS"

require_file ".github/workflows/ci.yml"
require_pattern ".github/workflows/ci.yml" "^\\s*portability:"
require_pattern ".github/workflows/ci.yml" "ubuntu-latest"
require_pattern ".github/workflows/ci.yml" "macos-latest"
require_pattern ".github/workflows/ci.yml" "make m10-matrix-gate"

echo "m10-matrix-gate: pass"
