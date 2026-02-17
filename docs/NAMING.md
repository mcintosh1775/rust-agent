# NAMING AND PACKAGING

This document defines the current naming standard for SecureAgnt.

## Product Identity
- Brand/domain: `SecureAgnt.ai`
- Project name: `SecureAgnt`
- Repo/org base: `secureagnt`

## User-Facing Binaries
- Primary CLI: `agntctl`
- Primary daemon/service binary: `secureagntd`
- API service binary alias: `secureagnt-api`

Compatibility note:
- Legacy `api` and `worker` binary names are still available.

## Rust Package Naming (Target)
For public crates/releases, prefer explicit package names:
- `secureagnt_core`
- `secureagnt_agent`
- `secureagnt_transport`
- `secureagnt_store`
- `secureagnt_crypto`
- `secureagnt_plugins`

Current status:
- Not fully migrated yet; tracked under roadmap milestone `M0N`.

## Fleet Paths and Service Naming
- Config dir: `/etc/secureagnt/`
- Primary config: `/etc/secureagnt/secureagnt.yaml`
- State dir: `/var/lib/secureagnt/`
- Logs dir: `/var/log/secureagnt/`
- systemd unit: `secureagnt.service`

## Skill and Policy Terminology
- Agent instance: `agent`
- Plugin/action unit: `skill`
- Guardrails: `policy`
- Evidence trail: `audit`

## Capability Naming Convention
Use simple, auditable capability names:
- `exec:command`
- `fs:read`
- `fs:write`
- `net:http`
- `svc:systemd`
- `pkg:apt`
- `msg:slack`

## Skill Packaging Convention
- Rust skill crate prefix: `secureagnt_skill_<name>`
- Python package prefix: `secureagnt-skill-<name>`
- Python module prefix: `secureagnt_skill_<name>`
- Recommended skill manifest file: `skill.toml`

## CLI Surface Baseline
- `agntctl status`
- `agntctl config validate`
- `agntctl skills list`
- `agntctl skills info <id>`
- `agntctl skills install <source>`
- `agntctl policy allow ...`
- `agntctl policy deny ...`
- `agntctl audit tail`
