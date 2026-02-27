#!/usr/bin/env python3
"""Update or append a key in a shell-style environment file."""

from __future__ import annotations

import sys
from pathlib import Path


def _quoted_env(value: str) -> str:
    escaped = (
        value.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("$", "\\$")
        .replace("`", "\\`")
    )
    return f'"{escaped}"'


def main() -> int:
    if len(sys.argv) != 4:
        print(
            "usage: update_env_var.py <file-path> <key> <value>",
            file=sys.stderr,
        )
        return 1

    path = Path(sys.argv[1]).resolve()
    key = sys.argv[2]
    value = sys.argv[3]

    if key == "":
        print("key is required", file=sys.stderr)
        return 1

    env_line = f"{key}={_quoted_env(value)}\n"
    lines = []
    replaced = False

    if path.exists():
        existing = path.read_text(encoding="utf-8").splitlines(keepends=True)
        for line in existing:
            if line.lstrip().startswith(f"{key}="):
                if not replaced:
                    lines.append(env_line)
                    replaced = True
                else:
                    continue
            else:
                lines.append(line)
    else:
        path.parent.mkdir(parents=True, exist_ok=True)

    if not replaced:
        lines.append(env_line)

    path.write_text("".join(lines), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
