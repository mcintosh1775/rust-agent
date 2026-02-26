# Release Workflow (SecureAgnt)

## Objective
Ship a tagged, auditable release with minimal manual steps and optional GitHub Actions dependency on paid compute.

## Install paths after release (choose one)

Use manual local packaging when CI billing is constrained, or use GitHub Actions for automated artifact build/upload.

`make release-upload` and upload scripts are supported for both paths.

Two post-build setup flows are supported:

1. **Solo-lite install flow** (operator bootstrap + persona/context init, best for single-agent/dev use)
2. **Debian/server install flow** (service baseline install, enterprise-style deployment)

## Required pre-release checks

1. Clean repo state: commit all intended changes.
2. Confirm version alignment with `make verify-workspace-versions`.
3. Update `CHANGELOG.md` for user-visible scope, validation, and any risk notes.
4. Decide release tag, e.g. `v0.1.98`.
5. Ensure startup-message text includes the exact release token on a representative host before publish:

   ```bash
   RELEASE_GATE_RUN_STARTUP_SMOKE=1 \
   RELEASE_SMOKE_DB_PATH=<runtime_db_path> \
   RELEASE_SMOKE_EXPECTED_TAG=<tag> \
   make release-gate
   ```

   For a local one-off check on a solo-lite host:

   ```bash
   RELEASE_SMOKE_DB_PATH=<runtime_db_path> \
   RELEASE_SMOKE_EXPECTED_TAG=<tag> \
   make release-startup-smoke
   ```

## Local release build path

From repo root:

```bash
TAG=v0.1.98
bash scripts/ops/package_release_assets.sh "${TAG}"
bash scripts/ops/package_release_deb.sh "${TAG}"
ls -lh dist/local-release/"${TAG}"
```

Expected artifacts include:

- `secureagnt-api-linux-x86_64-<tag>`
- `secureagntd-linux-x86_64-<tag>`
- `agntctl-linux-x86_64-<tag>`
- `secureagnt-nostr-keygen-linux-x86_64-<tag>`
- `secureagnt-solo-lite-installer-<tag>.sh`
- `secureagnt-solo-lite-installer.sh` (stable alias)
- `*.tar.gz` equivalents for the same four files
- `secureagnt-nostr-keygen-linux-x86_64-<tag>.tar.gz`
- `secureagnt_<tag>_amd64.deb`
- `release-manifest.sha256`

You can upload manually:

```bash
export GITHUB_TOKEN=...
bash scripts/ops/upload_release_assets.sh v0.1.98 dist/local-release/v0.1.98
```

If you already use a token and want make-driven upload:

```bash
TAG=v0.1.98
make release-upload TAG="${TAG}" RELEASE_DIR=dist/local-release/"${TAG}" REPO_NAME=mcintosh1775/rust-agent
```

## GitHub Actions release path

Trigger the workflow with a tag:

1. Go to Actions → Release
2. Run workflow
3. Set `release_tag` to the exact tag, for example `v0.1.98`

The workflow now resolves the tag once, checks out that tag explicitly, and then builds Linux x86_64 artifacts, generates the manifest, builds `.deb`, and publishes all files to the release.

## CI workflow (validation) controls

`.github/workflows/ci.yml` is manual-trigger-only and does not run on every push.
Use workflow dispatch and one of:

- `run_all: true`
- `run_build: true`
- `run_portability: true`
- `run_solo_lite_signoff: true`
- `run_enterprise_whitenoise_smoke: true`

This prevents CI from consuming quota on routine commits.

## Solo-light install flow (recommended for testing a new server quickly)

## Quick operator commands

1) Download the pinned installer:

```bash
export TAG=<release_tag> # e.g. v0.2.1 or "latest"

if [[ "${TAG}" == "latest" || "${TAG}" == "" ]]; then
  ASSET_NAME="secureagnt-solo-lite-installer.sh"
  ASSET_ID="$(curl -fsSL \
    -H "Authorization: token ${GITHUB_TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    https://api.github.com/repos/mcintosh1775/rust-agent/releases/latest \
    | jq -r --arg name "${ASSET_NAME}" '.assets[] | select(.name == $name) | .id' \
    | head -n 1)"
else
  ASSET_NAME="secureagnt-solo-lite-installer-${TAG//\//-}.sh"
  ASSET_ID="$(curl -fsSL \
    -H "Authorization: token ${GITHUB_TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    https://api.github.com/repos/mcintosh1775/rust-agent/releases/tags/"${TAG}" \
    | jq -r --arg name "${ASSET_NAME}" '.assets[] | select(.name == $name) | .id' \
    | head -n 1)"
fi

curl -L -fsSL \
  -H "Authorization: token ${GITHUB_TOKEN}" \
  -H "Accept: application/octet-stream" \
  "https://api.github.com/repos/mcintosh1775/rust-agent/releases/assets/${ASSET_ID}" \
  -o /tmp/secureagnt-solo-lite-installer.sh
chmod +x /tmp/secureagnt-solo-lite-installer.sh
```

For public repos (or when `GITHUB_TOKEN` is unavailable), you can still use:

```bash
curl -fsSL "https://github.com/mcintosh1775/rust-agent/releases/download/${TAG}/secureagnt-solo-lite-installer-${TAG//\//-}.sh" \
  -o /tmp/secureagnt-solo-lite-installer.sh
chmod +x /tmp/secureagnt-solo-lite-installer.sh
```

Or latest alias:

```bash
curl -fsSL "https://github.com/mcintosh1775/rust-agent/releases/latest/download/secureagnt-solo-lite-installer.sh" \
  -o /tmp/secureagnt-solo-lite-installer.sh
chmod +x /tmp/secureagnt-solo-lite-installer.sh
```

2) Run interactive bootstrap install (binary download + SOUL prompts + local sqlite initialization + service setup/start):

> If this repository is private, set `GITHUB_TOKEN` in your shell before this step (can be removed once public).

```bash
cd /tmp
SECUREAGNT_RELEASE_VERSION=${TAG} \
SECUREAGNT_PLATFORM_TAG=linux-x86_64 \
bash /tmp/secureagnt-solo-lite-installer.sh
```

If you configure Slack messaging during install, set destination allowlist entries to Slack destination IDs:
- `WORKER_MESSAGE_SLACK_DEST_ALLOWLIST=...` expects `C...`/`G...`/`D...` values.
- Workspace IDs (`T...`) are not valid destination IDs for messaging allowlists.
- Authentication is webhook-based: provide `SLACK_WEBHOOK_URL` (or `SLACK_WEBHOOK_URL_REF`) during install/runtime and ensure a workspace owner/admin has provisioned a **Slack App Incoming Webhook** for the workspace.
- `SLACK_WEBHOOK_URL` is sensitive; prefer `SLACK_WEBHOOK_URL_REF` with your secret adapter and avoid committing raw values.

Slack app setup quick path:

1. Open `https://api.slack.com/apps`.
2. Choose **From scratch**.
3. Enable **Incoming Webhooks**.
4. Add a new webhook to the desired channel.
5. Copy the generated `https://hooks.slack.com/services/...` URL.

This defaults to `bootstrap` mode, which runs bootstrap prompts, initializes the solo-lite SQLite profile, writes service files, and starts services by default (`SECUREAGNT_START_SERVICES=1`).
By default on root/system runs, binaries are placed in `/usr/local/bin` and installer workspace is `/opt/secureagnt`.
Use `sudo` (or set `SECUREAGNT_SERVICE_SCOPE=user` explicitly) because this flow uses system service files.
Use `--solo-light` for service-based install without bootstrap prompts.
For a new release, you can omit `SECUREAGNT_RELEASE_VERSION` to auto-resolve `latest` from GitHub.

3) Run solo-light service mode if you only want systemd service files:

```bash
cd /tmp
SECUREAGNT_RELEASE_VERSION=${TAG} \
SECUREAGNT_PLATFORM_TAG=linux-x86_64 \
bash /tmp/secureagnt-solo-lite-installer.sh --solo-light
```

4) Run fully scripted setup (copy/paste and edit values):

```bash
cd /tmp
SECUREAGNT_NON_INTERACTIVE=1 \
SECUREAGNT_RELEASE_VERSION=${TAG} \
SECUREAGNT_PLATFORM_TAG=linux-x86_64 \
SECUREAGNT_AGENT_NAME="home-ops-liaison" \
SECUREAGNT_AGENT_ROLE="Home operations coordinator for a single server" \
SECUREAGNT_SOUL_STYLE="concise, practical, safety-first" \
SECUREAGNT_SOUL_VALUES="secure-by-default, clear auditability, explicit boundaries" \
SECUREAGNT_SOUL_BOUNDARIES="never bypass policy, never expose secrets, escalate uncertainty" \
SECUREAGNT_SANDBOX_ROOT="/opt/secureagnt" \
bash /tmp/secureagnt-solo-lite-installer.sh
```

5) Pre-run validation only (no changes):

```bash
cd /tmp
SECUREAGNT_NON_INTERACTIVE=1 \
SECUREAGNT_RELEASE_VERSION=${TAG} \
SECUREAGNT_PLATFORM_TAG=linux-x86_64 \
SECUREAGNT_AGENT_NAME="home-ops-liaison" \
SECUREAGNT_AGENT_ROLE="Home operations coordinator for a single server" \
SECUREAGNT_SOUL_STYLE="concise, practical, safety-first" \
SECUREAGNT_SOUL_VALUES="secure-by-default, clear auditability, explicit boundaries" \
SECUREAGNT_SOUL_BOUNDARIES="never bypass policy, never expose secrets, escalate uncertainty" \
SECUREAGNT_SANDBOX_ROOT="/opt/secureagnt" \
bash /tmp/secureagnt-solo-lite-installer.sh --dry-run
```

How this works:
- It uses release artifacts only (`secureagnt-api`, `secureagntd`, `agntctl`) for the selected tag/platform.
- It also pulls `secureagnt-nostr-keygen` when `NOSTR_SIGNER_MODE=local_key` (bootstrap default).
- It tries download candidates in this order: `-linux-x86_64-<tag>`, then `-linux-x86_64`, then legacy names.
- It fails fast if required binaries are missing.
- It then runs solo-light install and writes minimal `systemd` service files with sqlite runtime defaults.
- Service logs are written to `/var/log/secureagnt/<service>.log` for quick troubleshooting by default, in addition to `journalctl -u`.

For details and a quick dry-run check, see the numbered steps above.
When checking API health on a fresh solo-lite install, include tenant and operator headers:
`curl -sf -H 'x-tenant-id: single' "http://127.0.0.1:8080/v1/ops/summary?window_secs=60"`.

The installer is not required for Debian-based production deployment, but it is the quickest path for solo-lite functional testing.

## Debian/server install flow (service baseline)

Use this path when deploying long-lived services:

1. Install the `.deb` package for the target tag:

```bash
sudo dpkg -i secureagnt_<tag>_amd64.deb
```

2. Edit `/etc/secureagnt/secureagnt.env` (database, API binding, queues, secrets, egress policy).
3. Start services:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now secureagnt-api.service secureagnt.service
```

4. Confirm service health before persona bootstrap (which is separate from this packaging path).

## Installer behavior (artifact selection)

The solo-lite installer now tries release assets in this order:

1. `-linux-x86_64-<tag>` suffix
2. existing non-suffixed `-linux-x86_64`
3. legacy filenames

By default, installer downloads are treated as required.
Installer exits with a clear error if required binaries are not available.

## Post-release validation

1. Confirm release page has all expected files.
2. Run a quick smoke on target host after download/install.
3. Verify startup message formatting smoke before closing release:
   - Run `make release-startup-smoke` with the runtime DB path and expected release tag.
   - For a local solo-lite host:
     ```bash
     RELEASE_GATE_RUN_STARTUP_SMOKE=1 \
     RELEASE_SMOKE_DB_PATH=/opt/secureagnt/secureagnt.sqlite3 \
     RELEASE_SMOKE_EXPECTED_TAG=v0.2.29 \
     make release-gate
     ```
   - For a targeted one-off check:
     ```bash
     RELEASE_SMOKE_DB_PATH=/opt/secureagnt/secureagnt.sqlite3 \
     RELEASE_SMOKE_EXPECTED_TAG=v0.2.29 \
     make release-startup-smoke
     ```
4. Verify tag appears once in `git tag --list --sort=creatordate` and branch log.
5. Move to deployment.

## Optional dependency security check

To validate Rust dependency health outside CI:

```bash
make cargo-audit
```

This command skips when crates.io is unreachable unless `CARGO_AUDIT_REQUIRE_NETWORK=1`.
