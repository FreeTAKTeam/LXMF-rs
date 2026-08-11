#!/usr/bin/env python3
"""Unit tests for the measurement-free public dashboard renderer."""

from __future__ import annotations

import unittest
from pathlib import Path

from performance_dashboard import PUBLIC_ROWS, fallback_matrix, format_value, render_dashboard


ROOT = Path(__file__).resolve().parents[2]


class PerformanceDashboardTests(unittest.TestCase):
    def test_fallback_keeps_requested_matrix_and_marks_exact_gaps(self) -> None:
        data = {
            "environment": {"git_commit": "abc123", "timestamp_utc": "now"},
            "profile": "report",
            "comparisons": [
                {
                    "label": "Reticulum packet pack",
                    "rust": {"throughput_ops_per_sec": 20.0, "p50_ns": 50.0},
                    "python": {"throughput_ops_per_sec": 10.0, "p50_ns": 100.0},
                },
                {
                    "label": "Reticulum announce validate",
                    "rust": {"throughput_ops_per_sec": 4.0, "p50_ns": 250.0},
                    "python": {"throughput_ops_per_sec": 2.0, "p50_ns": 500.0},
                },
            ],
            "e2e_comparisons": [
                {
                    "label": "Loopback TCP cold destination discovery",
                    "topology": "fixed",
                    "timed_boundary": "cold_path_request_to_route_available",
                    "rust": {"p50_ns": 1_000_000.0, "p99_ns": 2_000_000.0},
                    "python": {"p50_ns": 2_000_000.0, "p99_ns": 3_000_000.0},
                }
            ],
        }
        rows = fallback_matrix(data)

        self.assertEqual([row["id"] for row in rows], [row[0] for row in PUBLIC_ROWS])
        self.assertEqual(rows[0]["cells"]["lxmf_rs"]["value"], 20.0)
        indexed = {row["id"]: row for row in rows}
        self.assertEqual(indexed["path_convergence_cold"]["cells"]["python"]["p99"], 0.003)
        self.assertEqual(indexed["resource_1mib"]["cells"]["python"]["status"], "not_available")
        self.assertIn("1 MiB Resource", indexed["resource_1mib"]["cells"]["python"]["reason"])
        self.assertIn("No exact rns-rs", rows[0]["cells"]["rns_rs"]["reason"])
        self.assertEqual(indexed["active_links_1000"]["cells"]["rns_rs"]["status"], "not_available")

    def test_independent_measurements_overlay_only_exact_cells(self) -> None:
        data = {
            "independent_performance": {
                "public_cells": {
                    "resource_1mib": {
                        "rns_rs": {"status": "measured", "value": 4.5},
                        "lxmf_rs": {"status": "measured", "value": 5.5},
                    },
                    "packet_encode": {
                        "rns_rs": {"status": "not_supported", "reason": "no isolated API"}
                    },
                }
            }
        }

        indexed = {row["id"]: row for row in fallback_matrix(data)}

        self.assertEqual(indexed["resource_1mib"]["cells"]["rns_rs"]["value"], 4.5)
        self.assertEqual(indexed["packet_encode"]["cells"]["rns_rs"]["status"], "not_supported")
        self.assertEqual(indexed["resource_50mib"]["cells"]["rns_rs"]["status"], "not_available")

    def test_non_measurement_statuses_remain_explicit(self) -> None:
        self.assertEqual(format_value({"status": "not_available"}, "s"), "N/A")
        self.assertEqual(format_value({"status": "not_supported"}, "s"), "UNSUPPORTED")
        self.assertEqual(format_value({"status": "failed"}, "s"), "FAILED")

    def test_render_is_standalone_and_escapes_metadata(self) -> None:
        data = {
            "environment": {"git_commit": "<commit>", "timestamp_utc": "now"},
            "profile": "report",
            "public_benchmark": {
                "rows": [
                    {
                        "id": "packet_encode",
                        "label": "Packet <encode>",
                        "unit": "ops/s",
                        "cells": {
                            "python": {"status": "measured", "value": 1.0},
                            "lxmf_rs": {"status": "not_available", "reason": "missing"},
                            "rns_rs": {"status": "not_available", "reason": "missing"},
                        },
                    }
                ]
            },
        }
        rendered = render_dashboard(data, "v<test>")

        self.assertIn("Packet &lt;encode&gt;", rendered)
        self.assertIn("&lt;commit&gt;", rendered)
        self.assertNotIn("https://", rendered)
        self.assertIn('id="benchmark-data"', rendered)

    def test_performance_workflows_are_opt_in(self) -> None:
        release = (ROOT / ".github/workflows/performance-release.yml").read_text(encoding="utf-8")
        request = (ROOT / ".github/workflows/performance-smoke.yml").read_text(encoding="utf-8")

        for workflow in (release, request):
            self.assertNotIn("pull_request:", workflow)
            self.assertNotIn("schedule:", workflow)
        self.assertIn("workflow_dispatch:", release)
        self.assertIn("workflow_dispatch:", request)
        self.assertIn("tags:", release)


if __name__ == "__main__":
    unittest.main()
