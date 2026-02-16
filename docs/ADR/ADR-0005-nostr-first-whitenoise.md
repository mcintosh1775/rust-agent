# ADR-0005: Nostr-first messaging with White Noise as first-class connector

## Status
Accepted

## Context
The platform needs a primary secure messaging channel aligned with the product direction toward Nostr-native communication.

Enterprise users may still require Slack integration for existing workflows.

## Decision
Make White Noise chat the first-class messaging connector and keep the platform Nostr-first.

White Noise message delivery is modeled through `message.send` with explicit allowlisted scopes. White Noise transport uses the Marmot protocol over Nostr.

Slack remains supported as an enterprise connector, but secondary to White Noise in MVP and roadmap prioritization.

## Consequences
- Capability policy and examples must include White Noise scopes as primary.
- MVP demos and docs should default to White Noise notifications.
- Connector implementation order prioritizes White Noise before Slack.
