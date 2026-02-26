# SQLite Migration Baseline (M15)

These migrations define the solo-lite SQLite schema profile tracked under roadmap milestone
`M15`.

Usage:
- initialize a SQLite DB with `make solo-lite-init`
- run parity smoke checks with `make solo-lite-smoke`

Notes:
- this directory is intentionally separate from `migrations/` because Postgres-only objects
  (plpgsql functions, advisory locks, and interval arithmetic) are not portable to SQLite.
- this profile uses a single baseline schema file (`0001_init.sql`) for the test-harness-first
  install flow.
- compatibility shim migrations (`0019...` and `0020...`) are retained so legacy sqlite installs
  with those migrations already recorded in history can still run against the current baseline.

