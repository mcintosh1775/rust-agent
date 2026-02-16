# ADR-0001: Out-of-process skills with action-request model

## Status
Accepted

## Decision
Skills MUST run out-of-process and may not perform side effects directly.
Skills return `action_requests`; platform evaluates policy and executes allowed actions.
