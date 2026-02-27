#!/usr/bin/env python3
"""Unit tests for scripts/ops/release_distribution_check.sh."""

from __future__ import annotations

import tempfile
import subprocess
from pathlib import Path


SCRIPT_PATH = Path(__file__).resolve().parent / "release_distribution_check.sh"


def _run_check(tag: str, release_dir: Path, workflow_file: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "bash",
            str(SCRIPT_PATH),
            tag,
            "linux-x86_64",
            str(release_dir),
            str(workflow_file),
        ],
        capture_output=True,
        text=True,
    )


def _write_file(path: Path) -> None:
    path.write_text("payload")


def _write_manifest(release_dir: Path, tag: str) -> None:
    safe_tag = tag.replace("/", "-")
    safe_version = tag.removeprefix("v").replace("/", "-")
    files = [
        f"secureagnt-api-linux-x86_64-{safe_tag}",
        f"secureagntd-linux-x86_64-{safe_tag}",
        f"agntctl-linux-x86_64-{safe_tag}",
        f"secureagnt-nostr-keygen-linux-x86_64-{safe_tag}",
        f"secureagnt-solo-lite-installer-{safe_tag}.sh",
        "secureagnt-solo-lite-installer.sh",
        f"secureagnt-api-linux-x86_64-{safe_tag}.tar.gz",
        f"secureagntd-linux-x86_64-{safe_tag}.tar.gz",
        f"agntctl-linux-x86_64-{safe_tag}.tar.gz",
        f"secureagnt-nostr-keygen-linux-x86_64-{safe_tag}.tar.gz",
        "release-manifest.sha256",
        f"secureagnt_{safe_version}_amd64.deb",
    ]
    manifest = release_dir / "release-manifest.sha256"
    manifest.write_text("\n".join((f"0123456789abcdef  {name}" for name in files)) + "\n")


def _create_release_layout(release_dir: Path, tag: str) -> None:
    safe_tag = tag.replace("/", "-")
    safe_version = tag.removeprefix("v").replace("/", "-")
    required_files = [
        f"secureagnt-api-linux-x86_64-{safe_tag}",
        f"secureagntd-linux-x86_64-{safe_tag}",
        f"agntctl-linux-x86_64-{safe_tag}",
        f"secureagnt-nostr-keygen-linux-x86_64-{safe_tag}",
        f"secureagnt-solo-lite-installer-{safe_tag}.sh",
        "secureagnt-solo-lite-installer.sh",
        f"secureagnt-api-linux-x86_64-{safe_tag}.tar.gz",
        f"secureagntd-linux-x86_64-{safe_tag}.tar.gz",
        f"agntctl-linux-x86_64-{safe_tag}.tar.gz",
        f"secureagnt-nostr-keygen-linux-x86_64-{safe_tag}.tar.gz",
        "release-manifest.sha256",
        f"secureagnt_{safe_version}_amd64.deb",
    ]
    for file_name in required_files:
        _write_file(release_dir / file_name)
    _write_manifest(release_dir, tag)


def _workflow_with_template_patterns(workflow_file: Path, tag: str) -> None:
    safe_tag = tag.replace("/", "-")
    workflow_file.write_text(
        "\n".join(
            [
                f"secureagnt-nostr-keygen-linux-x86_64-{safe_tag}.tar.gz",
                "secureagnt-nostr-keygen-${{ env.PLATFORM_TAG }}-${{ steps.release_meta.outputs.safe_tag_name }}.tar.gz",
                f"secureagnt-nostr-keygen-linux-x86_64-{safe_tag}",
                "secureagnt-nostr-keygen-${{ env.PLATFORM_TAG }}-${{ steps.release_meta.outputs.safe_tag_name }}",
            ]
        )
        + "\n"
    )


def _workflow_without_nostr_keygen_patterns(workflow_file: Path) -> None:
    workflow_file.write_text("secureagnt-api-linux-x86_64-v0.0.0.tar.gz\n")


def test_release_distribution_check_passes_when_artifacts_and_manifest_match():
    tag = "v0.2.99"
    with tempfile.TemporaryDirectory() as workspace:
        release_dir = Path(workspace)
        workflow_file = release_dir / "workflow.yml"
        _create_release_layout(release_dir, tag)
        _write_file(workflow_file)
        _workflow_with_template_patterns(workflow_file, tag)

        proc = _run_check(tag, release_dir, workflow_file)
        assert proc.returncode == 0
        assert "passed for tag=" in proc.stdout


def test_release_distribution_check_fails_when_artifacts_missing():
    tag = "v0.2.99"
    with tempfile.TemporaryDirectory() as workspace:
        release_dir = Path(workspace)
        workflow_file = release_dir / "workflow.yml"
        _create_release_layout(release_dir, tag)
        (release_dir / "secureagnt-api-linux-x86_64-v0.2.99").unlink()
        _write_file(workflow_file)
        _workflow_with_template_patterns(workflow_file, tag)

        proc = _run_check(tag, release_dir, workflow_file)
        assert proc.returncode == 1
        assert "missing artifact" in proc.stderr


def test_release_distribution_check_fails_when_manifest_entry_missing():
    tag = "v0.2.99"
    with tempfile.TemporaryDirectory() as workspace:
        release_dir = Path(workspace)
        workflow_file = release_dir / "workflow.yml"
        _create_release_layout(release_dir, tag)
        manifest = release_dir / "release-manifest.sha256"
        manifest.write_text("deadbeef  secureagnt-api-linux-x86_64-v0.2.99\n")
        _write_file(workflow_file)
        _workflow_with_template_patterns(workflow_file, tag)

        proc = _run_check(tag, release_dir, workflow_file)
        assert proc.returncode == 1
        assert "manifest missing entry for: secureagntd-linux-x86_64-v0.2.99" in proc.stderr


def test_release_distribution_check_fails_when_workflow_patterns_absent():
    tag = "v0.2.99"
    with tempfile.TemporaryDirectory() as workspace:
        release_dir = Path(workspace)
        workflow_file = release_dir / "workflow.yml"
        _create_release_layout(release_dir, tag)
        _workflow_without_nostr_keygen_patterns(workflow_file)

        proc = _run_check(tag, release_dir, workflow_file)
        assert proc.returncode == 1
        assert "workflow missing artifact pattern" in proc.stderr
