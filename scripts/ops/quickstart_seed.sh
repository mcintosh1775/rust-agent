#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE_FILE="${COMPOSE_FILE:-${ROOT_DIR}/infra/containers/compose.yml}"

TENANT_ID="${TENANT_ID:-single}"
AGENT_NAME="${QUICKSTART_AGENT_NAME:-quickstart-agent}"
USER_SUBJECT="${QUICKSTART_USER_SUBJECT:-quickstart-user}"
USER_DISPLAY_NAME="${QUICKSTART_USER_DISPLAY_NAME:-Quickstart User}"
DATABASE_URL="${QUICKSTART_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"

if ! command -v uuidgen >/dev/null 2>&1; then
  echo "uuidgen is required for quickstart seed." >&2
  exit 1
fi

AGENT_ID="${AGENT_ID:-$(uuidgen)}"
USER_ID="${USER_ID:-$(uuidgen)}"

sql_escape() {
  printf "%s" "$1" | sed "s/'/''/g"
}

TENANT_SQL="$(sql_escape "$TENANT_ID")"
AGENT_ID_SQL="$(sql_escape "$AGENT_ID")"
AGENT_NAME_SQL="$(sql_escape "$AGENT_NAME")"
USER_ID_SQL="$(sql_escape "$USER_ID")"
USER_SUBJECT_SQL="$(sql_escape "$USER_SUBJECT")"
USER_DISPLAY_NAME_SQL="$(sql_escape "$USER_DISPLAY_NAME")"

SQL_STATEMENTS="INSERT INTO agents (id, tenant_id, name, status)
VALUES ('${AGENT_ID_SQL}', '${TENANT_SQL}', '${AGENT_NAME_SQL}', 'active')
ON CONFLICT (id) DO NOTHING;

INSERT INTO users (id, tenant_id, external_subject, display_name, status)
VALUES ('${USER_ID_SQL}', '${TENANT_SQL}', '${USER_SUBJECT_SQL}', '${USER_DISPLAY_NAME_SQL}', 'active')
ON CONFLICT (id) DO NOTHING;"

run_with_local_psql() {
  psql "$DATABASE_URL" <<SQL
${SQL_STATEMENTS}
SQL
}

run_with_compose_exec() {
  if [ ! -f "$COMPOSE_FILE" ]; then
    echo "Compose file not found: $COMPOSE_FILE" >&2
    exit 1
  fi

  if command -v podman >/dev/null 2>&1; then
    podman compose -f "$COMPOSE_FILE" exec postgres \
      psql -U postgres -d agentdb -c "$SQL_STATEMENTS"
    return
  fi

  if command -v podman-compose >/dev/null 2>&1; then
    podman-compose -f "$COMPOSE_FILE" exec postgres \
      psql -U postgres -d agentdb -c "$SQL_STATEMENTS"
    return
  fi

  if command -v docker >/dev/null 2>&1; then
    docker compose -f "$COMPOSE_FILE" exec postgres \
      psql -U postgres -d agentdb -c "$SQL_STATEMENTS"
    return
  fi

  echo "No compose runtime found for container seeding fallback." >&2
  exit 1
}

if command -v psql >/dev/null 2>&1; then
  run_with_local_psql
else
  run_with_compose_exec
fi

echo "Quickstart seed complete."
echo "export AGENT_ID=${AGENT_ID}"
echo "export USER_ID=${USER_ID}"
echo "export TENANT_ID=${TENANT_ID}"
