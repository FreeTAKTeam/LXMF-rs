import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch

from branch_protection import Inaccessible, classify, inspect, main, normalize, verify_checks

POLICY = json.loads((Path(__file__).resolve().parents[2] / ".github/branch-protection.json").read_text())["protection"]


def live_policy():
    result = copy.deepcopy(POLICY)
    for key, value in list(result.items()):
        if type(value) is bool:
            result[key] = {"enabled": value, "url": "https://api.github.com/example"}
    result["required_status_checks"]["contexts"] = [row["context"] for row in result["required_status_checks"]["checks"]]
    result["required_signatures"] = {"enabled": False}
    result["url"] = "https://api.github.com/example"
    return result


class ProtectionTests(unittest.TestCase):
    def test_native_policy_is_correct_without_approval_requirement(self):
        self.assertEqual(classify(live_policy(), [], POLICY)[0], "correct")
        self.assertEqual(normalize(live_policy())["required_pull_request_reviews"]["required_approving_review_count"], 0)

    def test_github_omits_unrestricted_field_on_readback(self):
        current = live_policy()
        current.pop("restrictions")
        self.assertEqual(classify(current, [], POLICY)[0], "correct")
        current["future_control"] = None
        state, detail = classify(current, [], POLICY)
        self.assertEqual(state, "conflicting")
        self.assertIn("future_control", detail)

    def test_missing_is_distinct_from_conflicting(self):
        self.assertEqual(classify(None, [], POLICY)[0], "missing")
        self.assertEqual(classify(None, [{"type": "pull_request"}], POLICY)[0], "conflicting")

    def test_stronger_and_unrecognized_controls_are_never_overwritten(self):
        changes = [
            {"required_signatures": {"enabled": True}},
            {"restrictions": {"users": [{"login": "maintainer"}], "teams": [], "apps": []}},
            {"future_protection": True},
            {"required_pull_request_reviews": dict(POLICY["required_pull_request_reviews"], required_approving_review_count=1)},
            {"required_pull_request_reviews": dict(POLICY["required_pull_request_reviews"], bypass_pull_request_allowances={"users": [{"login": "admin"}]})},
        ]
        for change in changes:
            with self.subTest(change=change):
                self.assertEqual(classify(dict(live_policy(), **change), [], POLICY)[0], "conflicting")

    def test_unbound_or_wrong_app_checks_conflict(self):
        for app_id in [-1, None, 42]:
            current = live_policy()
            current["required_status_checks"]["checks"][0]["app_id"] = app_id
            self.assertEqual(classify(current, [], POLICY)[0], "conflicting")

    def test_only_explicit_unprotected_404_means_missing(self):
        for status, message in [("403", "Forbidden"), ("404", "Not Found"), ("500", "Internal Error")]:
            with self.subTest(status=status), patch("branch_protection.api", side_effect=[{}, {}, [], [], Inaccessible(status, message)]):
                with self.assertRaises(Inaccessible):
                    inspect("owner/repo", "main", POLICY)
        with patch("branch_protection.api", side_effect=[{}, {}, [], [], Inaccessible("404", "Branch not protected")]):
            self.assertEqual(inspect("owner/repo", "main", POLICY)[1][0], "missing")

    def test_rules_read_failure_is_not_unprotected(self):
        with patch("branch_protection.api", side_effect=[{}, {}, Inaccessible("403", "Forbidden")]):
            with self.assertRaises(Inaccessible):
                inspect("owner/repo", "main", POLICY)

    def test_apply_is_idempotent_and_never_mutates_existing_conflicts(self):
        for state, expected in [("correct", 0), ("conflicting", 1)]:
            with self.subTest(state=state), patch("sys.argv", ["branch_protection.py", "apply"]), \
                    patch("branch_protection.inspect", return_value=("endpoint", (state, "evidence"))), \
                    patch("branch_protection.api") as call, patch("builtins.print"):
                self.assertEqual(main(), expected)
                call.assert_not_called()

    def test_only_successful_observed_contexts_from_expected_app_allow_apply(self):
        checks = [{"name": row["context"], "app": {"id": row["app_id"]},
                   "status": "completed", "conclusion": "success"}
                  for row in POLICY["required_status_checks"]["checks"]]
        verify_checks(checks, POLICY)
        for field, value in [("conclusion", "failure"), ("conclusion", "cancelled"),
                             ("conclusion", "skipped"), ("status", "in_progress"),
                             ("app", {"id": -1})]:
            bad = copy.deepcopy(checks)
            bad[0][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                verify_checks(bad, POLICY)
        with self.assertRaises(ValueError):
            verify_checks(checks[1:], POLICY)

    def test_initial_apply_checks_then_writes_and_verifies_readback(self):
        checks = [{"name": row["context"], "app": {"id": row["app_id"]},
                   "status": "completed", "conclusion": "success"}
                  for row in POLICY["required_status_checks"]["checks"]]
        events = []

        def inspect_state(*args):
            events.append("inspect")
            state = "correct" if events.count("inspect") == 3 else "missing"
            return "endpoint", (state, "evidence")

        def request(endpoint, payload=None):
            events.append("PUT" if payload is not None else "checks")
            if payload is not None:
                self.assertEqual(endpoint, "endpoint")
                self.assertEqual(payload, POLICY)
                return live_policy()
            self.assertIn("/commits/" + "a" * 40 + "/check-runs?", endpoint)
            return {"total_count": len(checks), "check_runs": checks}

        with patch("sys.argv", ["branch_protection.py", "apply", "--head", "a" * 40]), \
                patch("branch_protection.inspect", side_effect=inspect_state), \
                patch("branch_protection.api", side_effect=request), patch("builtins.print"):
            self.assertEqual(main(), 0)
        self.assertEqual(events, ["inspect", "checks", "inspect", "PUT", "inspect"])

    def test_policy_appearing_before_apply_is_preserved(self):
        for fresh_state in ["correct", "conflicting"]:
            with self.subTest(fresh_state=fresh_state), \
                    patch("sys.argv", ["branch_protection.py", "apply", "--head", "a" * 40]), \
                    patch("branch_protection.inspect", side_effect=[
                        ("endpoint", ("missing", "evidence")),
                        ("endpoint", (fresh_state, "evidence"))]), \
                    patch("branch_protection.api", return_value={"total_count": 0, "check_runs": []}) as call, \
                    patch("branch_protection.verify_checks"), patch("builtins.print"):
                with self.assertRaisesRegex(ValueError, "protection changed"):
                    main()
                self.assertEqual(call.call_count, 1)
                self.assertEqual(len(call.call_args.args), 1)

    def test_mismatching_readback_fails_after_apply(self):
        with patch("sys.argv", ["branch_protection.py", "apply", "--head", "a" * 40]), \
                patch("branch_protection.inspect", side_effect=[
                    ("endpoint", ("missing", "evidence")),
                    ("endpoint", ("missing", "evidence")),
                    ("endpoint", ("conflicting", "readback mismatch"))]), \
                patch("branch_protection.api", return_value={"total_count": 0, "check_runs": []}) as call, \
                patch("branch_protection.verify_checks"), patch("builtins.print"):
            self.assertEqual(main(), 1)
            self.assertEqual(call.call_count, 2)
            self.assertEqual(call.call_args.args, ("endpoint", POLICY))


if __name__ == "__main__":
    unittest.main()
