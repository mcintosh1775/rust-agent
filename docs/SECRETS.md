# Secrets Guide

This document defines how SecureAgnt resolves secrets and how to operate secret backends safely.

## Resolver model
- Secret values should be provided by reference (`*_REF`) instead of inline env values.
- Shared reference schemes:
  - `env:VAR_NAME`
  - `file:/path/to/secret.txt`
  - `vault:...`
  - `aws-sm:...`
  - `gcp-sm:...`
  - `azure-kv:...`
- Cloud CLI adapters are fail-closed unless enabled:
  - `SECUREAGNT_SECRET_ENABLE_CLOUD_CLI=1`

## Secret cache (runtime)
- API and worker resolve secrets through a shared TTL cache wrapper to reduce repeated backend calls.
- Cache controls:
  - `SECUREAGNT_SECRET_CACHE_TTL_SECS` (default `30`, `0` disables cache)
  - `SECUREAGNT_SECRET_CACHE_MAX_ENTRIES` (default `1024`)
- Rotation behavior:
  - values are refreshed automatically when TTL expires
  - set shorter TTL for faster rotation pickup

## Version pinning formats

### Vault
- Base: `vault:kv/data/app/slack#token`
- Pin version: `vault:kv/data/app/slack#token?version=3`
- Notes:
  - `#field` selects a key inside `data.data`
  - `version` maps to `vault kv get -version=<n>`

### AWS Secrets Manager
- Base: `aws-sm:prod/secureagnt/slack-webhook`
- Pin immutable version id: `aws-sm:prod/secureagnt/slack-webhook?version_id=<uuid>`
- Pin version stage: `aws-sm:prod/secureagnt/slack-webhook?version_stage=AWSCURRENT`
- Only one of `version_id` or `version_stage` may be set.

### Google Secret Manager
- Existing supported forms:
  - `gcp-sm:project:secret:latest`
  - `gcp-sm:projects/<project>/secrets/<secret>/versions/latest`
- Query-based version pin:
  - `gcp-sm:project:secret?version=42`

### Azure Key Vault
- Base secret id:
  - `azure-kv:https://my-vault.vault.azure.net/secrets/my-secret`
- Pin version:
  - `azure-kv:https://my-vault.vault.azure.net/secrets/my-secret?version=<version>`

## Provider auth strategy

### Vault
- Recommended:
  - Kubernetes auth for in-cluster deployments
  - AppRole for VM/bare-metal workers
- Avoid long-lived root/dev tokens on worker hosts.

### AWS
- Recommended:
  - IAM role / instance profile / IRSA workload identity
- Avoid static access keys in `.env` files.

### GCP
- Recommended:
  - Workload Identity / service account bindings
- Avoid user credential JSON on runtime hosts when possible.

### Azure
- Recommended:
  - Managed Identity + Key Vault access policies/RBAC
- Avoid shared client secrets on worker hosts.

## Operational guardrails
- Keep cloud CLI adapters disabled unless required.
- Keep secrets out of run payloads, skill protocol payloads, and artifacts.
- Rotate secrets with overlap windows when possible:
  - publish new version
  - shift version stage/alias
  - wait for cache TTL window
  - retire prior version
