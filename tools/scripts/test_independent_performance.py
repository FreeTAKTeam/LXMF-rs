#!/usr/bin/env python3
"""Unit tests for independent performance aggregation and public cells."""

from __future__ import annotations

import unittest

from independent_performance import aggregate_resources, public_cells


class IndependentPerformanceTests(unittest.TestCase):
    def test_resource_aggregate_keeps_exact_hash_cpu_rss_and_samples(self) -> None:
        rows = [
            {
                "seconds": value,
                "bytes": 1_048_576,
                "sha256": "ab" * 32,
                "cpu_seconds": 0.05,
                "peer_cpu_seconds": 0.04,
                "peak_rss_bytes": 20 * 1_048_576,
                "peer_peak_rss_bytes": 18 * 1_048_576,
            }
            for value in (0.2, 0.21, 0.19)
        ]

        result = aggregate_resources(rows, 1_048_576)

        self.assertEqual(result["sample_count"], 3)
        self.assertEqual(result["sha256"], "ab" * 32)
        self.assertAlmostEqual(result["throughput_mib_per_second"], 5.0)
        self.assertAlmostEqual(result["cpu_ms_per_mib"], 50.0)
        self.assertAlmostEqual(result["peak_rss_mib"], 20.0)

    def test_public_cells_do_not_invent_isolated_peer_microbenchmarks(self) -> None:
        timing = {
            "sample_count": 3,
            "p50_seconds": 0.1,
            "p95_seconds": 0.2,
            "p99_seconds": 0.2,
            "samples_seconds": [0.1, 0.1, 0.2],
            "p50_relative_mad": 0.0,
            "variation_class": "normal",
        }
        report = {
            "path_convergence": {"cold": timing, "warm": timing},
            "link_setup": timing,
            "resources": {},
            "active_links_1000": {"status": "not_supported", "reason": "bounded peer limit"},
        }

        cells = public_cells(report)

        self.assertEqual(cells["packet_encode"]["rns_rs"]["status"], "not_supported")
        self.assertEqual(cells["active_links_1000"]["rns_rs"]["status"], "not_supported")


if __name__ == "__main__":
    unittest.main()
