#!/usr/bin/env python3
"""Unit tests for the measurement-free public dashboard renderer."""

from __future__ import annotations

import unittest
from pathlib import Path

from performance_dashboard import PUBLIC_ROWS, fallback_matrix, render_dashboard


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
        self.assertEqual(rows[2]["cells"]["python"]["p99"], 0.003)
        self.assertEqual(rows[4]["cells"]["python"]["status"], "not_available")
        self.assertIn("1 MB Resource", rows[4]["cells"]["python"]["reason"])
        self.assertIn("rns-rs adapter", rows[0]["cells"]["rns_rs"]["reason"])
        self.assertEqual(rows[8]["cells"]["rns_rs"]["status"], "not_available")

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
