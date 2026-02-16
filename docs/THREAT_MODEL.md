# Threat Model (MVP-first)

## Assets
- Secrets, tenant data, integrity of side effects, availability, audit logs

## Trust boundaries
- Client→API, API/Worker→DB, Worker→Skill process, Worker→External (via primitives)

## Top threats
1) Prompt injection → unauthorized actions  
   Mitigation: default-deny capabilities, audit, approvals later
2) Secret exfiltration  
   Mitigation: secret firewall, redaction, scrub connector outputs
3) SSRF (when http exists)  
   Mitigation: host allowlist, block private/link-local/metadata, DNS+redirect checks
4) Supply-chain skills  
   Mitigation: MVP no third-party installs; later signing + sandbox
5) DoS/runaway  
   Mitigation: time budgets, concurrency limits, payload caps

## MVP acceptance criteria
- Skills cannot access secrets
- Skills cannot cause side effects without explicit grants
- Every action request is allow/deny audited
- Skill timeouts/crashes are contained
