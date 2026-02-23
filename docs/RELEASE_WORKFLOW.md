# Release Workflow (SecureAgnt)

## Objective
Ship a tagged, auditable release with minimal manual steps and optional GitHub Actions dependency on paid compute.

## Release modes

Use manual local packaging when CI billing is constrained, or use GitHub Actions for automated artifact build/upload.

`make release-upload` and upload scripts are supported for both paths.

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

## Installer behavior

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
