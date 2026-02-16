SHELL := /bin/bash

.PHONY: fmt lint test check api worker db-up db-down migrate sqlx-prepare

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test

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
