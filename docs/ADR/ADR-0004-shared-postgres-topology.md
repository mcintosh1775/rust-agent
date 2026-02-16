# ADR-0004: Shared Postgres topology with API-mediated access

## Status
Accepted

## Context
The platform will run multiple users and multiple agents. A per-agent Postgres instance increases operational cost, weakens consistency, and complicates governance/auditing.

The security model requires a clear trust boundary where policy and capability checks happen in platform services before side effects.

## Decision
Use one shared Postgres cluster per environment (`dev`, `staging`, `prod`) with a standardized platform schema for state tables (`runs`, `steps`, `artifacts`, `action_requests`, `action_results`, `audit_events`).

Data is partitioned logically with identifiers such as `tenant_id`, `agent_id`, and `user_id` rather than database-per-agent.

Only platform services (`api`, `worker`) connect to Postgres. Agents and skills do not connect directly; they interact through platform APIs/protocols.

## Consequences
- Simpler operations and better resource utilization at enterprise scale.
- Stronger governance/audit consistency across agents and users.
- Clear enforcement boundary for capabilities and policy.
- Requires careful indexing, connection pooling, and role-based DB access controls as scale grows.
