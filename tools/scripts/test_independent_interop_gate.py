#!/usr/bin/env python3

import unittest

from independent_interop_gate import (
    RETICULUM_GO_ALLOWED_UNSUPPORTED,
    RETICULUM_GO_REQUIRED_PASS,
    RNS_RS_ALLOWED,
    RNS_RS_REQUIRED_EXPANDED_PASS,
    RNS_RS_REQUIRED_PRESENT,
    RNS_RS_REQUIRED_PR_PASS,
    validate,
)


def row(signature: tuple[str, str, str], status: str = "PASS") -> dict:
    topology, direction, scenario = signature
    return {
        "topology": topology,
        "direction": direction,
        "scenario": scenario,
        "status": status,
    }


def required_rns_rows(level: str = "pr") -> list[dict]:
    required = set(RNS_RS_REQUIRED_PR_PASS)
    if level != "pr":
        required.update(RNS_RS_REQUIRED_EXPANDED_PASS)
    return [row(item) for item in required]


class IndependentInteropGateTests(unittest.TestCase):
    def test_exact_peer_divergences_are_allowed(self) -> None:
        rows = [
            {
                "scenario": scenario,
                "direction": direction,
                "status": status,
                "classification": classification,
                "failure_owner": "rns-rs",
                "normative_reference": (
                    "Python RNS 1.4.2 confirms the peer divergence"
                    if classification == "peer_divergence"
                    else None
                ),
                "topology": "two-node",
            }
            for scenario, direction, status, classification in RNS_RS_ALLOWED
        ]
        rows.extend(required_rns_rows())
        self.assertEqual(
            validate({"level": "pr", "peer": {"implementation": "rns-rs"}, "scenarios": rows}),
            [],
        )

    def test_new_failure_is_rejected(self) -> None:
        report = {
            "level": "pr",
            "peer": {"implementation": "rns-rs"},
            "scenarios": [{"scenario": "new failure", "status": "FAIL"}],
        }
        self.assertTrue(validate(report))

    def test_peer_improvement_does_not_require_divergence_rows(self) -> None:
        rows = required_rns_rows()
        rows.extend(row(item) for item in RNS_RS_REQUIRED_PRESENT)
        self.assertEqual(
            validate({"level": "pr", "peer": {"implementation": "rns-rs"}, "scenarios": rows}),
            [],
        )

    def test_peer_divergence_requires_normative_reference(self) -> None:
        rows = required_rns_rows()
        rows.extend(row(item) for item in RNS_RS_REQUIRED_PRESENT)
        scenario, direction, status, classification = next(
            item for item in RNS_RS_ALLOWED if item[3] == "peer_divergence"
        )
        rows.append(
            {
                "topology": "two-node",
                "scenario": scenario,
                "direction": direction,
                "status": status,
                "classification": classification,
                "failure_owner": "rns-rs",
            }
        )
        self.assertTrue(
            any(
                "lacks Python RNS 1.4.2 evidence" in error
                for error in validate(
                    {"level": "pr", "peer": {"implementation": "rns-rs"}, "scenarios": rows}
                )
            )
        )

    def test_release_requires_expanded_resource_and_chaos_rows(self) -> None:
        errors = validate(
            {
                "level": "release",
                "peer": {"implementation": "rns-rs"},
                "scenarios": required_rns_rows("pr")
                + [row(item) for item in RNS_RS_REQUIRED_PRESENT],
            }
        )
        self.assertTrue(any("Resource 50 MiB" in error for error in errors))
        self.assertTrue(any("500 ms" in error for error in errors))

    def test_reticulum_go_allows_only_named_unsupported_rows(self) -> None:
        rows = [row(item) for item in RETICULUM_GO_REQUIRED_PASS]
        for scenario, direction, status, classification in RETICULUM_GO_ALLOWED_UNSUPPORTED:
            rows.append(
                {
                    "topology": "two-node",
                    "scenario": scenario,
                    "direction": direction,
                    "status": status,
                    "classification": classification,
                    "failure_owner": "Reticulum-Go",
                    "failure_reason": "confirmed peer control API limit",
                }
            )
        self.assertEqual(
            validate(
                {
                    "level": "release",
                    "peer": {"implementation": "Reticulum-Go"},
                    "scenarios": rows,
                }
            ),
            [],
        )


if __name__ == "__main__":
    unittest.main()
