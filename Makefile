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

.PHONY: fmt lint build test test-db test-worker-db test-api-db verify-workspace-versions test-skills test-release-startup-smoke test-release-distribution-check test-release-llm-smoke check verify verify-db coverage coverage-db api worker agntctl secureagnt-api secureagntd db-up db-down stack-build stack-up stack-up-build stack-down stack-ps stack-logs stack-lite-build stack-lite-up stack-lite-up-build stack-lite-down stack-lite-ps stack-lite-logs stack-lite-smoke stack-lite-guardrails stack-lite-soak stack-lite-signoff slack-events-bridge slack-events-bridge-service-install slack-events-bridge-service-start slack-events-bridge-service-stop slack-events-bridge-service-restart slack-events-bridge-service-status slack-events-bridge-service-logs sync-solo-lite-skills solo-lite-agent solo-lite-chat solo-lite-command-smoke solo-lite-command-smoke-inbound solo-lite-command-smoke-inbound-slack solo-lite-command-smoke-inbound-live whitenoise-roundtrip-smoke whitenoise-enterprise-smoke llm-channel-parity-smoke llm-channel-parity-smoke-lite llm-channel-parity-smoke-enterprise llm-channel-drift-check llm-channel-drift-check-lite llm-channel-drift-check-enterprise quickstart-seed agent-context-init solo-lite-init solo-lite-smoke migrate sqlx-prepare container-info soak-gate perf-gate compliance-gate isolation-gate m5c-signoff m6-signoff m6a-signoff m7-signoff m8-signoff m8a-signoff m9-signoff m10-signoff m10-matrix-gate governance-gate capture-perf-baseline security-gate security-gate-with-audit runbook-validate validation-gate release-startup-smoke release-llm-smoke release-smoke-check release-distribution-check release-package release-manifest release-manifest-verify deploy-preflight release-gate release-upload cargo-audit handoff

SOLO_LITE_SKILL_REPO_PATH ?= skills/python/summarize_transcript/main.py
SOLO_LITE_DEPLOY_SOURCE_ROOT ?= /opt/secureagnt/source
SOLO_LITE_DEPLOY_ARTIFACT_ROOT ?= /opt/secureagnt/artifacts
SLACK_EVENTS_BRIDGE_SERVICE_NAME ?= secureagnt-slack-events-bridge.service
SLACK_EVENTS_BRIDGE_SERVICE_FILE ?= /etc/systemd/system/$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)
SLACK_EVENTS_BRIDGE_SERVICE_SOURCE ?= infra/systemd/$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)
SLACK_EVENTS_BRIDGE_SCRIPT_SOURCE ?= scripts/ops/slack_events_bridge.py
SLACK_EVENTS_BRIDGE_SCRIPT_BIN ?= /usr/local/bin/slack_events_bridge.py
SLACK_EVENTS_BRIDGE_ENV_FILE ?= /etc/secureagnt/slack-events-bridge.env
SLACK_EVENTS_BRIDGE_ARGS_STATE_FILE ?= /var/lib/secureagnt/slack-events-bridge.json


fmt:
	cargo fmt

lint:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo clippy --all-targets --all-features -- -D warnings

build:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo build --workspace

test:
	CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test

test-skills:
	python3 skills/python/test_all_python_skills.py

test-release-startup-smoke:
	python3 scripts/ops/test_release_startup_smoke.py

test-release-distribution-check:
	python3 scripts/ops/test_release_distribution_check.py

test-release-llm-smoke:
	python3 scripts/ops/test_release_llm_smoke.py

test-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test -p core --test db_integration

test-worker-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test -p worker --test worker_integration

test-api-db:
	RUN_DB_TESTS=1 TEST_DATABASE_URL=$${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb} CARGO_BUILD_JOBS=$(CARGO_BUILD_JOBS) cargo test -p api --test api_integration

check: verify-workspace-versions fmt lint test

verify: verify-workspace-versions build test

verify-workspace-versions:
	@bash scripts/ops/verify_workspace_versions.sh

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
	WORKER_ARTIFACT_ROOT=$${WORKER_ARTIFACT_ROOT:-/var/lib/secureagnt/artifacts} \
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
	WORKER_ARTIFACT_ROOT=$${WORKER_ARTIFACT_ROOT:-/var/lib/secureagnt/artifacts} \
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
	WORKER_ARTIFACT_ROOT=$${WORKER_ARTIFACT_ROOT:-/var/lib/secureagnt/artifacts} \
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

solo-lite-command-smoke:
	bash -lc 'eval python3 scripts/ops/solo_lite_command_smoke.py $$SOLO_LITE_COMMAND_SMOKE_ARGS'

solo-lite-command-smoke-inbound:
	bash -lc 'eval python3 scripts/ops/solo_lite_command_smoke.py --inbound-smoke $$SOLO_LITE_COMMAND_SMOKE_ARGS'

solo-lite-command-smoke-inbound-slack:
	bash -lc 'eval python3 scripts/ops/solo_lite_command_smoke.py --inbound-smoke --inbound-provider slack $$SOLO_LITE_COMMAND_SMOKE_ARGS'

solo-lite-command-smoke-inbound-live:
	bash -lc 'eval python3 scripts/ops/solo_lite_command_smoke.py --inbound-smoke --inbound-live $$SOLO_LITE_COMMAND_SMOKE_ARGS'

slack-events-bridge: sync-solo-lite-skills
	bash -lc 'eval python3 scripts/ops/slack_events_bridge.py $$SLACK_EVENTS_BRIDGE_ARGS'

slack-events-bridge-service-install:
	@if [ -z "$${SLACK_EVENTS_BRIDGE_ARGS:-}" ] && [ ! -f "$(SLACK_EVENTS_BRIDGE_ENV_FILE)" ]; then \
		echo "SLACK_EVENTS_BRIDGE_ARGS is required (or precreate $(SLACK_EVENTS_BRIDGE_ENV_FILE) with SLACK_EVENTS_BRIDGE_ARGS=...)"; \
		echo "Example:"; \
		echo "  SLACK_EVENTS_BRIDGE_ARGS=\"--base-url http://127.0.0.1:8080 --agent-id <uuid> --triggered-by-user-id <uuid> --recipe-id operator_chat_v1 --state-file $(SLACK_EVENTS_BRIDGE_ARGS_STATE_FILE) --allowed-channels C0AGRN3B895 --host 0.0.0.0 --port 9000\""; \
		exit 1; \
	fi
	sudo install -D -m 0755 "$(SLACK_EVENTS_BRIDGE_SCRIPT_SOURCE)" "$(SLACK_EVENTS_BRIDGE_SCRIPT_BIN)"
	sudo install -D -m 0644 "$(SLACK_EVENTS_BRIDGE_SERVICE_SOURCE)" "$(SLACK_EVENTS_BRIDGE_SERVICE_FILE)"
	sudo mkdir -p "$(dir $(SLACK_EVENTS_BRIDGE_ENV_FILE))"
	sudo python3 scripts/ops/update_env_var.py \
		"$(SLACK_EVENTS_BRIDGE_ENV_FILE)" \
		"SLACK_EVENTS_BRIDGE_ARGS" \
		"$${SLACK_EVENTS_BRIDGE_ARGS}"
	sudo chmod 0600 "$(SLACK_EVENTS_BRIDGE_ENV_FILE)"
	sudo systemctl daemon-reload
	sudo systemctl enable "$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)"
	@echo "[bridge-service] installed at $(SLACK_EVENTS_BRIDGE_SERVICE_FILE)"
	@echo "[bridge-service] env file: $(SLACK_EVENTS_BRIDGE_ENV_FILE)"
	@echo "run: sudo make slack-events-bridge-service-start"

slack-events-bridge-service-start:
	sudo systemctl start "$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)"

slack-events-bridge-service-stop:
	sudo systemctl stop "$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)"

slack-events-bridge-service-restart:
	sudo systemctl restart "$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)"

slack-events-bridge-service-status:
	sudo systemctl status "$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)"

slack-events-bridge-service-logs:
	sudo journalctl -u "$(SLACK_EVENTS_BRIDGE_SERVICE_NAME)" -f

sync-solo-lite-skills:
	@test -f "$(SOLO_LITE_SKILL_REPO_PATH)" || { \
		echo "missing skill file: $(SOLO_LITE_SKILL_REPO_PATH)"; \
		exit 1; \
	}
	sudo install -D -m 0644 "$(SOLO_LITE_SKILL_REPO_PATH)" "$(SOLO_LITE_DEPLOY_SOURCE_ROOT)/$(SOLO_LITE_SKILL_REPO_PATH)"
	sudo install -D -m 0644 "$(SOLO_LITE_SKILL_REPO_PATH)" "$(SOLO_LITE_DEPLOY_ARTIFACT_ROOT)/$(SOLO_LITE_SKILL_REPO_PATH)"
	@echo "synced $(SOLO_LITE_SKILL_REPO_PATH) to:"
	@echo "  $(SOLO_LITE_DEPLOY_SOURCE_ROOT)/$(SOLO_LITE_SKILL_REPO_PATH)"
	@echo "  $(SOLO_LITE_DEPLOY_ARTIFACT_ROOT)/$(SOLO_LITE_SKILL_REPO_PATH)"

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

security-gate-with-audit:
	$(MAKE) cargo-audit
	$(MAKE) security-gate

cargo-audit:
	@if ! command -v cargo-audit >/dev/null 2>&1; then \
		echo "cargo-audit not installed. Install with: cargo install cargo-audit --version 0.21.1"; \
		exit 1; \
	fi
	@if [ "${CARGO_AUDIT_REQUIRE_NETWORK:-0}" = "1" ]; then \
		echo "[cargo-audit] require_network=1: validating crates.io index reachability"; \
		curl -fsS --max-time 10 https://index.crates.io/config.json >/dev/null; \
	else \
		if ! curl -fsS --max-time 10 https://index.crates.io/config.json >/dev/null 2>&1; then \
			echo "[cargo-audit] skipped: crates.io index unreachable in this environment."; \
			echo "Set CARGO_AUDIT_REQUIRE_NETWORK=1 to force a hard failure."; \
			exit 0; \
		fi; \
	fi
	cargo audit

runbook-validate:
	bash scripts/ops/validate_runbook.sh

validation-gate:
	bash scripts/ops/validation_gate.sh

release-manifest:
	bash scripts/ops/generate_release_manifest.sh

release-manifest-verify:
	bash scripts/ops/verify_release_manifest.sh

release-package:
	@if [ -z "$${TAG}" ]; then \
		echo "TAG is required, for example: make release-package TAG=v0.3.2"; \
		exit 1; \
	fi
	bash scripts/ops/package_release_assets.sh "$${TAG}"
	bash scripts/ops/package_release_deb.sh "$${TAG}"

handoff:
	@bash scripts/ops/record_handoff.sh

release-startup-smoke:
	@if [ -z "$${RELEASE_SMOKE_DB_PATH:-}" ]; then \
		echo "RELEASE_SMOKE_DB_PATH is required (for example /opt/secureagnt/secureagnt.sqlite3)"; \
		exit 1; \
	fi
	python3 scripts/ops/release_startup_smoke.py \
		--db-path "$${RELEASE_SMOKE_DB_PATH}" \
		--tenant-id "$${RELEASE_SMOKE_TENANT_ID:-single}" \
		$${RELEASE_SMOKE_EXPECTED_TAG:+--expect-tag "$${RELEASE_SMOKE_EXPECTED_TAG}"}

release-smoke-check:
	@if [ -z "$${DB:-${RELEASE_SMOKE_DB_PATH:-}}" ]; then \
		echo "DB (or RELEASE_SMOKE_DB_PATH) is required. Example: DB=/opt/secureagnt/secureagnt.sqlite3"; \
		exit 1; \
	fi
	@if [ -n "$${TAG}" ]; then \
		RELEASE_SMOKE_DB_PATH="$${DB:-$${RELEASE_SMOKE_DB_PATH}}" \
		RELEASE_SMOKE_TENANT_ID="$${TENANT_ID:-single}" \
		RELEASE_SMOKE_EXPECTED_TAG="$${TAG}" \
		make release-startup-smoke; \
	else \
		RELEASE_SMOKE_DB_PATH="$${DB:-$${RELEASE_SMOKE_DB_PATH}}" \
		RELEASE_SMOKE_TENANT_ID="$${TENANT_ID:-single}" \
		make release-startup-smoke; \
	fi

release-llm-smoke:
	@if [ -z "$${DB:-${RELEASE_SMOKE_DB_PATH:-}}" ]; then \
		echo "DB (or RELEASE_SMOKE_DB_PATH) is required. Example: DB=/opt/secureagnt/secureagnt.sqlite3"; \
		exit 1; \
	fi
	python3 scripts/ops/release_llm_smoke.py \
		--db-path "$${DB:-$${RELEASE_SMOKE_DB_PATH}}" \
		--tenant-id "$${RELEASE_SMOKE_TENANT_ID:-single}" \
		$${RELEASE_LLM_SMOKE_RECIPE_ID:+--recipe-id "$${RELEASE_LLM_SMOKE_RECIPE_ID}"} \
		$${RELEASE_LLM_SMOKE_EXPECTED_ROUTE:+--expected-route "$${RELEASE_LLM_SMOKE_EXPECTED_ROUTE}"} \
		$${RELEASE_LLM_SMOKE_EXPECTED_MODEL:+--expected-model "$${RELEASE_LLM_SMOKE_EXPECTED_MODEL}"} \
		$${RELEASE_LLM_SMOKE_EXPECTED_HOST:+--expected-host "$${RELEASE_LLM_SMOKE_EXPECTED_HOST}"}

release-distribution-check:
	@if [ -z "$${TAG}" ]; then \
		echo "TAG is required, for example: make release-distribution-check TAG=v0.2.29"; \
		exit 1; \
	fi
	bash scripts/ops/release_distribution_check.sh \
		"$${TAG}" \
		"$${PLATFORM_TAG:-linux-x86_64}" \
		"$${RELEASE_DIR:-dist/local-release/$${TAG}}" \
		"$${RELEASE_WORKFLOW_FILE:-.github/workflows/release.yml}"

deploy-preflight:
	bash scripts/ops/deploy_preflight.sh

release-gate:
	bash scripts/ops/release_gate.sh

release-upload:
	@if [ -z "$(TAG)" ]; then \
		echo "TAG is required, for example: make release-upload TAG=v0.1.98"; \
		exit 1; \
	fi
	bash scripts/ops/upload_release_assets.sh "$(TAG)" "${RELEASE_DIR:-dist/local-release/$(TAG)}" "$(REPO_NAME)"
