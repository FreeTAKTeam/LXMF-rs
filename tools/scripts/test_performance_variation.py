#!/usr/bin/env python3
"""Tests for release performance variation classification and gating."""

from __future__ import annotations

import unittest

from performance_release_gate import evaluate
from performance_variation import MIN_RELEASE_SAMPLES, classify_relative_mad


def dataset(relative_mad: float, samples: list[float] | None = None) -> dict:
    run_samples = samples if samples is not None else [100.0, 101.0, 102.0, 101.5, 100.5]
    implementation = {
        "throughput_ops_per_sec": 100.0,
    }
    resources = {
        "median_cpu_seconds_per_1k_ops": 1.0,
        "median_peak_rss_bytes": 1024.0,
    }
    return {
        "profile": "report",
        "environment": {
            "git_commit": "candidate",
            "cpu": "cpu",
            "uname": "kernel",
            "rustc_version": "rustc",
            "python_version": "python",
        },
        "comparisons": [
            {
                "label": "packet decode",
                "rust": implementation,
                "python": implementation,
                "rust_resources": resources,
                "python_resources": resources,
                "dispersion": {
                    name: {
                        "p50_relative_mad": relative_mad,
                        "run_p50_ns": run_samples,
                    }
                    for name in ("rust", "python")
                },
            }
        ],
        "e2e_comparisons": [],
        "sdk_transport_comparisons": [],
    }


class PerformanceVariationTests(unittest.TestCase):
    def test_release_dispersion_requires_five_samples(self) -> None:
        self.assertEqual(MIN_RELEASE_SAMPLES, 5)

    def test_classification_has_distinct_normal_warning_and_hard_bands(self) -> None:
        self.assertEqual(classify_relative_mad(0.10), "normal")
        self.assertEqual(classify_relative_mad(0.1013), "warning")
        self.assertEqual(classify_relative_mad(0.20), "warning")
        self.assertEqual(classify_relative_mad(0.2001), "hard_failure")

    def test_warning_band_does_not_fail_release_gate(self) -> None:
        candidate = dataset(0.1013)
        baseline = dataset(0.01)
        baseline["environment"]["git_commit"] = "baseline"

        report = evaluate(candidate, baseline)

        self.assertEqual(report["status"], "pass_with_warnings")
        self.assertEqual(report["failures"], [])
        self.assertEqual(len(report["warnings"]), 2)

    def test_hard_variation_and_four_samples_fail(self) -> None:
        candidate = dataset(0.21, [100.0, 120.0, 101.0, 99.0])
        baseline = dataset(0.01)
        baseline["environment"]["git_commit"] = "baseline"

        report = evaluate(candidate, baseline)

        self.assertEqual(report["status"], "fail")
        self.assertTrue(any("sample count" in failure for failure in report["failures"]))
        self.assertTrue(any("> 20%" in failure for failure in report["failures"]))

    def test_independent_warning_is_reported_without_failing(self) -> None:
        candidate = dataset(0.01)
        candidate["independent_performance"] = {
            "path_convergence": {
                "cold": {
                    "samples_seconds": [1.0, 1.1, 1.2, 1.05, 1.15],
                    "p50_relative_mad": 0.1013,
                }
            },
            "resources": {},
        }
        baseline = dataset(0.01)
        baseline["environment"]["git_commit"] = "baseline"

        report = evaluate(candidate, baseline)

        self.assertEqual(report["status"], "pass_with_warnings")
        self.assertTrue(any("independent dispersion path cold" in row for row in report["warnings"]))


if __name__ == "__main__":
    unittest.main()
