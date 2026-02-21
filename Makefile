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

.PHONY: fmt lint build test test-db test-worker-db test-api-db check verify verify-db coverage coverage-db api worker agntctl secureagnt-api secureagntd db-up db-down stack-build stack-up stack-up-build stack-down stack-ps stack-logs stack-lite-build stack-lite-up stack-lite-up-build stack-lite-down stack-lite-ps stack-lite-logs stack-lite-smoke stack-lite-guardrails stack-lite-soak stack-lite-signoff solo-lite-agent solo-lite-chat whitenoise-roundtrip-smoke whitenoise-enterprise-smoke llm-channel-parity-smoke llm-channel-parity-smoke-lite llm-channel-parity-smoke-enterprise llm-channel-drift-check llm-channel-drift-check-lite llm-channel-drift-check-enterprise quickstart-seed agent-context-init solo-lite-init solo-lite-smoke migrate sqlx-prepare container-info soak-gate perf-gate compliance-gate isolation-gate m5c-signoff m6-signoff m6a-signoff m7-signoff m8-signoff m8a-signoff m9-signoff m10-signoff m10-matrix-gate governance-gate capture-perf-baseline security-gate runbook-validate validation-gate release-manifest release-manifest-verify deploy-preflight release-gate

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
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile db up -d postgres

db-down:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile db down

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

stack-lite-build:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	SECUREAGNT_CARGO_BUILD_JOBS=$${SECUREAGNT_CARGO_BUILD_JOBS:-$(CARGO_BUILD_JOBS)} \
		$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile solo-lite build api-lite worker-lite

stack-lite-up:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	LLM_MODE=$${LLM_MODE:-local_first} \
	LLM_MAX_INPUT_BYTES=$${LLM_MAX_INPUT_BYTES:-262144} \
	LLM_MAX_PROMPT_BYTES=$${LLM_MAX_PROMPT_BYTES:-32000} \
	LLM_MAX_OUTPUT_BYTES=$${LLM_MAX_OUTPUT_BYTES:-64000} \
	LLM_LOCAL_BASE_URL=$${LLM_LOCAL_BASE_URL:-} \
	LLM_LOCAL_MODEL=$${LLM_LOCAL_MODEL:-qwen2.5:7b-instruct} \
	LLM_LOCAL_API_KEY=$${LLM_LOCAL_API_KEY:-} \
	LLM_LOCAL_API_KEY_REF=$${LLM_LOCAL_API_KEY_REF:-} \
	LLM_LOCAL_SMALL_BASE_URL=$${LLM_LOCAL_SMALL_BASE_URL:-} \
	LLM_LOCAL_SMALL_MODEL=$${LLM_LOCAL_SMALL_MODEL:-} \
	LLM_LOCAL_SMALL_API_KEY=$${LLM_LOCAL_SMALL_API_KEY:-} \
	LLM_LOCAL_SMALL_API_KEY_REF=$${LLM_LOCAL_SMALL_API_KEY_REF:-} \
	LLM_LOCAL_INTERACTIVE_TIER=$${LLM_LOCAL_INTERACTIVE_TIER:-workhorse} \
	LLM_LOCAL_BATCH_TIER=$${LLM_LOCAL_BATCH_TIER:-workhorse} \
	LLM_CHANNEL_DEFAULTS_JSON=$${LLM_CHANNEL_DEFAULTS_JSON:-} \
	LLM_VERIFIER_ENABLED=$${LLM_VERIFIER_ENABLED:-0} \
	LLM_REMOTE_EGRESS_ENABLED=$${LLM_REMOTE_EGRESS_ENABLED:-0} \
	LLM_REMOTE_EGRESS_CLASS=$${LLM_REMOTE_EGRESS_CLASS:-cloud_allowed} \
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile solo-lite up -d api-lite worker-lite

stack-lite-up-build:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	SECUREAGNT_CARGO_BUILD_JOBS=$${SECUREAGNT_CARGO_BUILD_JOBS:-$(CARGO_BUILD_JOBS)} \
	LLM_MODE=$${LLM_MODE:-local_first} \
	LLM_MAX_INPUT_BYTES=$${LLM_MAX_INPUT_BYTES:-262144} \
	LLM_MAX_PROMPT_BYTES=$${LLM_MAX_PROMPT_BYTES:-32000} \
	LLM_MAX_OUTPUT_BYTES=$${LLM_MAX_OUTPUT_BYTES:-64000} \
	LLM_LOCAL_BASE_URL=$${LLM_LOCAL_BASE_URL:-} \
	LLM_LOCAL_MODEL=$${LLM_LOCAL_MODEL:-qwen2.5:7b-instruct} \
	LLM_LOCAL_API_KEY=$${LLM_LOCAL_API_KEY:-} \
	LLM_LOCAL_API_KEY_REF=$${LLM_LOCAL_API_KEY_REF:-} \
	LLM_LOCAL_SMALL_BASE_URL=$${LLM_LOCAL_SMALL_BASE_URL:-} \
	LLM_LOCAL_SMALL_MODEL=$${LLM_LOCAL_SMALL_MODEL:-} \
	LLM_LOCAL_SMALL_API_KEY=$${LLM_LOCAL_SMALL_API_KEY:-} \
	LLM_LOCAL_SMALL_API_KEY_REF=$${LLM_LOCAL_SMALL_API_KEY_REF:-} \
	LLM_LOCAL_INTERACTIVE_TIER=$${LLM_LOCAL_INTERACTIVE_TIER:-workhorse} \
	LLM_LOCAL_BATCH_TIER=$${LLM_LOCAL_BATCH_TIER:-workhorse} \
	LLM_CHANNEL_DEFAULTS_JSON=$${LLM_CHANNEL_DEFAULTS_JSON:-} \
	LLM_VERIFIER_ENABLED=$${LLM_VERIFIER_ENABLED:-0} \
	LLM_REMOTE_EGRESS_ENABLED=$${LLM_REMOTE_EGRESS_ENABLED:-0} \
	LLM_REMOTE_EGRESS_CLASS=$${LLM_REMOTE_EGRESS_CLASS:-cloud_allowed} \
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile solo-lite up -d --build api-lite worker-lite

stack-lite-down:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile solo-lite down

stack-lite-ps:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile solo-lite ps

stack-lite-logs:
	@if [ -z "$(COMPOSE_CMD)" ]; then \
		echo "No compose runtime found. Install Podman (with compose) or Docker."; \
		exit 1; \
	fi
	@if [ ! -f "$(COMPOSE_FILE_ABS)" ]; then \
		echo "Compose file not found: $(COMPOSE_FILE_ABS)"; \
		exit 1; \
	fi
	$(COMPOSE_CMD) -f "$(COMPOSE_FILE_ABS)" --profile solo-lite logs -f

stack-lite-smoke:
	python3 scripts/ops/stack_lite_smoke.py

stack-lite-guardrails:
	python3 scripts/ops/stack_lite_guardrails.py

stack-lite-soak:
	python3 scripts/ops/stack_lite_soak.py \
		--iterations $${STACK_LITE_SOAK_ITERATIONS:-10} \
		--interval-secs $${STACK_LITE_SOAK_INTERVAL_SECS:-2} \
		--timeout-secs $${STACK_LITE_SOAK_TIMEOUT_SECS:-10} \
		--user-roles "$${STACK_LITE_SOAK_ROLES:-owner,operator}" \
		$${STACK_LITE_SOAK_FAIL_FAST:+--fail-fast}

stack-lite-signoff:
	python3 scripts/ops/stack_lite_smoke.py --user-role owner
	python3 scripts/ops/stack_lite_smoke.py --user-role operator
	python3 scripts/ops/stack_lite_guardrails.py
	python3 scripts/ops/stack_lite_soak.py \
		--iterations $${STACK_LITE_SIGNOFF_ITERATIONS:-20} \
		--interval-secs $${STACK_LITE_SIGNOFF_INTERVAL_SECS:-2} \
		--timeout-secs $${STACK_LITE_SIGNOFF_TIMEOUT_SECS:-10} \
		--user-roles "$${STACK_LITE_SIGNOFF_ROLES:-owner,operator}" \
		--fail-fast

solo-lite-agent:
	python3 scripts/ops/solo_lite_agent_run.py

solo-lite-chat:
	python3 scripts/ops/solo_lite_chat.py

whitenoise-roundtrip-smoke:
	python3 scripts/ops/whitenoise_roundtrip_smoke.py $${WHITENOISE_SMOKE_ARGS:-}

whitenoise-enterprise-smoke:
	python3 scripts/ops/whitenoise_enterprise_smoke.py $${WHITENOISE_ENTERPRISE_SMOKE_ARGS:-}

llm-channel-parity-smoke:
	python3 scripts/ops/llm_channel_parity_smoke.py $${LLM_CHANNEL_PARITY_SMOKE_ARGS:-}

llm-channel-parity-smoke-lite:
	python3 scripts/ops/llm_channel_parity_smoke.py --profile solo-lite $${LLM_CHANNEL_PARITY_SMOKE_LITE_ARGS:-}

llm-channel-parity-smoke-enterprise:
	python3 scripts/ops/llm_channel_parity_smoke.py --profile stack $${LLM_CHANNEL_PARITY_SMOKE_ENTERPRISE_ARGS:-}

llm-channel-drift-check:
	python3 scripts/ops/llm_channel_drift_check.py --profile solo-lite $${LLM_CHANNEL_DRIFT_CHECK_LITE_ARGS:-}
	python3 scripts/ops/llm_channel_drift_check.py --profile stack $${LLM_CHANNEL_DRIFT_CHECK_ENTERPRISE_ARGS:-}

llm-channel-drift-check-lite:
	python3 scripts/ops/llm_channel_drift_check.py --profile solo-lite $${LLM_CHANNEL_DRIFT_CHECK_LITE_ARGS:-}

llm-channel-drift-check-enterprise:
	python3 scripts/ops/llm_channel_drift_check.py --profile stack $${LLM_CHANNEL_DRIFT_CHECK_ENTERPRISE_ARGS:-}

quickstart-seed:
	bash scripts/ops/quickstart_seed.sh

agent-context-init:
	bash scripts/ops/init_agent_context.sh

solo-lite-init:
	python3 scripts/ops/solo_lite_init.py

solo-lite-smoke:
	python3 scripts/ops/solo_lite_smoke.py

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

m6a-signoff:
	bash scripts/ops/m6a_signoff.sh

m7-signoff:
	bash scripts/ops/m7_signoff.sh

m8-signoff:
	bash scripts/ops/m8_signoff.sh

m8a-signoff:
	bash scripts/ops/m8a_signoff.sh

m9-signoff:
	bash scripts/ops/m9_signoff.sh

m10-signoff:
	bash scripts/ops/m10_signoff.sh

m10-matrix-gate:
	bash scripts/ops/m10_matrix_gate.sh

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
