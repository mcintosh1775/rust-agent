#!/usr/bin/env python3
"""Operational drift checks for channel-scoped llm.infer routing."""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

import solo_lite_agent_run as runner


def _sql_literal(value: str) -> str:
    return value.replace("'", "''")


def _parse_json_loose(raw: str, *, label: str) -> dict[str, object]:
    text = raw.strip()
    if not text:
        raise RuntimeError(f"{label} returned empty output")
    try:
        payload = json.loads(text)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{label} returned invalid JSON:\n{text}") from error
    if not isinstance(payload, dict):
        raise RuntimeError(f"{label} expected JSON object")
    return payload


def _exec_worker_python(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    script: str,
    args: list[str],
) -> str:
    compose_file = repo_root / "infra" / "containers" / "compose.yml"
    base_cmd = compose_cmd + [
        "-f",
        str(compose_file),
        "--profile",
        "solo-lite",
        "exec",
        "-T",
        "worker-lite",
        "python3",
        "-c",
        script,
        *args,
    ]
    try:
        completed = runner._run(base_cmd, cwd=repo_root, capture_output=True)
    except subprocess.CalledProcessError as err:
        if "-T" in base_cmd and ("unknown flag" in err.stderr or "unknown shorthand flag" in err.stderr):
            fallback = [part for part in base_cmd if part != "-T"]
            completed = runner._run(fallback, cwd=repo_root, capture_output=True)
        else:
            raise RuntimeError(
                f"worker-lite exec failed:\nstdout:\n{err.stdout}\nstderr:\n{err.stderr}"
            ) from err
    return completed.stdout


def _exec_postgres_psql(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    sql: str,
) -> str:
    compose_file = repo_root / "infra" / "containers" / "compose.yml"
    base_cmd = compose_cmd + [
        "-f",
        str(compose_file),
        "--profile",
        "stack",
        "exec",
        "-T",
        "postgres",
        "psql",
        "-U",
        "postgres",
        "-d",
        "agentdb",
        "-v",
        "ON_ERROR_STOP=1",
        "-At",
        "-F",
        "\t",
        "-c",
        sql,
    ]
    try:
        completed = runner._run(base_cmd, cwd=repo_root, capture_output=True)
    except subprocess.CalledProcessError as err:
        if "-T" in base_cmd and ("unknown flag" in err.stderr or "unknown shorthand flag" in err.stderr):
            fallback = [part for part in base_cmd if part != "-T"]
            completed = runner._run(fallback, cwd=repo_root, capture_output=True)
        else:
            raise RuntimeError(
                f"postgres exec failed:\nstdout:\n{err.stdout}\nstderr:\n{err.stderr}"
            ) from err
    return completed.stdout


def _collect_metrics_sqlite(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    sqlite_path: str,
    tenant_id: str,
    window_secs: int,
) -> dict[str, object]:
    query_script = r"""
import json
import sqlite3
import sys

db_path, tenant_id, window_secs = sys.argv[1:]
conn = sqlite3.connect(db_path)
row = conn.execute(
    '''
    WITH llm_events AS (
      SELECT
        ae.event_type AS event_type,
        json_extract(ae.payload_json, '$.result.gateway.channel') AS gateway_channel,
        json_extract(ae.payload_json, '$.result.gateway.channel_defaults_applied') AS gateway_channel_defaults_applied,
        lower(
          ltrim(
            trim(
              COALESCE(
                NULLIF(CAST(json_extract(r.input_json, '$.llm_channel') AS TEXT), ''),
                NULLIF(CAST(json_extract(r.input_json, '$.channel') AS TEXT), ''),
                NULLIF(CAST(json_extract(r.input_json, '$._trigger.channel') AS TEXT), ''),
                NULLIF(CAST(json_extract(r.input_json, '$.event_payload.channel') AS TEXT), ''),
                ''
              )
            ),
            '#'
          )
        ) AS expected_channel
      FROM audit_events ae
      JOIN runs r ON r.id = ae.run_id
      WHERE r.tenant_id = ?
        AND json_extract(ae.payload_json, '$.action_type') = 'llm.infer'
        AND datetime(ae.created_at) >= datetime('now', '-' || ? || ' seconds')
    )
    SELECT
      SUM(CASE WHEN event_type = 'action.executed' THEN 1 ELSE 0 END) AS executed_count,
      SUM(CASE WHEN event_type = 'action.denied' THEN 1 ELSE 0 END) AS denied_count,
      SUM(CASE WHEN event_type = 'action.executed' AND expected_channel <> '' THEN 1 ELSE 0 END) AS hinted_executed_count,
      SUM(
        CASE
          WHEN event_type = 'action.executed'
            AND expected_channel <> ''
            AND (gateway_channel IS NULL OR trim(CAST(gateway_channel AS TEXT)) = '')
          THEN 1 ELSE 0
        END
      ) AS missing_gateway_channel_count,
      SUM(
        CASE
          WHEN event_type = 'action.executed'
            AND expected_channel <> ''
            AND lower(trim(COALESCE(CAST(gateway_channel AS TEXT), ''))) <> expected_channel
          THEN 1 ELSE 0
        END
      ) AS channel_mismatch_count,
      SUM(
        CASE
          WHEN event_type = 'action.executed'
            AND expected_channel IN ('general', 'inbox', 'monitoring')
            AND gateway_channel_defaults_applied IS NULL
          THEN 1 ELSE 0
        END
      ) AS missing_channel_defaults_flag_count,
      SUM(
        CASE
          WHEN event_type = 'action.executed'
            AND expected_channel IN ('general', 'inbox', 'monitoring')
            AND lower(trim(COALESCE(CAST(gateway_channel_defaults_applied AS TEXT), ''))) NOT IN ('true', '1')
          THEN 1 ELSE 0
        END
      ) AS channel_defaults_false_count
    FROM llm_events
    ''',
    (tenant_id, window_secs),
).fetchone()
conn.close()
if row is None:
    print("{}")
else:
    print(json.dumps({
        "executed_count": int(row[0] or 0),
        "denied_count": int(row[1] or 0),
        "hinted_executed_count": int(row[2] or 0),
        "missing_gateway_channel_count": int(row[3] or 0),
        "channel_mismatch_count": int(row[4] or 0),
        "missing_channel_defaults_flag_count": int(row[5] or 0),
        "channel_defaults_false_count": int(row[6] or 0),
    }))
"""
    raw = _exec_worker_python(
        repo_root=repo_root,
        compose_cmd=compose_cmd,
        script=query_script,
        args=[sqlite_path, tenant_id, str(window_secs)],
    )
    return _parse_json_loose(raw, label="sqlite drift query")


def _collect_metrics_postgres(
    *,
    repo_root: pathlib.Path,
    compose_cmd: list[str],
    tenant_id: str,
    window_secs: int,
) -> dict[str, object]:
    tenant_sql = _sql_literal(tenant_id)
    sql = f"""
WITH llm_events AS (
  SELECT
    ae.event_type,
    ae.payload_json #>> '{{result,gateway,channel}}' AS gateway_channel,
    ae.payload_json #>> '{{result,gateway,channel_defaults_applied}}' AS gateway_channel_defaults_applied,
    lower(trim(leading '#' FROM COALESCE(
      NULLIF(r.input_json->>'llm_channel', ''),
      NULLIF(r.input_json->>'channel', ''),
      NULLIF(r.input_json#>>'{{_trigger,channel}}', ''),
      NULLIF(r.input_json#>>'{{event_payload,channel}}', ''),
      ''
    ))) AS expected_channel
  FROM audit_events ae
  JOIN runs r ON r.id = ae.run_id
  WHERE r.tenant_id = '{tenant_sql}'
    AND ae.payload_json->>'action_type' = 'llm.infer'
    AND ae.created_at >= NOW() - ('{int(window_secs)} seconds')::interval
)
SELECT json_build_object(
  'executed_count', COUNT(*) FILTER (WHERE event_type = 'action.executed'),
  'denied_count', COUNT(*) FILTER (WHERE event_type = 'action.denied'),
  'hinted_executed_count', COUNT(*) FILTER (WHERE event_type = 'action.executed' AND expected_channel <> ''),
  'missing_gateway_channel_count', COUNT(*) FILTER (
    WHERE event_type = 'action.executed'
      AND expected_channel <> ''
      AND COALESCE(trim(gateway_channel), '') = ''
  ),
  'channel_mismatch_count', COUNT(*) FILTER (
    WHERE event_type = 'action.executed'
      AND expected_channel <> ''
      AND lower(COALESCE(trim(gateway_channel), '')) <> expected_channel
  ),
  'missing_channel_defaults_flag_count', COUNT(*) FILTER (
    WHERE event_type = 'action.executed'
      AND expected_channel IN ('general', 'inbox', 'monitoring')
      AND gateway_channel_defaults_applied IS NULL
  ),
  'channel_defaults_false_count', COUNT(*) FILTER (
    WHERE event_type = 'action.executed'
      AND expected_channel IN ('general', 'inbox', 'monitoring')
      AND lower(COALESCE(trim(gateway_channel_defaults_applied), '')) <> 'true'
  )
)::text
FROM llm_events;
""".strip()
    raw = _exec_postgres_psql(repo_root=repo_root, compose_cmd=compose_cmd, sql=sql)
    return _parse_json_loose(raw, label="postgres drift query")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", required=True, choices=["solo-lite", "stack"])
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--window-secs", type=int, default=3600)
    parser.add_argument("--sqlite-path", default=runner.DEFAULT_SQLITE_PATH)
    parser.add_argument("--min-observations", type=int, default=1)
    parser.add_argument("--max-denied-rate-pct", type=float, default=40.0)
    parser.add_argument("--max-channel-mismatch-count", type=int, default=0)
    parser.add_argument("--max-missing-gateway-channel-count", type=int, default=0)
    parser.add_argument("--max-missing-channel-defaults-flag-count", type=int, default=0)
    parser.add_argument("--max-channel-defaults-false-count", type=int, default=0)
    args = parser.parse_args()

    repo_root = runner._repo_root()
    compose_cmd = runner._detect_compose_cmd()

    if args.profile == "solo-lite":
        metrics = _collect_metrics_sqlite(
            repo_root=repo_root,
            compose_cmd=compose_cmd,
            sqlite_path=args.sqlite_path,
            tenant_id=args.tenant_id,
            window_secs=args.window_secs,
        )
    else:
        metrics = _collect_metrics_postgres(
            repo_root=repo_root,
            compose_cmd=compose_cmd,
            tenant_id=args.tenant_id,
            window_secs=args.window_secs,
        )

    executed_count = int(metrics.get("executed_count") or 0)
    denied_count = int(metrics.get("denied_count") or 0)
    total_observed = executed_count + denied_count
    denied_rate_pct = (100.0 * denied_count / total_observed) if total_observed > 0 else 0.0

    failures: list[str] = []
    if total_observed < args.min_observations:
        failures.append(
            f"observed llm.infer count {total_observed} < min_observations {args.min_observations}"
        )
    if denied_rate_pct > args.max_denied_rate_pct:
        failures.append(
            f"denied_rate_pct {denied_rate_pct:.2f} > max_denied_rate_pct {args.max_denied_rate_pct:.2f}"
        )

    for key, max_value in [
        ("channel_mismatch_count", args.max_channel_mismatch_count),
        ("missing_gateway_channel_count", args.max_missing_gateway_channel_count),
        ("missing_channel_defaults_flag_count", args.max_missing_channel_defaults_flag_count),
        ("channel_defaults_false_count", args.max_channel_defaults_false_count),
    ]:
        observed = int(metrics.get(key) or 0)
        if observed > max_value:
            failures.append(f"{key} {observed} > {max_value}")

    output = {
        "profile": args.profile,
        "tenant_id": args.tenant_id,
        "window_secs": args.window_secs,
        "metrics": metrics,
        "total_observed": total_observed,
        "denied_rate_pct": round(denied_rate_pct, 2),
        "failures": failures,
    }
    if failures:
        print("llm channel drift check failed")
        print(json.dumps(output, indent=2, sort_keys=True))
        return 1

    print("llm channel drift check passed")
    print(json.dumps(output, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
