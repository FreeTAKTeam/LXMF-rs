"""A green final check must prove every selected dependency succeeded."""
import copy
import unittest

from ci_gate import evaluate
from ci_selection import required_jobs, select
from test_ci_selection import metadata


class GateTests(unittest.TestCase):
    def setUp(self):
        self.selection = select(None, None)
        self.needs = {key: {"result": "success"} for key in self.selection["jobs"]}
        self.needs["selection"] = {"result": "success"}

    def test_all_required_success_passes(self):
        self.assertEqual(evaluate(self.selection, self.needs), [])

    def test_failed_cancelled_missing_or_skipped_required_job_never_passes(self):
        for job in ["selection", "security", "test-integration", "build-matrix"]:
            for status in ["failure", "cancelled", "skipped", "pending", None]:
                with self.subTest(job=job, status=status):
                    needs = copy.deepcopy(self.needs)
                    if status is None:
                        del needs[job]
                    else:
                        needs[job]["result"] = status
                    self.assertTrue(evaluate(self.selection, needs))

    def test_deliberate_inapplicable_job_may_skip_but_not_fail(self):
        self.selection = select(None, None, event="schedule")
        for job, required in self.selection["jobs"].items():
            self.needs[job]["result"] = "success" if required else "skipped"
        self.assertEqual(evaluate(self.selection, self.needs), [])
        self.needs["quality"]["result"] = "failure"
        self.assertTrue(evaluate(self.selection, self.needs))

    def test_malformed_or_incomplete_selection_fails(self):
        for value in [None, {}, {"version": 100}, dict(self.selection, jobs={}),
                      dict(self.selection, mode="unknown"), dict(self.selection, jobs={"security": "true"})]:
            with self.subTest(value=value):
                self.assertTrue(evaluate(value, self.needs))

    def test_missing_or_unexpected_dependency_fails(self):
        for needs in [None, {}, dict(self.needs, surprise={"result": "success"})]:
            self.assertTrue(evaluate(self.selection, needs))

    def test_security_cannot_be_deselected(self):
        self.selection["jobs"]["security"] = False
        self.assertTrue(evaluate(self.selection, self.needs))

    def test_broad_integration_cannot_be_deselected_or_removed(self):
        for replacement in [{"jobs": dict(self.selection["jobs"], **{"test-integration": False})},
                            {"workspace_tests": False}, {"integration": "invalid"},
                            {"reasons": []}]:
            self.assertTrue(evaluate(dict(self.selection, **replacement), self.needs))

    def test_valid_metadata_broad_selection_cannot_omit_integration(self):
        selection = select(metadata(), [("M", "Cargo.lock")])
        selection["integration"] = []
        selection["jobs"] = required_jobs(selection)
        self.needs["test-integration"]["result"] = "skipped"
        self.assertTrue(evaluate(selection, self.needs))


if __name__ == "__main__":
    unittest.main()
