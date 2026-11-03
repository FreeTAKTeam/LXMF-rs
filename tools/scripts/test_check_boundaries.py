#!/usr/bin/env python3
"""Seed violations through the same Cargo metadata gate used in CI."""
import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT, text=True,
        ))

    def check_seed(self, package, dependency, message):
        metadata = copy.deepcopy(self.metadata)
        next(p for p in metadata["packages"] if p["name"] == package)["dependencies"].append(dependency)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "metadata.json"
            path.write_text(json.dumps(metadata))
            env = dict(os.environ, BOUNDARY_METADATA_FILE=str(path),
                       ARCH_REPORT_PATH=str(Path(temporary) / "report.txt"))
            result = subprocess.run(["bash", "tools/scripts/check-boundaries.sh"],
                                    cwd=ROOT, env=env, text=True, capture_output=True)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(message, result.stderr)

    def test_current_graph_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = subprocess.run(["bash", "tools/scripts/check-boundaries.sh"], cwd=ROOT,
                                    env=dict(os.environ, ARCH_REPORT_PATH=str(Path(temporary) / "report.txt")),
                                    text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_optional_runtime_dependency_is_rejected(self):
        self.check_seed("rns-embedded-runtime", {"name": "tokio", "optional": True, "kind": None},
                        "package 'rns-embedded-runtime' depends on forbidden crate 'tokio'")

    def test_build_dependency_is_rejected(self):
        self.check_seed("rns-embedded-core", {"name": "axum", "optional": False, "kind": "build"},
                        "package 'rns-embedded-core' depends on forbidden crate 'axum'")

    def test_production_json_is_rejected(self):
        self.check_seed("rns-embedded-core", {"name": "serde_json", "optional": True, "kind": None},
                        "package 'rns-embedded-core' depends on forbidden crate 'serde_json'")

    def test_target_runtime_dependency_is_rejected(self):
        self.check_seed("rns-embedded-runtime", {"name": "axum", "optional": False, "kind": None,
                        "target": "cfg(windows)"},
                        "package 'rns-embedded-runtime' depends on forbidden crate 'axum'")

    def test_dev_runtime_dependency_is_rejected(self):
        self.check_seed("lxmf-embedded-mini", {"name": "ureq", "optional": False, "kind": "dev"},
                        "package 'lxmf-embedded-mini' depends on forbidden crate 'ureq'")

    def test_unlisted_workspace_edge_is_rejected(self):
        self.check_seed("rns-embedded-runtime", {"name": "lxmf-wire", "optional": False, "kind": None,
                        "path": str(ROOT / "crates/libs/lxmf-core")},
                        "library dependency edges diverged from workspace allowlist")

    def test_mininode_does_not_unify_core_std(self):
        tree = subprocess.check_output([
            "cargo", "tree", "--locked", "-p", "rns-embedded-mininode",
            "--no-default-features", "-e", "features", "--prefix", "none",
        ], cwd=ROOT, text=True)
        self.assertNotIn('reticulum-rs-core feature "std"', tree)


if __name__ == "__main__":
    unittest.main()
