"""The required HIL gate must prove complete, same-commit shard evidence."""
import json
from pathlib import Path
import tempfile
import unittest

from hil_pr_evidence import expected_cases, validate

ROOT = Path(__file__).resolve().parents[2]


class HilEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        self.cases = sorted(expected_cases(ROOT))

    def report(self, cases):
        return {"schema": "lxmf-rs-hil-result-v1", "commit_sha": "candidate", "level": "pr",
                "result": "PASS", "cases": [
                    {"profile_id": profile, "case_id": case, "result": "PASS"}
                    for profile, case in cases]}

    def write(self, report, name="one"):
        path = self.path / name / "result.json"
        path.parent.mkdir(exist_ok=True)
        path.write_text(json.dumps(report))

    def test_all_cases_partitioned_across_shards_pass(self):
        for index in range(4):
            self.write(self.report(self.cases[index::4]), str(index))
        self.assertEqual(validate(ROOT, self.path, "candidate"), (4, len(self.cases)))

    def test_missing_duplicate_or_unknown_case_is_rejected(self):
        for cases in [self.cases[:-1], self.cases + self.cases[:1],
                      self.cases + [("virtual", "unknown")]]:
            with self.subTest(cases=cases):
                self.write(self.report(cases))
                with self.assertRaises(ValueError):
                    validate(ROOT, self.path, "candidate")

    def test_failed_stale_or_wrong_level_report_is_rejected(self):
        for field, value in [("result", "FAIL_PROTOCOL"), ("commit_sha", "stale"),
                             ("level", "nightly"), ("schema", "unknown")]:
            with self.subTest(field=field):
                report = self.report(self.cases)
                report[field] = value
                self.write(report)
                with self.assertRaises(ValueError):
                    validate(ROOT, self.path, "candidate")

    def test_failed_case_cannot_hide_behind_passing_summary(self):
        report = self.report(self.cases)
        report["cases"][0]["result"] = "BLOCKED"
        self.write(report)
        with self.assertRaises(ValueError):
            validate(ROOT, self.path, "candidate")

    def test_absent_or_invalid_report_is_rejected(self):
        with self.assertRaises(ValueError):
            validate(ROOT, self.path, "candidate")
        (self.path / "result.json").write_text("{")
        with self.assertRaises(ValueError):
            validate(ROOT, self.path, "candidate")


if __name__ == "__main__":
    unittest.main()
