# Cross-Platform Runtime Notes

This document captures the current M10 portability baseline for SecureAgnt.

## Scope
- Target families:
  - Ubuntu / Debian
  - Fedora / RHEL-family
  - Arch
  - openSUSE
  - macOS
- Runtime modes:
  - native binaries (`secureagnt-api`, `secureagntd`, `agntctl`)
  - container stack (`infra/containers/compose.yml`)

## Shared Requirements
- Rust toolchain (stable) for source builds.
- Postgres 18+ for state/audit data.
- TLS reverse proxy for production API exposure.
- Explicit config/state/log roots:
  - config: `/etc/secureagnt/`
  - state: `/var/lib/secureagnt/`
  - logs: `/var/log/secureagnt/`

## Linux Service Supervision (systemd)
- Unit templates:
  - `infra/systemd/secureagnt.service`
  - `infra/systemd/secureagnt-api.service`
- Install baseline:
```bash
sudo cp infra/systemd/secureagnt*.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now secureagnt.service secureagnt-api.service
```

## macOS Service Supervision (launchd)
- Plist templates:
  - `infra/launchd/secureagnt.plist`
  - `infra/launchd/secureagnt-api.plist`
- Install baseline:
```bash
sudo cp infra/launchd/secureagnt*.plist /Library/LaunchDaemons/
sudo launchctl load /Library/LaunchDaemons/secureagnt.plist
sudo launchctl load /Library/LaunchDaemons/secureagnt-api.plist
```

## Container Baseline
- Compose file: `infra/containers/compose.yml`
- Runtime images:
  - `infra/containers/Dockerfile.api`
  - `infra/containers/Dockerfile.worker`
- Stack boot:
```bash
make stack-build
make stack-up
make stack-ps
```

## Distro-Specific Notes
- Ubuntu / Debian:
  - use packaged `podman`/`docker` and `systemd` templates directly.
- Fedora / RHEL-family:
  - prefer Podman-native flows; SELinux policies may require explicit volume labels for custom paths.
- Arch:
  - verify `podman compose` provider availability (`make container-info`).
- openSUSE:
  - verify cgroup and service defaults for long-running worker units.
- macOS:
  - use launchd templates for binary deployments.
  - container workflows typically run via Docker Desktop/Podman Desktop.

## Validation Baseline
- Run:
```bash
make m10-signoff
```
- The signoff checks packaging templates and required portability documentation markers.

## Portability Signoff Checklist
Use this sequence before tagging a deployment candidate:

1. Baseline portability/docs check:
```bash
make m10-signoff
```
2. Deployment template preflight:
```bash
make deploy-preflight
```
3. Optional compose syntax/profile validation (recommended for container releases):
```bash
DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 make deploy-preflight
```
4. Optional release-manifest verification in the same preflight pass:
```bash
DEPLOY_PREFLIGHT_VALIDATE_COMPOSE=1 \
DEPLOY_PREFLIGHT_VERIFY_MANIFEST=1 \
RELEASE_MANIFEST_INPUT=dist/release-manifest.sha256 \
make deploy-preflight
```
5. Matrix gate for CI/static portability wiring:
```bash
make m10-matrix-gate
```
6. Record runtime validation evidence in:
- `docs/M10_EXECUTION_CHECKLIST.md`

## Practical Portability Notes
- Prefer profile-driven env files instead of editing compose/service units in-place:
  - `infra/config/profile.solo-dev.env`
  - `infra/config/profile.enterprise.env`
- Build throttling for smaller hosts:
  - `CARGO_BUILD_JOBS` for local cargo commands
  - `SECUREAGNT_CARGO_BUILD_JOBS` for container stack builds
- Compose path override is supported via `COMPOSE_FILE` in Make targets and preflight scripts.
- Keep runtime mode choices explicit in release notes:
  - native binaries (`secureagnt-api`, `secureagntd`)
  - container stack (`make stack-up*`)
- CI portability sanity is wired through:
  - `.github/workflows/ci.yml` `portability` job
  - `scripts/ops/m10_matrix_gate.sh`
