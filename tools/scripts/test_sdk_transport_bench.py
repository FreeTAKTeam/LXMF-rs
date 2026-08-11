#!/usr/bin/env python3
"""Tests for the SDK transport benchmark aggregation schema."""

from __future__ import annotations

import argparse
import unittest

from sdk_transport_bench import comparison_row, measured_variation


def sample(value: float) -> dict[str, float | int | str]:
    return {
        "p50_ns": value,
        "p95_ns": value * 1.5,
        "batch_size": 100,
        "timed_boundary": "per-call latency normalized from fixed-size in-process batches",
    }


class SdkTransportBenchTests(unittest.TestCase):
    def setUp(self) -> None:
        self.args = argparse.Namespace(runs=3, iterations=10)

    def test_common_operation_records_in_process_measurement(self) -> None:
        values = {
            ("snapshot", transport): [sample(100.0), sample(101.0), sample(102.0)]
            for transport in ("in_process", "zmq", "http", "unix")
        }

        row = comparison_row("snapshot", self.args, values)

        self.assertEqual(row["in_process_status"], "measured")
        self.assertEqual(row["in_process_p50_ns"], 101.0)
        self.assertEqual(row["in_process_iterations_per_run"], 1_000)
        self.assertEqual(row["in_process_batch_size"], 100)
        self.assertIn("in_process", row["raw_runs"])
        self.assertEqual(len(measured_variation(row)), 4)

    def test_unsupported_in_process_operation_is_explicit(self) -> None:
        values = {
            ("operation_registry", transport): [sample(100.0), sample(101.0), sample(102.0)]
            for transport in ("zmq", "http", "unix")
        }

        row = comparison_row("operation_registry", self.args, values)

        self.assertEqual(row["in_process_status"], "not_supported")
        self.assertIn("operation registry", row["in_process_reason"])
        self.assertEqual(len(measured_variation(row)), 3)


if __name__ == "__main__":
    unittest.main()
