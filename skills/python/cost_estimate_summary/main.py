#!/usr/bin/env python3
import importlib.util
from pathlib import Path


def _load_runner():
    shared_path = (
        Path(__file__)
        .resolve()
        .parent.parent / "_shared_top20_skill_runner.py"
    )
    spec = importlib.util.spec_from_file_location("top20_skill_runner", shared_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load shared runner from {shared_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_SKILL_NAME = "cost_estimate_summary"
_RUNNER = _load_runner()


def handle_describe(message: dict) -> dict:
    return _RUNNER.handle_describe(message, skill_name=_SKILL_NAME)


def handle_invoke(message: dict) -> dict:
    return _RUNNER.handle_invoke(message, skill_name=_SKILL_NAME)


def main() -> int:
    return _RUNNER.run(_SKILL_NAME)


if __name__ == "__main__":
    raise SystemExit(main())
