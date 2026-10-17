"""Guard the wiring around the tested selector and evaluator."""
from pathlib import Path
import re
import unittest

from ci_selection import JOB_IDS

ROOT = Path(__file__).resolve().parents[2]


class WorkflowTests(unittest.TestCase):
    def test_every_ci_job_is_a_final_dependency(self):
        text = (ROOT / ".github/workflows/ci.yml").read_text()
        jobs = text.split("\njobs:\n", 1)[1]
        ids = set(re.findall(r"^  ([a-z][a-z0-9-]*):$", jobs, flags=re.M))
        self.assertEqual(ids, set(JOB_IDS) | {"selection", "required"})
        final = jobs.split("  required:\n", 1)[1]
        self.assertIn("name: CI required", final)
        self.assertIn("if: ${{ always() }}", final)
        dependencies = final.split("    needs:\n", 1)[1].split("    runs-on:", 1)[0]
        self.assertEqual(set(re.findall(r"^      - ([a-z0-9-]+)$", dependencies, re.M)), ids - {"required"})
        self.assertIn("CI_NEEDS: ${{ toJSON(needs) }}", final)
        self.assertIn("CI_SELECTION: ${{ needs.selection.outputs.selection }}", final)

    def test_job_conditions_match_the_selection_and_security_always_runs(self):
        text = (ROOT / ".github/workflows/ci.yml").read_text().split("\njobs:\n", 1)[1]
        for job in JOB_IDS:
            block = re.split(r"\n  [a-z][a-z0-9-]*:\n", text.split(f"  {job}:\n", 1)[1])[0]
            if job == "security":
                self.assertNotIn("    if:", block)
            else:
                self.assertIn("    needs: selection", block)
                self.assertIn("if: ${{ needs.selection.outputs['" + job + "'] == 'true' }}", block)

    def test_required_workflows_have_unfiltered_pr_triggers(self):
        for filename in ["ci.yml", "verify.yml", "independent-interop.yml"]:
            trigger = (ROOT / ".github/workflows" / filename).read_text().split("\njobs:", 1)[0]
            self.assertIn("  pull_request:", trigger)
            self.assertNotIn("pull_request_target:", trigger)
            self.assertNotIn("    paths", trigger)
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("permissions:\n  contents: read", ci)
        self.assertIn("  schedule:", ci)


if __name__ == "__main__":
    unittest.main()
