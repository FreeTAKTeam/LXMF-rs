"""Selection regressions: uncertainty must never reduce required coverage."""
import copy
import unittest

from ci_selection import JOB_IDS, parse_changes, select


def package(name, dependencies=(), tests=()):
    return {
        "id": name, "name": name, "manifest_path": f"/repo/crates/{name}/Cargo.toml",
        "dependencies": [
            {"name": dep, "rename": "alias", "path": f"/repo/crates/{dep}",
             "kind": kind, "optional": True, "target": "cfg(windows)"}
            for dep, kind in dependencies
        ],
        "targets": [{"name": test, "kind": ["test"]} for test in tests],
    }


def metadata():
    packages = [
        package("core", tests=["wire"]),
        package("transport", [("core", None)], ["carrier"]),
        package("lxmf-sdk", [("transport", "build")], ["client"]),
        package("cli", [("lxmf-sdk", "dev")], ["process"]),
        package("other", tests=["isolated"]), package("test-support"),
    ]
    return {"workspace_root": "/repo", "workspace_members": [p["id"] for p in packages],
            "packages": packages}


class SelectionTests(unittest.TestCase):
    def choose(self, *paths, **kwargs):
        return select(metadata(), [("M", path) for path in paths], **kwargs)

    def test_core_selects_transitive_consumers_of_all_dependency_kinds(self):
        result = self.choose("crates/core/src/lib.rs")
        self.assertEqual(result["packages"], ["cli", "core", "lxmf-sdk", "transport"])
        self.assertEqual([row["package"] for row in result["integration"]], result["packages"])
        self.assertTrue(result["jobs"]["peer-lifecycle-essential"])

    def test_sdk_and_cli_select_only_their_consumers(self):
        self.assertEqual(self.choose("crates/lxmf-sdk/src/lib.rs")["packages"], ["cli", "lxmf-sdk"])
        self.assertEqual(self.choose("crates/cli/src/main.rs")["packages"], ["cli"])

    def test_uncertain_paths_broaden(self):
        for path in ["Cargo.lock", "crates/core/Cargo.toml", ".github/workflows/ci.yml",
                     ".cargo/config.toml", "tests/fixtures/frame.bin", "vendor/zeromq/src/lib.rs",
                     "crates/test-support/src/lib.rs", "unknown.py",
                     "xtask/src/main_parts/run_ci_stage.rs",
                     "docs/goals/reticulum-reference-parity-605/GOAL.md",
                     "docs/contracts/wire.md", "docs/status/python-surface-mapping.json"]:
            with self.subTest(path=path):
                self.assertEqual(self.choose(path)["mode"], "broad")
                self.assertTrue(all(self.choose(path)["jobs"].values()))

    def test_only_recognized_prose_gets_fast_path(self):
        result = self.choose("README.md", "docs/goals/open-issues-2026-10/PLAN.md")
        self.assertEqual(result["mode"], "docs")
        self.assertEqual({key for key, value in result["jobs"].items() if value}, {"security", "contracts"})
        self.assertEqual(result["integration"], [])
        self.assertEqual(self.choose("docs/goals/example/fixture.json")["mode"], "broad")

    def test_missing_empty_and_invalid_diff_broaden(self):
        for changes in [None, [], [("M", "../outside")], [("Q", "README.md")]]:
            self.assertEqual(select(metadata(), changes)["mode"], "broad")

    def test_missing_or_ambiguous_metadata_uses_workspace_fallback(self):
        bad = metadata()
        bad["packages"].append(copy.deepcopy(bad["packages"][0]))
        for value in [None, {}, bad]:
            result = select(value, [("M", "crates/core/src/lib.rs")])
            self.assertTrue(result["workspace_tests"])
            self.assertTrue(result["jobs"]["test-integration"])

    def test_rename_and_delete_preserve_both_owners(self):
        changes = parse_changes(b"R100\0crates/core/src/old.rs\0crates/other/src/new.rs\0D\0crates/cli/src/old.rs\0")
        result = select(metadata(), changes)
        self.assertEqual(result["packages"], ["cli", "core", "lxmf-sdk", "other", "transport"])
        self.assertEqual(select(metadata(), [("D", "crates/deleted/src/lib.rs")])["mode"], "broad")

    def test_nul_parser_retains_unusual_filenames_and_rejects_truncation(self):
        self.assertEqual(parse_changes(b"M\0a\nb\0"), [("M", "a\nb")])
        for data in [b"M\0path", b"R100\0old\0", b"Q\0path\0", b"M\0\xff\0"]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                parse_changes(data)

    def test_labels_only_widen_and_schedule_only_runs_security(self):
        self.assertEqual(self.choose("README.md", labels=["run-full-CI"])["mode"], "broad")
        self.assertEqual(self.choose("README.md", labels=["run-peer-lifecycle-CI"])["mode"], "broad")
        result = select(None, None, event="schedule")
        self.assertEqual({k for k, v in result["jobs"].items() if v}, {"security"})
        self.assertEqual(select(metadata(), None, event="workflow_dispatch")["mode"], "broad")

    def test_selection_is_deterministic(self):
        first = self.choose("crates/other/src/lib.rs", "crates/core/src/lib.rs")
        second = self.choose("crates/core/src/lib.rs", "crates/other/src/lib.rs")
        self.assertEqual(first, second)
        self.assertEqual(set(first["jobs"]), set(JOB_IDS))


if __name__ == "__main__":
    unittest.main()
