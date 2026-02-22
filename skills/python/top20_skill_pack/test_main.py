#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import unittest


def _load_skill_module():
    module_path = Path(__file__).with_name("main.py")
    spec = importlib.util.spec_from_file_location("top20_main", module_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load module spec from {module_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


TOP20_MAIN = _load_skill_module()


class Top20SkillPackTests(unittest.TestCase):
    def test_describe_manifest_has_expected_new_skills(self):
        describe = TOP20_MAIN.handle_describe()
        manifest_names = {row.get("name") for row in describe["skill"]["manifest"]}
        self.assertIn("web_research_draft", manifest_names)
        self.assertIn("calendar_event_plan", manifest_names)
        self.assertIn("incident_postmortem_brief", manifest_names)
        self.assertIn("slo_status_snapshot", manifest_names)

    def test_skill_order_includes_new_skills(self):
        describe = TOP20_MAIN.handle_describe()
        available = describe["skill"]["available_skills"]
        self.assertIn("web_research_draft", available)
        self.assertIn("calendar_event_plan", available)
        self.assertIn("incident_postmortem_brief", available)
        self.assertIn("slo_status_snapshot", available)
        self.assertTrue(available.index("web_research_draft") < available.index("calendar_event_plan"))
        self.assertTrue(available.index("calendar_event_plan") < available.index("incident_postmortem_brief"))

    def test_runtime_alias_resolution(self):
        self.assertEqual(
            TOP20_MAIN._resolve_skill({"runtime": {"recipe_id": "show_notes_v1"}}),
            "summarize_transcript",
        )
        self.assertEqual(
            TOP20_MAIN._resolve_skill({"runtime": {"recipe_id": "payments_v1"}}),
            "payment_action_plan",
        )


if __name__ == "__main__":
    unittest.main()
