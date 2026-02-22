# Top-20 Skills Research (Public Agent Ecosystem, 2026-02-22)

I collected baseline signal from OpenAI Agents SDK tool categories, Anthropic MCP docs, Google ADK docs, and OpenClaw docs/skill directory patterns to define a practical “top-20” agent-skill starter set.

## Sources reviewed

- OpenAI Agents SDK (tool categories): hosted tools, local built-ins, function tools, MCP servers, and agents as tools
  - https://openai.github.io/openai-agents-js/guides/tools/
- OpenClaw skills model + clawhub discovery and local execution precedence
  - https://docs.openclaw.ai/tools/skills
  - https://docs.openclaw.ai/clawhub
  - https://openclawskills.co/
- Anthropic MCP connector + remote MCP server guidance
  - https://docs.anthropic.com/en/docs/agents-and-tools/mcp-connector
  - https://docs.anthropic.com/en/docs/agents-and-tools/remote-mcp-servers
- MCP registry model and moderation model
  - https://modelcontextprotocol.io/registry/about
  - https://modelcontextprotocol.io/registry/moderation-policy
- Google ADK tool ecosystem and integration catalog
  - https://google.github.io/adk-docs/tools/google-cloud/application-integration/
  - https://google.github.io/adk-docs/

## Top-20 skill capabilities to build (2026 baseline)

1. Summarization/abstractive writeup
2. Action-item extraction
3. Draft reply composition
4. Translation/localization scaffolding
5. Sentiment / urgency triage
6. Incident triage + priority recommendation
7. Meeting-minute + follow-up capture
8. Change summary generation
9. Release note drafting
10. Ticket/issue packaging
11. Compliance / secret and PII scan
12. Knowledge extraction / snapshotting
13. Memory checkpoint generation
14. Runbook generation
15. On-call / ops handoff brief
16. Observability interpretation
17. PII scrub/reporting
18. Rewrite / style normalization
19. Follow-up plan generation
20. Payment action planning (provider-aware)

## SecureAgnt mapping after this pass

| Skill | In-pack | Notes |
| --- | --- | --- |
| Structured data query | ✅ | Added as `structured_data_query` |
| Web research planning | ✅ | Added as `web_research_draft` (compute-only planning, no direct outbound I/O) |
| Calendar/event planning | ✅ | Added as `calendar_event_plan` |
| Message composition | ✅ | `draft_reply` and `ops_on_call_brief` |
| Issue/ticketing workflow | ✅ | `ticket_packager` |
| Local snapshot / diagnostics | ✅ | `local_exec_snapshot` |
| Incident postmortem support | ✅ | `incident_postmortem_brief` |
| SLO status reporting | ✅ | `slo_status_snapshot` |
| Risk register drafting | ✅ | `risk_register_draft` |
| Deployment readiness checklist | ✅ | `deployment_readiness_checklist` |
| Policy decision documentation | ✅ | `policy_decision_record` |
| Customer impact assessment | ✅ | `customer_impact_assessment` |
| Rollback strategy drafting | ✅ | `rollback_strategy` |
| Dependency health checks | ✅ | `dependency_health_check` |
| SLO breach timeline | ✅ | `sla_breach_timeline` |
| Audit finding summaries | ✅ | `audit_finding_summary` |
| Incident communication planning | ✅ | `incident_comm_plan` |
| Vendor dependency risk | ✅ | `vendor_dependency_risk` |
| Runbook validation | ✅ | `runbook_validation_checklist` |
| Cost estimate summary | ✅ | `cost_estimate_summary` |
| Payment workflow | ✅ | `payment_action_plan` |
| Compliance & redaction | ✅ | `compliance_audit_check`, `pii_scrub_report` |

## Coverage note

- Our pack now includes `38` handlers (named entries), up from 20, with the same policy-first model:
  - pure compute-first handlers,
  - explicit action-requests only for declared types.
- The two additions above (`web_research_draft`, `calendar_event_plan`) are intentionally **non-executing planners**. Execution against external systems should stay in future connectors/governed adapters (not in this pack).

## Non-obvious risks from the ecosystem

- OpenClaw ecosystem breadth is large (multiple categories), but its default model requires strong allowlisting and secrets gating to avoid unsafe skills.
- MCP tooling is powerful but discovery/security gaps vary by registry/provider; MCP registry moderation is intentionally limited, so allowlist + policy enforcement at our boundary is still required.
- Local execution paths are commonly broader than policy-safe runtime assumptions; SecureAgnt should keep the out-of-process default-deny model and explicit action contracts.
