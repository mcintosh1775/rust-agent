#!/usr/bin/env python3
import importlib.util
import inspect
import unittest
from pathlib import Path


BASE_DIR = Path(__file__).resolve().parent


def _load_module(module_name: str, module_path: Path):
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load module spec from {module_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _python_skill_dirs():
    dirs = []
    for entry in sorted(BASE_DIR.iterdir()):
        if not entry.is_dir() or entry.name.startswith("."):
            continue
        if not entry.is_dir():
            continue
        if entry.name == "__pycache__":
            continue
        if entry.name.startswith("_"):
            continue
        main_py = entry / "main.py"
        skill_md = entry / "SKILL.md"
        if main_py.exists() and skill_md.exists():
            dirs.append((entry.name, main_py))
    return dirs


class PythonSkillsBehaviorTests(unittest.TestCase):
    def _summarize_transcript_module(self):
        return _load_module(
            "python_skill_summarize_transcript",
            BASE_DIR / "summarize_transcript" / "main.py",
        )

    def test_all_python_skills_have_describe_and_invoke(self):
        for skill_name, main_path in _python_skill_dirs():
            module = _load_module(f"python_skill_{skill_name}", main_path)
            self.assertTrue(
                hasattr(module, "handle_describe"),
                f"{skill_name}: missing handle_describe",
            )
            self.assertTrue(
                hasattr(module, "handle_invoke"),
                f"{skill_name}: missing handle_invoke",
            )

            # describe may be implemented with or without input message.
            if "message" in inspect.signature(module.handle_describe).parameters:
                describe = module.handle_describe({"id": "python-skills-describe"})
            else:
                describe = module.handle_describe()
            self.assertEqual(
                describe.get("type"),
                "describe_result",
                f"{skill_name}: invalid describe_result type",
            )
            self.assertIn("skill", describe)
            self.assertIn("name", describe["skill"])
            if "manifest" in describe["skill"]:
                self.assertIsInstance(describe["skill"]["manifest"], list)

    def test_all_python_skills_invoke_return_output(self):
        for skill_name, main_path in _python_skill_dirs():
            module = _load_module(f"python_skill_invoke_{skill_name}", main_path)
            invoke = module.handle_invoke(
                {
                    "id": f"python-skills-invoke-{skill_name}",
                    "input": {"text": f"sample input for {skill_name}"},
                }
            )
            self.assertEqual(
                invoke.get("type"),
                "invoke_result",
                f"{skill_name}: invalid invoke result type",
            )
            self.assertIn("output", invoke)
            self.assertIn("markdown", invoke["output"])

    def test_summarize_text_preserves_version_token(self):
        module = self._summarize_transcript_module()
        summary = module.summarize_text(
            "Agent 'solo-lite-agent' is now upgraded to SecureAgnt v0.2.28 (destinations: slack:C0AGRN3B895)."
        )
        self.assertIn("v0.2.28", summary)
        self.assertNotIn("v0. 2.", summary)

    def test_summarize_text_normalizes_whitespace_in_version(self):
        module = self._summarize_transcript_module()
        summary = module.summarize_text(
            "Agent 'solo-lite-agent' is now upgraded to SecureAgnt v0. 2. 28 (destinations: slack:C0AGRN3B895)."
        )
        self.assertIn("v0.2.28", summary)
        self.assertNotIn("v0. 2.", summary)

    def test_summarize_ops_digest_keeps_release_text(self):
        module = self._summarize_transcript_module()
        digest = module.summarize_ops_digest(
            "Agent 'solo-lite-agent' is now upgraded to SecureAgnt v0.2.28 (destinations: slack:C0AGRN3B895)."
        )
        self.assertIn("v0.2.28", digest)
        self.assertNotIn("v0. 2.", digest)

    def test_extract_whitenoise_event_for_slack_nested_payload(self):
        module = self._summarize_transcript_module()
        event_ctx = module._extract_whitenoise_event(
            {
                "event_payload": {
                    "channel": "slack",
                    "event": {
                        "user": "U123456",
                        "text": "hello from channel",
                        "channel": "C777777",
                        "ts": "1710000000.123456",
                    },
                    "source": "slack_event",
                }
            }
        )
        self.assertEqual(event_ctx["provider"], "slack")
        self.assertEqual(event_ctx["author_pubkey"], "U123456")
        self.assertEqual(event_ctx["content"], "hello from channel")
        self.assertEqual(event_ctx["reply_channel"], "C777777")
        self.assertEqual(event_ctx["event_id"], "1710000000.123456")

    def test_extract_whitenoise_event_for_trigger_envelope_payload(self):
        module = self._summarize_transcript_module()
        event_ctx = module._extract_whitenoise_event(
            {
                "event_payload": {
                    "channel": "slack",
                    "event_id": "evt-123",
                    "event": {
                        "user": "U987654",
                        "text": "enveloped event path",
                        "channel": "C888888",
                        "ts": "1711111111.000000",
                    },
                },
                "payload": {
                    "event_payload": {
                        "channel": "slack",
                        "event": {
                            "user": "U998877",
                            "text": "wrapper should be ignored if top-level exists",
                            "channel": "C999999",
                            "event_id": "evt-ignored",
                        },
                    }
                },
            }
        )
        self.assertEqual(event_ctx["provider"], "slack")
        self.assertEqual(event_ctx["author_pubkey"], "U987654")
        self.assertEqual(event_ctx["content"], "enveloped event path")
        self.assertEqual(event_ctx["reply_channel"], "C888888")
        self.assertEqual(event_ctx["event_id"], "evt-123")

    def test_extract_whitenoise_event_for_slack_legacy_top_level_event(self):
        module = self._summarize_transcript_module()
        event_ctx = module._extract_whitenoise_event(
            {
                "channel": "slack",
                "event": {
                    "user": "U999999",
                    "text": "legacy path event",
                    "channel": "C123456",
                    "ts": "1712345678.000000",
                    "thread_ts": "",
                    "client_msg_id": "evt-legacy-1",
                },
                "source": "slack_events_api",
            }
        )
        self.assertEqual(event_ctx["provider"], "slack")
        self.assertEqual(event_ctx["author_pubkey"], "U999999")
        self.assertEqual(event_ctx["content"], "legacy path event")
        self.assertEqual(event_ctx["reply_channel"], "C123456")
        self.assertEqual(event_ctx["event_id"], "evt-legacy-1")

    def test_extract_whitenoise_event_falls_back_to_top_level_for_whitenoise_payload(self):
        module = self._summarize_transcript_module()
        event_ctx = module._extract_whitenoise_event(
            {
                "text": "manual command",
                "channel": "whitenoise",
                "event": {"pubkey": "npub1foo", "content": "payload content"},
            }
        )
        self.assertEqual(event_ctx["provider"], "whitenoise")
        self.assertEqual(event_ctx["author_pubkey"], "npub1foo")
        self.assertEqual(event_ctx["content"], "payload content")

    def test_invoke_defaults_to_llm_for_inbound_messages(self):
        module = self._summarize_transcript_module()
        invoke = module.handle_invoke(
            {
                "id": "summarize-invoke-inbound-default-llm",
                "input": {
                    "event_payload": {
                        "channel": "slack",
                        "event": {"user": "U123", "text": "hello agent", "channel": "C1"},
                        "source": "slack_event",
                    },
                    "text": "ignored because event has content",
                },
            }
        )
        actions = invoke["action_requests"]
        action_types = [action["action_type"] for action in actions]
        self.assertIn("llm.infer", action_types)
        self.assertIn("message.send", action_types)
        self.assertLess(
            action_types.index("llm.infer"), action_types.index("message.send")
        )
        message = next(action for action in actions if action["action_type"] == "message.send")
        self.assertIn("{{llm_response}}", message["args"]["text"])
        llm_action = next(action for action in actions if action["action_type"] == "llm.infer")
        self.assertIn("Respond helpfully to this operator message", llm_action["args"]["prompt"])


if __name__ == "__main__":
    unittest.main()
