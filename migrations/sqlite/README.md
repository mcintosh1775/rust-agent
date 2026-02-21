# SQLite Migration Baseline (M15)

These migrations define the solo-lite SQLite schema profile tracked under roadmap milestone `M15`.

Usage:
- initialize a SQLite DB with `make solo-lite-init`
- run parity smoke checks with `make solo-lite-smoke`

Notes:
- this directory is intentionally separate from `migrations/` because Postgres-only objects
  (plpgsql functions, advisory locks, and interval arithmetic) are not portable to SQLite.
- the current scope is schema parity scaffolding for M15A/M15B while runtime query parity work is in progress.
