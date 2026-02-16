SHELL := /bin/bash

COMPOSE_CMD ?= $(shell \
	if command -v podman >/dev/null 2>&1; then \
		echo "podman compose"; \
	elif command -v podman-compose >/dev/null 2>&1; then \
		echo "podman-compose"; \
	elif command -v docker >/dev/null 2>&1; then \
		echo "docker compose"; \
	else \
		echo ""; \
	fi)
COMPOSE_FILE ?= infra/containers/compose.yml
COMPOSE_FILE_ABS := $(abspath $(COMPOSE_FILE))

.PHONY: fmt lint test test-db test-worker-db check api worker db-up db-down migrate sqlx-prepare container-info

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test

test-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} cargo test -p core --test db_integration

test-worker-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} cargo test -p worker --test worker_integration

check: fmt lint test

api:
	cargo run -p api

worker:
	cargo run -p worker

db-up:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" up -d

db-down:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" down

migrate:
	sqlx migrate run

sqlx-prepare:
	cargo sqlx prepare --workspace

container-info:
	@echo "Detected compose command: $(if $(COMPOSE_CMD),$(COMPOSE_CMD),<none>)"
	@echo "Compose file: $(COMPOSE_FILE)"
	@echo "Compose file (absolute): $(COMPOSE_FILE_ABS)"
	@echo "Compose file exists: $(if $(wildcard $(COMPOSE_FILE_ABS)),yes,no)"
	@command -v podman >/dev/null 2>&1 && podman --version || true
	@command -v docker >/dev/null 2>&1 && docker --version || true
	@if [ -n "$(COMPOSE_CMD)" ]; then $(COMPOSE_CMD) version || true; fi
