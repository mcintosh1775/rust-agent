#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

require_file() {
  local file="$1"
  if [[ ! -f "$file" ]]; then
    echo "m10-signoff: missing required file: $file" >&2
    exit 1
  fi
}

require_pattern() {
  local file="$1"
  local pattern="$2"
  if ! rg -n "$pattern" "$file" >/dev/null 2>&1; then
    echo "m10-signoff: missing required pattern in $file: $pattern" >&2
    exit 1
  fi
}

require_file "infra/systemd/secureagnt.service"
require_file "infra/systemd/secureagnt-api.service"
require_file "infra/launchd/secureagnt.plist"
require_file "infra/launchd/secureagnt-api.plist"
require_file "infra/config/secureagnt.yaml"
require_file "infra/containers/compose.yml"
require_file "infra/containers/Dockerfile.api"
require_file "infra/containers/Dockerfile.worker"
require_file "docs/CROSS_PLATFORM.md"

require_pattern "docs/CROSS_PLATFORM.md" "Ubuntu"
require_pattern "docs/CROSS_PLATFORM.md" "Debian"
require_pattern "docs/CROSS_PLATFORM.md" "Fedora"
require_pattern "docs/CROSS_PLATFORM.md" "RHEL"
require_pattern "docs/CROSS_PLATFORM.md" "Arch"
require_pattern "docs/CROSS_PLATFORM.md" "openSUSE"
require_pattern "docs/CROSS_PLATFORM.md" "macOS"
require_pattern "docs/CROSS_PLATFORM.md" "systemd"
require_pattern "docs/CROSS_PLATFORM.md" "launchd"

echo "m10-signoff: verified cross-platform packaging/docs baseline"
