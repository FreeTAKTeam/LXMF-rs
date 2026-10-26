"""Guard the wiring around the tested selector and evaluator."""
import os
from pathlib import Path
import re
import subprocess
import unittest

from ci_selection import JOB_IDS

ROOT = Path(__file__).resolve().parents[2]


class WorkflowTests(unittest.TestCase):
    def verify_suites(self):
        text = (ROOT / ".github/workflows/verify.yml").read_text()
        suites, final = text.split("\n  hil-pr:\n", 1)
        blocks = re.findall(r"^      - .*?(?=\n      - |\Z)", suites, re.M | re.S)
        return suites, blocks, final

    def test_python_suites_run_independently_and_keep_all_checks_required(self):
        suites, _, final = self.verify_suites()
        self.assertIn("suite: [controller, compatibility, interfaces, tools]", suites)
        self.assertIn("fail-fast: false", suites)
        self.assertNotIn("    needs:", suites)
        self.assertNotIn("continue-on-error:", suites)
        self.assertIn("name: HIL PR level", final)
        self.assertIn("if: ${{ always() }}", final)
        self.assertIn("needs: python-compat", final)
        self.assertIn("SUITES_RESULT: ${{ needs.python-compat.result }}", final)
        command = final.split("        run: ", 1)[1].strip()
        for status in ["success", "failure", "cancelled", "skipped", "", "pending"]:
            with self.subTest(status=status):
                result = subprocess.run(
                    ["bash", "-c", command], env=dict(os.environ, SUITES_RESULT=status)
                )
                self.assertEqual(result.returncode == 0, status == "success")

    def test_python_test_commands_belong_to_exactly_one_suite(self):
        _, blocks, _ = self.verify_suites()
        assigned = {suite: [] for suite in ["controller", "compatibility", "interfaces", "tools"]}
        for block in blocks:
            if "        run:" not in block or "        if:" not in block:
                continue
            matches = re.findall(r"matrix.suite == '([a-z]+)'", block)
            self.assertEqual(len(matches), 1, block)
            self.assertIn(matches[0], assigned)
            assigned[matches[0]].append(block)
        for suite, checks in assigned.items():
            self.assertTrue(checks, suite)
        self.assertTrue(any("hil run --level pr --all" in b for b in assigned["controller"]))
        self.assertTrue(any("python_compat_matrix.py" in b and "--all" in b
                            for b in assigned["compatibility"]))
        for block in blocks:
            if "cargo test" in block or "./tools/scripts/" in block:
                self.assertIn("matrix.suite == '", block)
        clean_check = next(b for b in blocks if "Exclude auxiliary checkouts" in b)
        self.assertNotIn("        if:", clean_check)
        self.assertLess(blocks.index(clean_check), next(
            i for i, b in enumerate(blocks) if "python_compat_matrix.py" in b
        ))

    def test_python_suites_cache_rust_before_building_and_retain_evidence(self):
        suites, blocks, _ = self.verify_suites()
        cache = next(b for b in blocks if "Swatinem/rust-cache@v2" in b)
        self.assertIn("key: verify-${{ matrix.suite }}", cache)
        self.assertIn("cache-targets: true", cache)
        self.assertIn("cache-on-failure: false", cache)
        self.assertLess(suites.index("dtolnay/rust-toolchain@stable"), suites.index(cache))
        self.assertLess(suites.index(cache), suites.index("cargo test"))
        expected = {
            "hil-pr": "controller",
            "python-compat-matrix": "compatibility",
            "python-behavioral-inventory": "interfaces",
            "python-rust-wire-conformance": "interfaces",
            "local-interface-python-shared": "interfaces",
            "pipe-fake-subprocess": "interfaces",
        }
        uploads = [b for b in blocks if "uses: actions/upload-artifact@v4" in b]
        self.assertEqual(len(uploads), len(expected))
        for name, suite in expected.items():
            block = next(b for b in uploads if f"name: {name}-${{{{ github.run_id }}}}" in b)
            self.assertIn("always() && matrix.suite == '" + suite + "'", block)

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
