#!/usr/bin/env python3
import json
import sys
import importlib.util
from pathlib import Path
from typing import Any, Dict


_SKILL_IMPL_PATH = Path(__file__).resolve().with_name("skill_impl.py")
_spec = importlib.util.spec_from_file_location("skill_impl_main", _SKILL_IMPL_PATH)
if _spec is None or _spec.loader is None:
    raise RuntimeError(f"Unable to load shared skill implementation module from {_SKILL_IMPL_PATH}")
_skill_impl_main = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_skill_impl_main)


def handle_describe(message: dict, skill_name: str) -> Dict[str, Any]:
    return _skill_impl_main.describe_skill_output(skill_name, message.get("id", "unknown"))


def handle_invoke(message: Dict[str, Any], skill_name: str) -> Dict[str, Any]:
    return _skill_impl_main.invoke_skill_by_name(message, skill_name)


def run(skill_name: str) -> int:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        incoming = json.loads(line)
        msg_type = incoming.get("type")
        if msg_type == "describe":
            response = handle_describe(incoming, skill_name)
        elif msg_type == "invoke":
            response = handle_invoke(incoming, skill_name)
        else:
            response = {
                "type": "error",
                "id": incoming.get("id", "unknown"),
                "error": {
                    "code": "INVALID_INPUT",
                    "message": f"unsupported message type: {msg_type}",
                    "details": {},
                },
            }
        print(json.dumps(response), flush=True)
    return 0
