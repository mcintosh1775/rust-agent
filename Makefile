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

COVERAGE_MIN_LINES ?= 70
CARGO_BUILD_JOBS ?= 2

.PHONY: fmt lint build test test-db test-worker-db test-api-db check verify verify-db coverage coverage-db api worker agntctl secureagnt-api secureagntd db-up db-down stack-build stack-up stack-up-build stack-down stack-ps stack-logs quickstart-seed migrate sqlx-prepare container-info soak-gate perf-gate compliance-gate isolation-gate m5c-signoff m6-signoff m7-signoff m8a-signoff governance-gate capture-perf-baseline security-gate runbook-validate validation-gate release-manifest release-manifest-verify deploy-preflight release-gate

fmt:
	cargo fmt

lint:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo clippy --all-targets --all-features -- -D warnings

build:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo build --workspace

test:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test

test-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test -p core --test db_integration

test-worker-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test -p worker --test worker_integration

test-api-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test -p api --test api_integration

check: fmt lint test

verify: build test

verify-db: build test-db test-api-db test-worker-db

coverage:
	@cargo llvm-cov --version >/dev/null 2>&1 || { \
		echo "cargo-llvm-cov is required. Install with: cargo install cargo-llvm-cov"; \
		exit 1; \
	}
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines $(COVERAGE_MIN_LINES)

coverage-db:
	@cargo llvm-cov --version >/dev/null 2>&1 || { \
		echo "cargo-llvm-cov is required. Install with: cargo install cargo-llvm-cov"; \
		exit 1; \
	}
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb_test} \
		CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines $(COVERAGE_MIN_LINES)

api:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo run -p api

worker:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo run -p worker

agntctl:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo run -p agntctl --

secureagnt-api:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo run -p api --bin secureagnt-api

secureagntd:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo run -p worker --bin secureagntd

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

stack-build:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	SECUREAGNT_CARGO_BUILD_JOBS=$${SECUREAGNT_CARGO_BUILD_JOBS:-$(CARGO_BUILD_JOBS)} \
		$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile stack build

stack-up:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile stack up -d

stack-up-build:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	SECUREAGNT_CARGO_BUILD_JOBS=$${SECUREAGNT_CARGO_BUILD_JOBS:-$(CARGO_BUILD_JOBS)} \
		$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile stack up -d --build

stack-down:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile stack down

stack-ps:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile stack ps

stack-logs:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile stack logs -f api worker postgres

quickstart-seed:
	bash scripts/ops/quickstart_seed.sh

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

soak-gate:
	bash scripts/ops/soak_gate.sh

perf-gate:
	bash scripts/ops/perf_gate.sh

compliance-gate:
	bash scripts/ops/compliance_gate.sh

isolation-gate:
	bash scripts/ops/isolation_gate.sh

m5c-signoff:
	bash scripts/ops/m5c_signoff.sh

m6-signoff:
	bash scripts/ops/m6_signoff.sh

m7-signoff:
	bash scripts/ops/m7_signoff.sh

m8a-signoff:
	bash scripts/ops/m8a_signoff.sh

governance-gate:
	bash scripts/ops/governance_gate.sh

capture-perf-baseline:
	bash scripts/ops/capture_perf_baseline.sh

security-gate:
	bash scripts/ops/security_gate.sh

runbook-validate:
	bash scripts/ops/validate_runbook.sh

validation-gate:
	bash scripts/ops/validation_gate.sh

release-manifest:
	bash scripts/ops/generate_release_manifest.sh

release-manifest-verify:
	bash scripts/ops/verify_release_manifest.sh

deploy-preflight:
	bash scripts/ops/deploy_preflight.sh

release-gate:
	bash scripts/ops/release_gate.sh
