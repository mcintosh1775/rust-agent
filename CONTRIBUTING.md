# Contributing

## Ground rules
- Read `AGENTS.md` before making changes.
- Keep the trusted computing base small: policy + primitives + dispatcher.
- Prefer small PRs that preserve the MVP scope.

## Dev quickstart
- `docker compose up -d`
- `make check`

## Security
- See `SECURITY.md` and `docs/THREAT_MODEL.md`.
- Do not introduce patterns listed as forbidden in `SECURITY.md`.
