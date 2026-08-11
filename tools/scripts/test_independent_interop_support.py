#!/usr/bin/env python3
"""Tests for independent interoperability evidence status semantics."""

from __future__ import annotations

import unittest

from independent_interop_support import Evidence, render_markdown


class EvidenceTests(unittest.TestCase):
    def test_protocol_failure_records_owner_and_classification(self) -> None:
        evidence = Evidence({})

        def fail() -> None:
            raise AssertionError("wire mismatch")

        evidence.run(
            "packet",
            "left -> right",
            fail,
            failure_owner="peer",
            classification="peer_divergence",
            normative_reference="Python control PASS",
        )

        row = evidence.report()["scenarios"][0]
        self.assertEqual(row["status"], "FAIL")
        self.assertEqual(row["failure_owner"], "peer")
        self.assertEqual(row["classification"], "peer_divergence")
        self.assertEqual(row["normative_reference"], "Python control PASS")

    def test_blocked_is_not_reported_as_protocol_failure(self) -> None:
        evidence = Evidence({})
        evidence.record(
            "resource",
            "left -> right",
            "BLOCKED",
            "peer failed to build",
            classification="external_dependency",
        )

        report = evidence.report()
        self.assertEqual(report["summary"]["status"], "BLOCKED")
        self.assertEqual(report["summary"]["counts"], {"BLOCKED": 1})

    def test_unsupported_does_not_override_passing_supported_subset(self) -> None:
        metadata = {
            "peer": {"implementation": "peer", "version": "1", "revision": "abc"},
            "lxmf_rs": {"revision": "def"},
            "rns_reference": {"version": "1.4.2", "revision": "ghi"},
        }
        evidence = Evidence(metadata)
        evidence.run("announce", "bidirectional", lambda: {"verified": True})
        evidence.record(
            "channel",
            "bidirectional",
            "UNSUPPORTED",
            "peer surface unavailable",
            classification="peer_surface_unavailable",
        )

        report = evidence.report()
        self.assertEqual(report["summary"]["status"], "PASS")
        markdown = render_markdown(report)
        self.assertIn("UNSUPPORTED", markdown)
        self.assertIn("peer_surface_unavailable", markdown)


if __name__ == "__main__":
    unittest.main()
