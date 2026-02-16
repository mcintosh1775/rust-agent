# Development Guide

This is a living document for contributors building Aegis locally.

## Scope
- Local developer setup
- Build, lint, and test workflows
- Local Postgres workflow for migrations and integration testing

## Prerequisites
- Rust toolchain (stable) with `rustfmt` and `clippy`
- Podman (preferred) with compose support, or Docker with Compose
- `sqlx-cli` for migration commands

Install `sqlx-cli`:

```bash
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

## Repository bootstrap

```bash
git clone <repo-url>
cd rust-agent
```

## Local database (shared service model, local instance)
Aegis uses a shared Postgres service per environment. In local dev, run one local Postgres container and one standardized app schema.

Start/stop DB:

```bash
make container-info
make db-up
make db-down
```

If auto-detection picks the wrong runtime, override it explicitly:

```bash
COMPOSE_CMD="podman compose" make db-up
```

Useful runtime checks:

```bash
make container-info
```
- Shows which compose command the Makefile detected and prints available runtime versions.

```bash
COMPOSE_CMD="podman compose" make db-up
COMPOSE_CMD="podman compose" make db-down
```
- Forces Podman compose regardless of auto-detection.

```bash
podman ps
```
- Confirms the Postgres container is running after `make db-up`.

Default connection:

```bash
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/agentdb
```

## Build and quality commands

```bash
make fmt
make lint
make test
make test-db
make check
```

Run services:

```bash
make api
make worker
```

## Migrations
Run migrations:

```bash
make migrate
```

Prepare sqlx offline metadata (when needed):

```bash
make sqlx-prepare
```

## Integration test notes
- Integration tests should use isolated test schemas per test run.
- Keep DB tests deterministic.
- Always cap loops/timeouts to avoid hanging CI.
- DB integration tests are enabled when `RUN_DB_TESTS=1`.

Run all tests with DB integration enabled:

```bash
RUN_DB_TESTS=1 TEST_DATABASE_URL=$DATABASE_URL cargo test
```

See `docs/TESTING.md` for mandatory test coverage expectations.

## Workflow expectations
- Follow `AGENTS.md` non-negotiables.
- Keep trusted code paths small (`core` policy + primitives + dispatcher).
- Add or update tests in the same change as feature work.
- Update `CHANGELOG.md` for every meaningful repository change.
