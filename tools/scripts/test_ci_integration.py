import unittest

from ci_integration import commands
from ci_selection import select
from test_ci_selection import metadata


class IntegrationTests(unittest.TestCase):
    def test_selects_only_named_integration_targets(self):
        selection = select(metadata(), [("M", "crates/cli/src/main.rs")])
        self.assertEqual(commands(selection), [["cargo", "test", "--locked", "--package", "cli",
                                               "--test", "process", "--", "--test-threads=4"]])

    def test_metadata_failure_runs_all_workspace_tests(self):
        self.assertEqual(commands(select(None, None)), [["cargo", "test", "--locked", "--workspace",
                                                        "--tests", "--", "--test-threads=4"]])

    def test_required_target_features_are_enabled(self):
        value = metadata()
        value["packages"][3]["targets"][0]["required-features"] = ["test-feature"]
        selection = select(value, [("M", "crates/cli/src/main.rs")])
        self.assertIn("test-feature", commands(selection)[0])

    def test_inapplicable_or_malformed_input_cannot_pass(self):
        for selection in [None, {}, select(None, None, event="schedule")]:
            with self.assertRaises(ValueError):
                commands(selection)


if __name__ == "__main__":
    unittest.main()
