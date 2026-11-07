import json
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import python_compat_matrix as matrix


class PythonCompatibilityMatrixTests(unittest.TestCase):
    def test_required_contract_is_dispatchable(self) -> None:
        self.assertEqual(matrix.dispatch_errors(), [])
        self.assertEqual(len(matrix.REQUIRED_CASES), 30)

    def test_target_reference_profile_is_explicit_and_frozen(self) -> None:
        args = matrix.parse_args(["--reference-profile", "lxmf-1.2.0"])
        self.assertEqual(args.reference_profile, "lxmf-1.2.0")
        self.assertEqual(matrix.parse_args([]).reference_profile, "baseline")
        self.assertEqual(matrix.LXMF_REFERENCE_PROFILES["lxmf-1.2.0"],
                         "c3ff2d6dc2f256daab896dadc044dd5a913ecbb7")

    def test_reference_profiles_preserve_exact_pin_and_clean_candidate_gates(self) -> None:
        for profile, expected in matrix.LXMF_REFERENCE_PROFILES.items():
            with self.subTest(profile=profile), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                harness = root / "harness.py"
                harness.touch()
                metadata = [
                    {"revision": "candidate", "clean": True},
                    {"revision": matrix.EXPECTED_RETICULUM_REVISION, "clean": True},
                    {"revision": expected, "clean": True},
                ]

                def check() -> list[str]:
                    with patch.object(matrix, "git_metadata", side_effect=metadata), \
                         patch.object(matrix, "run_capture", return_value=(0, "", "")), \
                         patch.object(matrix.shutil, "which", return_value="python3"):
                        errors, _ = matrix.preflight("python3", "bash", harness, root, root, profile)
                    return errors

                self.assertEqual(check(), [])
                metadata[2]["revision"] = next(
                    revision for name, revision in matrix.LXMF_REFERENCE_PROFILES.items()
                    if name != profile
                )
                self.assertTrue(any("LXMF checkout does not match" in error for error in check()))
                metadata[2]["revision"] = expected
                metadata[1]["revision"] = "another-rns-revision"
                self.assertTrue(any("Reticulum checkout does not match" in error for error in check()))
                metadata[1]["revision"] = matrix.EXPECTED_RETICULUM_REVISION
                for candidate in metadata:
                    candidate["clean"] = False
                    self.assertTrue(any("dirty" in error for error in check()))
                    candidate["clean"] = True

    def test_missing_dispatch_is_reported(self) -> None:
        original = matrix.SMOKE_SCRIPT_CASES
        try:
            matrix.SMOKE_SCRIPT_CASES = set(original) - {"direct_rust_to_python"}
            errors = matrix.dispatch_errors()
        finally:
            matrix.SMOKE_SCRIPT_CASES = original
        self.assertTrue(any("direct_rust_to_python" in error for error in errors))

    def test_cargo_evidence_rejects_ignored_required_tests(self) -> None:
        status, reason, summaries = matrix.validate_cargo_evidence(
            "test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 29 filtered out"
        )
        self.assertEqual(status, "fail")
        self.assertIn("ignored", reason)
        self.assertEqual(summaries[0]["ignored"], 1)

    def test_live_evidence_requires_pass_status(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            report_path = Path(temporary) / "report.json"
            report_path.write_text(json.dumps({"status": "fail"}), encoding="utf-8")
            status, reason, report = matrix.validate_live_evidence(report_path)
        self.assertEqual(status, "fail")
        self.assertIn("status", reason)
        self.assertEqual(report, {"status": "fail"})

    def test_live_evidence_rejects_skipped_or_ignored_fields(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            report_path = Path(temporary) / "report.json"
            report_path.write_text(json.dumps({"status": "pass", "skipped": 1}), encoding="utf-8")
            status, reason, _ = matrix.validate_live_evidence(report_path)
        self.assertEqual(status, "fail")
        self.assertIn("skipped", reason)

    def test_overall_status_rejects_partial_acceptance_by_default(self) -> None:
        status, exit_code = matrix.overall_status(
            ["direct_python_to_rust"],
            [{"status": "pass"}],
            [],
            allow_subset=False,
        )
        self.assertEqual((status, exit_code), ("partial", 1))

    def test_overall_status_can_be_used_for_development_subset(self) -> None:
        status, exit_code = matrix.overall_status(
            ["direct_python_to_rust"],
            [{"status": "pass"}],
            [],
            allow_subset=True,
        )
        self.assertEqual((status, exit_code), ("partial", 0))


if __name__ == "__main__":
    unittest.main()
