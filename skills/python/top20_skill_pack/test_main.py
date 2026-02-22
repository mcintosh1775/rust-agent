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
        self.assertIn("risk_register_draft", manifest_names)
        self.assertIn("deployment_readiness_checklist", manifest_names)
        self.assertIn("policy_decision_record", manifest_names)
        self.assertIn("customer_impact_assessment", manifest_names)
        self.assertIn("rollback_strategy", manifest_names)
        self.assertIn("dependency_health_check", manifest_names)
        self.assertIn("sla_breach_timeline", manifest_names)
        self.assertIn("audit_finding_summary", manifest_names)
        self.assertIn("incident_comm_plan", manifest_names)
        self.assertIn("vendor_dependency_risk", manifest_names)
        self.assertIn("runbook_validation_checklist", manifest_names)
        self.assertIn("cost_estimate_summary", manifest_names)

    def test_skill_order_includes_new_skills(self):
        describe = TOP20_MAIN.handle_describe()
        available = describe["skill"]["available_skills"]
        self.assertIn("web_research_draft", available)
        self.assertIn("calendar_event_plan", available)
        self.assertIn("incident_postmortem_brief", available)
        self.assertIn("slo_status_snapshot", available)
        self.assertIn("risk_register_draft", available)
        self.assertIn("deployment_readiness_checklist", available)
        self.assertIn("policy_decision_record", available)
        self.assertIn("customer_impact_assessment", available)
        self.assertIn("rollback_strategy", available)
        self.assertIn("dependency_health_check", available)
        self.assertIn("sla_breach_timeline", available)
        self.assertIn("audit_finding_summary", available)
        self.assertIn("incident_comm_plan", available)
        self.assertIn("vendor_dependency_risk", available)
        self.assertIn("runbook_validation_checklist", available)
        self.assertIn("cost_estimate_summary", available)
        self.assertTrue(available.index("web_research_draft") < available.index("calendar_event_plan"))
        self.assertTrue(available.index("calendar_event_plan") < available.index("incident_postmortem_brief"))
        self.assertTrue(available.index("incident_postmortem_brief") < available.index("slo_status_snapshot"))
        self.assertTrue(available.index("slo_status_snapshot") < available.index("risk_register_draft"))
        self.assertTrue(available.index("risk_register_draft") < available.index("deployment_readiness_checklist"))
        self.assertTrue(available.index("deployment_readiness_checklist") < available.index("policy_decision_record"))
        self.assertTrue(available.index("policy_decision_record") < available.index("customer_impact_assessment"))
        self.assertTrue(available.index("customer_impact_assessment") < available.index("rollback_strategy"))
        self.assertTrue(available.index("rollback_strategy") < available.index("dependency_health_check"))
        self.assertTrue(available.index("dependency_health_check") < available.index("sla_breach_timeline"))
        self.assertTrue(available.index("sla_breach_timeline") < available.index("audit_finding_summary"))
        self.assertTrue(available.index("audit_finding_summary") < available.index("incident_comm_plan"))
        self.assertTrue(available.index("incident_comm_plan") < available.index("vendor_dependency_risk"))
        self.assertTrue(available.index("vendor_dependency_risk") < available.index("runbook_validation_checklist"))
        self.assertTrue(available.index("runbook_validation_checklist") < available.index("cost_estimate_summary"))
        self.assertTrue(available.index("cost_estimate_summary") < available.index("code_change_summary"))

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
