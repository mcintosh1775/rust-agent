# M10 Execution Checklist

Use this checklist to close M10 cross-platform runtime/packaging exit criteria with reproducible evidence.

## How To Use
1. Run baseline static gates:
```bash
make m10-signoff
make deploy-preflight
make m10-matrix-gate
```
2. For container targets, run:
```bash
DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 make deploy-preflight
```
3. For each target OS family below, record exact command output summaries and final pass/fail status.
4. Link incident/change records and operator notes in the evidence fields.

## Release Metadata
- candidate version:
- git commit:
- operator:
- date (UTC):

## Target Matrix

### Ubuntu / Debian
- host / image:
- runtime mode checked:
  - native binaries: `yes|no`
  - container stack: `yes|no`
- commands run:
  - `make build`
  - `make test`
  - `make m10-signoff`
  - `make deploy-preflight`
  - `make stack-up` + basic API/worker health checks (if container mode)
- result: `pass|fail`
- notes / evidence:

### Fedora / RHEL-family
- host / image:
- runtime mode checked:
  - native binaries: `yes|no`
  - container stack: `yes|no`
- commands run:
  - `make build`
  - `make test`
  - `make m10-signoff`
  - `make deploy-preflight`
  - `make stack-up` + basic API/worker health checks (if container mode)
- result: `pass|fail`
- notes / evidence:

### Arch
- host / image:
- runtime mode checked:
  - native binaries: `yes|no`
  - container stack: `yes|no`
- commands run:
  - `make build`
  - `make test`
  - `make m10-signoff`
  - `make deploy-preflight`
  - `make stack-up` + basic API/worker health checks (if container mode)
- result: `pass|fail`
- notes / evidence:

### openSUSE
- host / image:
- runtime mode checked:
  - native binaries: `yes|no`
  - container stack: `yes|no`
- commands run:
  - `make build`
  - `make test`
  - `make m10-signoff`
  - `make deploy-preflight`
  - `make stack-up` + basic API/worker health checks (if container mode)
- result: `pass|fail`
- notes / evidence:

### macOS
- host / image:
- runtime mode checked:
  - native binaries: `yes|no`
  - container stack: `yes|no`
- commands run:
  - `make build`
  - `make test`
  - `make m10-signoff`
  - `make deploy-preflight`
  - launchd template validation
- result: `pass|fail`
- notes / evidence:

## Signoff Summary
- blockers:
- deferred items:
- final M10 status: `complete|in_progress`
- approver:
