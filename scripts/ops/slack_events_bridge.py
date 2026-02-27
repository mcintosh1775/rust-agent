#!/usr/bin/env python3
"""Bridge Slack Events API callbacks to SecureAgnt webhook triggers.

This script lets a real Slack channel become an operator ingress path without
building a separate daemon.

Usage pattern:
- Configure a Slack app Event Subscriptions callback URL to this script's public URL.
- Run this bridge so it creates or reuses an operator webhook trigger.
- The script posts each Slack message event into
  ``/v1/triggers/<id>/events`` as ``payload`` data.
"""

from __future__ import annotations

import argparse
import json
import hashlib
import hmac
import os
import re
import sys
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


def _env(name: str) -> str:
    return os.environ.get(name, "").strip()


def _require_arg(value: str | None, name: str) -> str:
    if value and value.strip():
        return value.strip()
    raise ValueError(f"--{name} is required")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Forward Slack Events API callbacks into SecureAgnt webhook trigger events."
    )
    parser.add_argument("--base-url", default="http://127.0.0.1:8080")
    parser.add_argument("--tenant-id", default="single")
    parser.add_argument("--agent-id", default="", help="Agent UUID for auto-created trigger.")
    parser.add_argument("--trigger-id", default="", help="Existing webhook trigger UUID.")
    parser.add_argument(
        "--recipe-id",
        default="operator_chat_v1",
        help="Recipe for auto-created trigger.",
    )
    parser.add_argument("--triggered-by-user-id", default="")
    parser.add_argument("--auth-proxy-token", default="")
    parser.add_argument("--trigger-secret", default="")
    parser.add_argument(
        "--trigger-secret-ref",
        default="",
        help="Optional trigger-secret reference for create + ingest headers.",
    )
    parser.add_argument(
        "--signing-secret",
        default="",
        help="Slack signing secret string (use --signing-secret-env for env variable name).",
    )
    parser.add_argument(
        "--signing-secret-env",
        default="SLACK_SIGNING_SECRET",
        help="Environment variable that holds the Slack signing secret.",
    )
    parser.add_argument(
        "--verify-signature",
        action="store_true",
        help="Verify X-Slack-Signature/X-Slack-Request-Timestamp on callbacks.",
    )
    parser.add_argument(
        "--allowed-channels",
        default="",
        help="Optional comma-separated Slack channel IDs to accept.",
    )
    parser.add_argument(
        "--channel-allowlist",
        default="",
        help="Alias for --allowed-channels.",
    )
    parser.add_argument("--max-text-bytes", type=int, default=12000)
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=9000)
    parser.add_argument("--path", default="/slack/events")
    parser.add_argument(
        "--state-file",
        default="",
        help="Optional JSON file for persisting created trigger id.",
    )
    parser.add_argument(
        "--user-role",
        default="owner",
        choices=["owner", "operator", "viewer"],
        help="Role used for trigger create requests.",
    )
    parser.add_argument(
        "--skip-subtypes",
        default="bot_message,message_changed,message_deleted",
        help="Comma-separated Slack message event subtypes to ignore.",
    )
    return parser.parse_args()


def _http_json(
    *,
    base_url: str,
    method: str,
    path: str,
    tenant_id: str,
    user_role: str,
    user_id: str | None,
    timeout_secs: float,
    body: dict,
    trigger_secret: str | None = None,
    auth_proxy_token: str | None = None,
) -> tuple[int, dict | None, str]:
    url = f"{base_url.rstrip('/')}/{path.lstrip('/')}"
    payload = json.dumps(body).encode("utf-8")
    headers = {
        "x-tenant-id": tenant_id,
        "x-user-role": user_role,
        "Content-Type": "application/json",
    }
    if user_id:
        headers["x-user-id"] = user_id
    if trigger_secret:
        headers["x-trigger-secret"] = trigger_secret
    if auth_proxy_token:
        headers["x-auth-proxy-token"] = auth_proxy_token

    request = Request(
        url,
        data=payload,
        headers=headers,
        method=method.upper(),
    )

    try:
        with urlopen(request, timeout=timeout_secs) as response:
            raw = response.read().decode("utf-8")
            return response.status, _safe_json(raw), raw
    except HTTPError as exc:
        raw = exc.read().decode("utf-8") if exc.fp else ""
        return exc.code, None, raw
    except URLError as exc:
        raise RuntimeError(f"failed requesting {url}: {exc}") from exc


def _safe_json(raw: str) -> dict | None:
    try:
        parsed = json.loads(raw)
        if isinstance(parsed, dict):
            return parsed
    except json.JSONDecodeError:
        pass
    return None


def _read_state_trigger_id(path: str) -> str:
    if not path:
        return ""
    try:
        state = json.loads(Path(path).read_text(encoding="utf-8"))
        value = str(state.get("trigger_id", "")).strip()
        if value and _is_uuid(value):
            return value
    except FileNotFoundError:
        return ""
    except (OSError, json.JSONDecodeError):
        return ""
    return ""


def _write_state_trigger_id(path: str, trigger_id: str) -> None:
    if not path:
        return
    try:
        Path(path).write_text(
            json.dumps({"trigger_id": trigger_id}, indent=2),
            encoding="utf-8",
        )
    except OSError:
        print(f"[warn] could not write state file {path}", file=sys.stderr)


def _is_uuid(raw: str) -> bool:
    return bool(re.fullmatch(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-"
        r"[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        raw,
    ))


def _resolve_signing_secret(args: argparse.Namespace) -> str:
    if args.signing_secret:
        return args.signing_secret
    if args.signing_secret_env:
        env_value = _env(args.signing_secret_env)
        if env_value:
            return env_value
    return ""


def _verify_signature(secret: str, body: bytes, signature: str, timestamp: str) -> bool:
    try:
        event_ts = int(timestamp or "0")
    except ValueError:
        return False
    if not secret or event_ts <= 0:
        return False
    now = int(time.time())
    if abs(now - event_ts) > 300:
        return False
    base = f"v0:{timestamp}:{body.decode('utf-8', errors='ignore')}"
    digest = hmac.new(
        secret.encode("utf-8"),
        base.encode("utf-8"),
        hashlib.sha256,
    ).hexdigest()
    expected = f"v0={digest}"
    return hmac.compare_digest(expected, signature or "")


class SlackBridge:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.trigger_id = ""
        self.signing_secret = _resolve_signing_secret(args)
        if args.verify_signature and not self.signing_secret:
            raise RuntimeError(
                "signature verification requested but no signing secret available"
            )
        self.allowed_channels = {
            c.strip()
            for c in [x for x in re.split(r"[,\s]+", args.allowed_channels or args.channel_allowlist) if x.strip()]
            if c.strip()
        }
        self.ignore_subtypes = {
            s.strip()
            for s in args.skip_subtypes.split(",")
            if s.strip()
        }
        self._ensure_trigger()

    def _ensure_trigger(self) -> None:
        trigger_id = self.args.trigger_id.strip()
        if not trigger_id and self.args.state_file:
            trigger_id = _read_state_trigger_id(self.args.state_file)
        if trigger_id:
            if not _is_uuid(trigger_id):
                raise RuntimeError(f"invalid trigger id {trigger_id!r}")
            self.trigger_id = trigger_id
            print(f"[bridge] using existing trigger {self.trigger_id}")
            return

        agent_id = _require_arg(self.args.agent_id, "agent-id")
        status, payload, raw = _http_json(
            base_url=self.args.base_url,
            method="POST",
            path="/v1/triggers/webhook",
            tenant_id=self.args.tenant_id,
            user_role=self.args.user_role,
            user_id=(self.args.triggered_by_user_id.strip() or None),
            timeout_secs=10.0,
            trigger_secret=(self.args.trigger_secret or "").strip() or None,
            auth_proxy_token=(self.args.auth_proxy_token.strip() or None),
            body={
                "agent_id": agent_id,
                "triggered_by_user_id": self.args.triggered_by_user_id.strip() or None,
                "recipe_id": self.args.recipe_id,
                "input": {
                    "source": "slack_events_api",
                    "channel": "slack",
                    "reply_to_event_author": True,
                },
                "requested_capabilities": [],
                "max_attempts": 3,
                "max_inflight_runs": 1,
                "jitter_seconds": 0,
                "webhook_secret_ref": self.args.trigger_secret_ref.strip() or None,
            },
        )
        if status not in {200, 201} or not isinstance(payload, dict):
            raise RuntimeError(
                f"create webhook trigger failed status={status}: {raw}"
            )
        created_id = str(payload.get("id", "")).strip()
        if not _is_uuid(created_id):
            raise RuntimeError(f"create webhook trigger response missing id: {payload}")
        self.trigger_id = created_id
        _write_state_trigger_id(self.args.state_file, self.trigger_id)
        print(f"[bridge] created trigger {self.trigger_id}")

    def _post_trigger_event(self, event_id: str, payload: dict) -> tuple[int, dict | None]:
        status, response, raw = _http_json(
            base_url=self.args.base_url,
            method="POST",
            path=f"/v1/triggers/{self.trigger_id}/events",
            tenant_id=self.args.tenant_id,
            user_role=self.args.user_role,
            user_id=(self.args.triggered_by_user_id.strip() or None),
            timeout_secs=10.0,
            trigger_secret=(self.args.trigger_secret or "").strip() or None,
            auth_proxy_token=(self.args.auth_proxy_token.strip() or None),
            body={"event_id": event_id, "payload": payload},
        )
        return status, response

    def handle_event(self, body: bytes) -> dict:
        envelope = json.loads(body.decode("utf-8"))
        if not isinstance(envelope, dict):
            return {"status": "ignored", "reason": "malformed"}

        if envelope.get("type") == "url_verification":
            return {
                "status": "challenge",
                "challenge": envelope.get("challenge", ""),
            }

        if envelope.get("type") != "event_callback":
            return {"status": "ignored", "reason": "unsupported payload type"}

        event = envelope.get("event")
        if not isinstance(event, dict):
            return {"status": "ignored", "reason": "missing event object"}

        if event.get("type") != "message":
            return {"status": "ignored", "reason": "non-message event"}

        subtype = str(event.get("subtype", "")).strip()
        if subtype and subtype in self.ignore_subtypes:
            return {"status": "ignored", "reason": f"ignored subtype {subtype!r}"}

        user = str(event.get("user", "")).strip()
        channel = str(event.get("channel", "")).strip()
        if not user or not channel:
            return {"status": "ignored", "reason": "missing user/channel"}

        if self.allowed_channels and channel not in self.allowed_channels:
            return {
                "status": "ignored",
                "reason": f"channel {channel!r} not in allowlist",
            }

        text = str(event.get("text", "")).strip()
        if not text:
            return {"status": "ignored", "reason": "empty text"}
        if len(text.encode("utf-8")) > self.args.max_text_bytes:
            text = text.encode("utf-8")[: self.args.max_text_bytes].decode(
                "utf-8",
                errors="ignore",
            ).strip()

        event_id = str(envelope.get("event_id", "")).strip() or str(
            event.get("client_msg_id", "")
        ).strip()
        if not event_id:
            event_id = str(uuid.uuid4())

        payload = {
            "channel": "slack",
            "event": {
                "user": user,
                "channel": channel,
                "text": text,
                "ts": str(event.get("ts", "")),
                "thread_ts": str(event.get("thread_ts", "")),
                "event_id": event_id,
            },
            "source": "slack_events_api",
            "team": str(envelope.get("team_id", "")),
            "api_app_id": str(envelope.get("api_app_id", "")),
        }
        status, response = self._post_trigger_event(event_id=event_id, payload=payload)
        if status not in {200, 202} or not isinstance(response, dict):
            raise RuntimeError(
                f"event enqueue failed status={status} response={response}"
            )

        return {
            "status": "queued",
            "trigger_event_id": event_id,
            "enqueued_status": response.get("status", ""),
            "trigger_id": self.trigger_id,
        }


class SlackBridgeHandler(BaseHTTPRequestHandler):
    bridge: SlackBridge

    def do_POST(self) -> None:  # noqa: N802
        if self.path != self.server.bridge_path:
            self._send_json({"error": "not found"}, status=404)
            return

        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        if not body:
            self._send_json({"error": "empty body"}, status=400)
            return

        if self.server.bridge_verify_signature:
            signature = self.headers.get("X-Slack-Signature", "")
            timestamp = self.headers.get("X-Slack-Request-Timestamp", "")
            if not _verify_signature(
                self.server.bridge_secret,
                body,
                signature,
                timestamp,
            ):
                self._send_json({"error": "invalid signature"}, status=401)
                return

        try:
            result = self.server.bridge.handle_event(body)
        except Exception as exc:
            print(f"[bridge] failed handle_event: {exc}", file=sys.stderr)
            self._send_json({"error": "processing failure"}, status=500)
            return

        self._send_json(result, status=200)

    def log_message(self, fmt, *args):  # pragma: no cover - intentionally concise
        print(f"[bridge] {fmt % args}")

    def _send_json(self, payload: dict, status: int) -> None:
        encoded = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


class SlackBridgeServer(ThreadingHTTPServer):
    bridge: SlackBridge
    bridge_path: str
    bridge_secret: str
    bridge_verify_signature: bool


def main() -> int:
    args = _parse_args()
    bridge = SlackBridge(args)

    server = SlackBridgeServer((args.host, args.port), SlackBridgeHandler)
    server.bridge = bridge
    server.bridge_path = args.path if args.path.startswith("/") else f"/{args.path}"
    server.bridge_secret = _resolve_signing_secret(args)
    server.bridge_verify_signature = bool(args.verify_signature)

    print(
        "[bridge] secureagnt-slack-events-bridge starting",
        f"listen={args.host}:{args.port}",
        f"path={server.bridge_path}",
        f"trigger={bridge.trigger_id}",
        f"tenant={args.tenant_id}",
        f"recipe={args.recipe_id}",
        sep=" ",
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("[bridge] stopped")
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
