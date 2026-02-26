#!/usr/bin/env bash
set -euo pipefail

DB_PATH="${1:-/opt/secureagnt/secureagnt.sqlite3}"
TENANT_ID="${2:-single}"

if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "sqlite3 is required" >&2
  exit 1
fi

if [[ ! -f "${DB_PATH}" ]]; then
  echo "SQLite database not found: ${DB_PATH}" >&2
  exit 1
fi

echo "Inspecting tenant '${TENANT_ID}' in ${DB_PATH}"
echo

sqlite3 "${DB_PATH}" <<SQL
.headers on
.mode column

.print '=== Startup runs (notify_v1, latest 20) ==='
SELECT
  id,
  status,
  created_at,
  triggered_by_user_id,
  substr(input_json,1,160) AS input_json_preview
FROM runs
WHERE tenant_id = '${TENANT_ID}' AND recipe_id = 'notify_v1'
ORDER BY created_at DESC
LIMIT 20;

.print '=== Steps and action.request rows for startup runs ==='
SELECT
  r.id AS run_id,
  s.id AS step_id,
  s.status AS step_status,
  ar.id AS action_request_id,
  ar.action_type,
  ar.status AS action_request_status,
  json_extract(ar.args_json, '$.destination') AS destination,
  substr(json_extract(ar.args_json,'$.text'),1,160) AS requested_text
FROM runs r
JOIN steps s ON s.run_id = r.id
LEFT JOIN action_requests ar ON ar.step_id = s.id
WHERE r.tenant_id = '${TENANT_ID}' AND r.recipe_id = 'notify_v1'
ORDER BY r.created_at DESC, s.created_at DESC, ar.created_at DESC;

.print '=== Action result outcome for startup message actions ==='
SELECT
  r.id AS run_id,
  ar.id AS action_request_id,
  ar.action_type,
  ar.status AS action_request_status,
  rr.status AS action_result_status,
  rr.executed_at,
  substr(coalesce(rr.error_json, rr.result_json),1,180) AS result_or_error_preview
FROM runs r
JOIN steps s ON s.run_id = r.id
JOIN action_requests ar ON ar.step_id = s.id
LEFT JOIN action_results rr ON rr.action_request_id = ar.id
WHERE r.tenant_id = '${TENANT_ID}' AND r.recipe_id = 'notify_v1'
ORDER BY rr.executed_at DESC, ar.created_at DESC
LIMIT 50;
SQL

echo
echo "One-liner equivalent (copy/paste):"
echo "sqlite3 '${DB_PATH}' \"SELECT id, status, created_at, triggered_by_user_id, substr(input_json,1,160) AS input_json_preview FROM runs WHERE tenant_id='${TENANT_ID}' AND recipe_id='notify_v1' ORDER BY created_at DESC LIMIT 20; SELECT r.id AS run_id, s.id AS step_id, s.status AS step_status, ar.id AS action_request_id, ar.action_type, ar.status AS action_request_status, json_extract(ar.args_json, '$.destination') AS destination, substr(json_extract(ar.args_json,'$.text'),1,160) AS requested_text FROM runs r JOIN steps s ON s.run_id = r.id LEFT JOIN action_requests ar ON ar.step_id = s.id WHERE r.tenant_id='${TENANT_ID}' AND r.recipe_id='notify_v1' ORDER BY r.created_at DESC, s.created_at DESC, ar.created_at DESC; SELECT r.id AS run_id, ar.id AS action_request_id, ar.action_type, ar.status AS action_request_status, rr.status AS action_result_status, rr.executed_at, substr(coalesce(rr.error_json, rr.result_json),1,180) AS result_or_error_preview FROM runs r JOIN steps s ON s.run_id = r.id JOIN action_requests ar ON ar.step_id = s.id LEFT JOIN action_results rr ON rr.action_request_id = ar.id WHERE r.tenant_id='${TENANT_ID}' AND r.recipe_id='notify_v1' ORDER BY rr.executed_at DESC, ar.created_at DESC LIMIT 50;\""
