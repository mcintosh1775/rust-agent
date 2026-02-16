SHELL := /bin/bash

.PHONY: fmt lint test test-db check api worker db-up db-down migrate sqlx-prepare

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test

test-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb_test} cargo test -p core --test db_integration

check: fmt lint test

api:
	cargo run -p api

worker:
	cargo run -p worker

db-up:
	docker compose up -d

db-down:
	docker compose down

migrate:
	sqlx migrate run

sqlx-prepare:
	cargo sqlx prepare --workspace
