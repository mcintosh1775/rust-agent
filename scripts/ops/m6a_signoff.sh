#!/usr/bin/env bash
set -euo pipefail

export RUN_DB_TESTS=1
export TEST_DATABASE_URL="${TEST_DATABASE_URL:-postgres://postgres:postgres@localhost:5432/agentdb}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export MEMORY_RETRIEVAL_BENCH_MAX_MS="${MEMORY_RETRIEVAL_BENCH_MAX_MS:-15000}"

run() {
  echo "[m6a-signoff] $*"
  "$@"
}

# Core DB memory retention/isolation/benchmark coverage
run cargo test -p core --test db_integration memory_records_persist_and_query_tenant_scoped -- --nocapture
run cargo test -p core --test db_integration handoff_memory_listing_filters_by_to_and_from_agent -- --nocapture
run cargo test -p core --test db_integration memory_purge_and_compaction_records_work -- --nocapture
run cargo test -p core --test db_integration memory_retrieval_under_concurrent_load_is_tenant_isolated_and_bounded -- --nocapture
run cargo test -p core --test db_integration memory_compaction_under_load_compacts_groups_and_exposes_stats -- --nocapture

# API memory-plane role/retention/redaction paths
run cargo test -p api --test api_integration memory_records_create_list_and_purge_flow -- --nocapture
run cargo test -p api --test api_integration memory_records_auto_redact_sensitive_content_before_persist -- --nocapture
run cargo test -p api --test api_integration memory_retrieve_enforces_scope_role_and_tenant_isolation -- --nocapture
run cargo test -p api --test api_integration handoff_packets_enforce_role_and_tenant_guardrails -- --nocapture
run cargo test -p api --test api_integration memory_compaction_stats_returns_counts_and_enforces_role -- --nocapture

echo "[m6a-signoff] pass"
