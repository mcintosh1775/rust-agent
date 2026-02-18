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
