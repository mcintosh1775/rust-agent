# M11A Implementation Plan - Web Operations Console (Baseline)

## Goal
Ship a lightweight, operator-focused web console that runs inside the existing API service (`secureagnt-api`) and exposes a usable first dashboard without introducing a separate frontend deployment pipeline.

## Scope (M11A)
- Add a web console shell route served by API (`GET /console`).
- Use existing API endpoints as the data source (no new control-plane write operations).
- Provide read-only operator visibility for:
  - run/queue health
  - latency and action-path performance
  - token usage/burn
  - payment summary
  - compliance/SIEM delivery SLO status
- Keep deployment model unchanged for both binary and container paths.

## Design Decisions
- Runtime: serve console from API process/binary first.
- Frontend: server-served HTML/CSS/JS (no SPA build system for baseline).
- Data fetch: browser polls existing `/v1/*` endpoints with tenant/role headers.
- Security: console remains read-only and follows existing role policy on backing APIs.

## Delivery Steps
1. Console shell
- Add `GET /console` HTML route in API.
- Include tenant/role/window controls and refresh controls.

2. Data wiring
- Query and render:
  - `/v1/ops/summary`
  - `/v1/ops/latency-histogram`
  - `/v1/ops/action-latency`
  - `/v1/usage/llm/tokens`
  - `/v1/payments/summary`
  - `/v1/audit/compliance/siem/deliveries/slo`
- Implement graceful per-panel error handling.

3. Baseline UX
- Responsive cards/panels for desktop and mobile.
- Auto-refresh with configurable interval.
- Last-refresh indicator and status chips.

4. Validation
- API integration test for console route availability/content type.
- Manual runbook note for opening console in local/container flows.

## Exit Criteria (M11A)
- `GET /console` serves a functional dashboard shell from `secureagnt-api`.
- Dashboard renders live data from existing API endpoints with no backend schema changes required.
- Works in local binary mode and container stack mode.
- Read-only baseline with role-aligned behavior documented.

## Deferred (post-M11A)
- Dedicated frontend service/container.
- SSO/session auth integration and UI-level RBAC enforcement.
- Drill-down pages (per-agent, per-run, per-action).
- Alert management and acknowledgement workflows.
- Historical charts with long-range aggregation APIs.
