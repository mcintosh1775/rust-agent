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
4. Decide release tag, e.g. `v0.1.95`.

## Local release build path

From repo root:

```bash
TAG=v0.1.95
bash scripts/ops/package_release_assets.sh "${TAG}"
bash scripts/ops/package_release_deb.sh "${TAG}"
ls -lh dist/local-release/"${TAG}"
```

Expected artifacts include:

- `secureagnt-api-linux-x86_64-<tag>`
- `secureagntd-linux-x86_64-<tag>`
- `agntctl-linux-x86_64-<tag>`
- `*.tar.gz` equivalents for the same three files
- `secureagnt_<tag>_amd64.deb`
- `release-manifest.sha256`

You can upload manually:

```bash
export GITHUB_TOKEN=...
bash scripts/ops/upload_release_assets.sh v0.1.95 dist/local-release/v0.1.95
```

If you already use a token and want make-driven upload:

```bash
TAG=v0.1.95
make release-upload TAG="${TAG}" RELEASE_DIR=dist/local-release/"${TAG}" REPO_NAME=mcintosh1775/rust-agent
```

## GitHub Actions release path

Trigger the workflow with a tag:

1. Go to Actions → Release
2. Run workflow
3. Set `release_tag` to the exact tag, for example `v0.1.95`

The workflow builds Linux x86_64 artifacts, generates the manifest, builds `.deb`, and publishes all files to the release.

## CI workflow (validation) controls

`.github/workflows/ci.yml` is manual-trigger-only and does not run on every push.
Use workflow dispatch and one of:

- `run_all: true`
- `run_build: true`
- `run_portability: true`
- `run_solo_lite_signoff: true`
- `run_enterprise_whitenoise_smoke: true`

This prevents CI from consuming quota on routine commits.

## Solo-lite install flow (recommended for testing a new server quickly)

On the target server, the installer is the one to use for interactive setup:

```bash
curl -fsSL https://raw.githubusercontent.com/mcintosh1775/rust-agent/main/scripts/install/secureagnt-solo-lite-installer.sh \
  -o /tmp/secureagnt-solo-lite-installer.sh
chmod +x /tmp/secureagnt-solo-lite-installer.sh
SECUREAGNT_RELEASE_REPO=mcintosh1775/rust-agent \
SECUREAGNT_RELEASE_VERSION=v0.1.95 \
SECUREAGNT_PLATFORM_TAG=linux-x86_64 \
bash /tmp/secureagnt-solo-lite-installer.sh
```

How this works:
- It first tries to download tagged release binaries from GitHub Releases (`...-linux-x86_64-<tag>`), then legacy names.
- If download is unavailable, it falls back to local git+`cargo build` when tools are present.
- It runs solo-lite bootstrap so you end with a seeded agent context and startup guidance.

To quickly verify the script before full install on the server:

```bash
bash /tmp/secureagnt-solo-lite-installer.sh --help
```

To verify the installer on a server after download (non-destructive smoke):

```bash
cd /tmp
SECUREAGNT_RELEASE_REPO=mcintosh1775/rust-agent \
SECUREAGNT_RELEASE_VERSION=v0.1.95 \
SECUREAGNT_PLATFORM_TAG=linux-x86_64 \
bash /tmp/secureagnt-solo-lite-installer.sh
```

Follow installer prompts for agent name, persona, and sandbox directories. Use `SECUREAGNT_NON_INTERACTIVE=1` with explicit env values for scripted runs.

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

If all downloads fail and build tools are available, it falls back to local `cargo build` for missing binaries.

## Post-release validation

1. Confirm release page has all expected files.
2. Run a quick smoke on target host after download/install.
3. Verify tag appears once in `git tag --list --sort=creatordate` and branch log.
4. Move to deployment.

## Optional dependency security check

To validate Rust dependency health outside CI:

```bash
make cargo-audit
```

This command skips when crates.io is unreachable unless `CARGO_AUDIT_REQUIRE_NETWORK=1`.
