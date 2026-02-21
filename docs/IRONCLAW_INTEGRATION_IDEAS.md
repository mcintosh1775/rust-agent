# Ironclaw-Inspired Integration Ideas for SecureAgnt

## Source summary
This list captures high-signal ideas from `nearai/ironclaw` that are likely worth integrating into this repository's current architecture.

## Priority ideas

### 1) Strengthen skill/action contract versioning
- **Why**: Prevents runtime ambiguity as skill action schemas evolve.
- **What to implement**:
  - Add explicit contract versioning for skill `action_requests` (and optional capability bundles).
  - Validate skill outputs against registered/expected schema before policy execution.
  - Reject unknown/invalid contract versions as explicit `INVALID_INPUT`-style deny conditions.
- **SecureAgnt fit**:
  - Extend protocol/types in `core` and policy gate to include schema contract fields.
  - Add deny/audit reason codes for schema drift.

### 2) Bounded context and deterministic compaction strategy
- **Why**: Prevents LLM context blowup and unstable latency under long-running agent threads.
- **What to implement**:
  - Introduce explicit compaction policy for run context and memory payloads.
  - Make compaction deterministic (window size, retention policy, summarization policy).
  - Add a test that validates predictable retained context under repeated append conditions.
- **SecureAgnt fit**:
  - Extend `agent_context` and memory retrieval planning with explicit budget limits and deterministic ordering.

### 3) Idempotency and replay-safe dedupe beyond direct IDs
- **Why**: Real systems receive retry storms/reordered events; plain IDs can still duplicate work.
- **What to implement**:
  - Add semantic dedupe keys (canonicalized payload hash / signature tuple) for run creation and trigger-event ingestion.
  - Ensure duplicate replays preserve idempotency even when event IDs differ.
- **SecureAgnt fit**:
  - Strengthen trigger/event dedupe and side-effect guardrails in API/worker pathways.

### 4) Normalize and harden action arg schema before policy checks
- **Why**: Inconsistent schema shape can bypass intended restrictions or create policy uncertainty.
- **What to implement**:
  - Canonicalize action args (ordering, defaults, aliases) before capability matching.
  - Apply strict deny-closed validation errors early in worker.
- **SecureAgnt fit**:
  - Add canonicalization stage in worker policy evaluation for all `action_requests`.

### 5) End-to-end traceability across run lifecycle
- **Why**: Correlating API ↔ worker ↔ skill failures is hard without common trace IDs.
- **What to implement**:
  - Add trace/context correlation IDs propagated through API request creation, worker claim, skill invocation, and primitive execution.
  - Emit IDs in audit/compliance payloads for cross-plane debugging.
- **SecureAgnt fit**:
  - Expand audit event payload schema with stable trace metadata.

### 6) Scheduling fairness and throughput control as first-class policy
- **Why**: Throughput spikes can starve critical interactive work and worsen tail latency.
- **What to implement**:
  - Tuneable scheduler fairness controls (class promotion/backoff/aging), plus batch pacing.
  - Add metrics-backed control defaults and alerts.
- **SecureAgnt fit**:
  - Fold into existing queue-class and scheduler knobs with stricter defaults + guardrails.

### 7) Hardened skill runtime profiles by default
- **Why**: Isolation should be explicit, minimal, and opt-in for dangerous capabilities.
- **What to implement**:
  - Profile-driven execution defaults for skill subprocess resources, filesystem visibility, and environment allowlists.
  - “Safe mode” profile that is default for untrusted/standard runs.
- **SecureAgnt fit**:
  - Extend worker skill runner with stronger named profiles and explicit kill-switch controls.

### 8) Consistent error taxonomy and deny reasons
- **Why**: Actionability in production depends on stable machine-readable failure signals.
- **What to implement**:
  - Unified failure code set for contract errors, policy denials, execution errors, schema mismatch, and timeouts.
  - Add audit assertions on deterministic reason strings/enums.
- **SecureAgnt fit**:
  - Tighten API/worker/test coverage for stable deny reason outputs.

## Suggested next steps
1. Convert these into backlog tickets with owner + owner area (API / worker / core / docs).
2. Prioritize as: 1, 4, 5, 3, 6, 2, 8, 7.
3. Add one migration/test per item to preserve current “security by default” posture while shipping incrementally.
