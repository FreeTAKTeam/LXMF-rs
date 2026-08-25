#!/usr/bin/env python3

import unittest

from performance_docs import DEFAULT_RELEASE, make_environment_paths_portable


class PerformanceDocsTests(unittest.TestCase):
    def test_default_release_tracks_current_published_dataset(self) -> None:
        self.assertEqual(DEFAULT_RELEASE, "v0.10.0")

    def test_environment_reference_paths_are_repository_relative(self) -> None:
        data = {
            "environment": {
                "python_rns_module": "/home/runner/work/repo/refs/Reticulum/RNS/__init__.py",
                "python_lxmf_module": "/tmp/repo/refs/LXMF/LXMF/__init__.py",
            }
        }

        make_environment_paths_portable(data)

        self.assertEqual(
            data["environment"]["python_rns_module"],
            "refs/Reticulum/RNS/__init__.py",
        )
        self.assertEqual(
            data["environment"]["python_lxmf_module"],
            "refs/LXMF/LXMF/__init__.py",
        )


if __name__ == "__main__":
    unittest.main()
