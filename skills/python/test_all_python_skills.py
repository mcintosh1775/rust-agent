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


if __name__ == "__main__":
    unittest.main()
