#!/usr/bin/env python3
import json
import sys
import time


def summarize_text(text: str) -> str:
    text = text.strip()
    if not text:
        return "# Summary\n\n_No content provided._"

    lines = [line.strip() for line in text.splitlines() if line.strip()]
    preview = " ".join(lines)[:240]
    return "# Summary\n\n" + preview


def handle_describe(message: dict) -> dict:
    return {
        "type": "describe_result",
        "id": message["id"],
        "skill": {
            "name": "summarize_transcript",
            "version": "0.1.0",
            "description": "Summarize transcript text into markdown.",
            "inputs_schema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "mode": {"type": "string"},
                    "sleep_s": {"type": "number"},
                    "bytes": {"type": "integer"},
                    "request_write": {"type": "boolean"},
                },
            },
            "outputs_schema": {
                "type": "object",
                "properties": {"markdown": {"type": "string"}},
                "required": ["markdown"],
            },
            "requested_capabilities": [
                {"capability": "object.write", "scope": "shownotes/*"}
            ],
            "action_types": ["object.write"],
        },
    }


def handle_invoke(message: dict) -> dict:
    payload = message.get("input") or {}
    mode = payload.get("mode", "success")

    if mode == "timeout":
        time.sleep(float(payload.get("sleep_s", 10)))

    if mode == "crash":
        sys.exit(17)

    if mode == "oversize":
        size = int(payload.get("bytes", 100_000))
        markdown = "x" * size
    else:
        markdown = summarize_text(str(payload.get("text", "")))

    action_requests = []
    if payload.get("request_write"):
        action_requests.append(
            {
                "action_id": "a-1",
                "action_type": "object.write",
                "args": {
                    "path": "shownotes/ep245.md",
                    "content": markdown,
                },
                "justification": "Persist generated show notes",
            }
        )

    return {
        "type": "invoke_result",
        "id": message["id"],
        "output": {"markdown": markdown},
        "action_requests": action_requests,
    }


def main() -> int:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        incoming = json.loads(line)
        msg_type = incoming.get("type")
        if msg_type == "describe":
            response = handle_describe(incoming)
        elif msg_type == "invoke":
            response = handle_invoke(incoming)
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


if __name__ == "__main__":
    raise SystemExit(main())
