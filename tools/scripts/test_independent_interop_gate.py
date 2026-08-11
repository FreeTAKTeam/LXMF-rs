#!/usr/bin/env python3

import unittest

from independent_interop_gate import RNS_RS_ALLOWED, REQUIRED_RNS_RS, validate


class IndependentInteropGateTests(unittest.TestCase):
    def test_exact_peer_divergences_are_allowed(self) -> None:
        rows = [
            {
                "scenario": scenario,
                "direction": direction,
                "status": status,
                "classification": classification,
                "failure_owner": "rns-rs",
            }
            for scenario, direction, status, classification in RNS_RS_ALLOWED
        ]
        rows.extend({"scenario": name, "status": "PASS"} for name in REQUIRED_RNS_RS)
        self.assertEqual(validate({"peer": {"implementation": "rns-rs"}, "scenarios": rows}), [])

    def test_new_failure_is_rejected(self) -> None:
        report = {
            "peer": {"implementation": "rns-rs"},
            "scenarios": [{"scenario": "new failure", "status": "FAIL"}],
        }
        self.assertTrue(validate(report))

    def test_peer_improvement_does_not_require_divergence_rows(self) -> None:
        rows = [{"scenario": name, "status": "PASS"} for name in REQUIRED_RNS_RS]
        self.assertEqual(validate({"peer": {"implementation": "rns-rs"}, "scenarios": rows}), [])


if __name__ == "__main__":
    unittest.main()
