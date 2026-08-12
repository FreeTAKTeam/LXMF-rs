#!/usr/bin/env python3

import json
import unittest

from independent_evidence_publish import build_bundle, render_html, render_markdown


class IndependentEvidencePublishTests(unittest.TestCase):
    def setUp(self) -> None:
        self.rns = {
            "artifact_root": "/home/runner/work/repo/target/independent-rns-rs",
            "peer": {"revision": "rns-pin"},
            "summary": {"counts": {"PASS": 1, "FAIL": 1}},
            "scenarios": [
                {
                    "topology": "two-node",
                    "scenario": "packet",
                    "direction": "LXMF-rs -> rns-rs",
                    "status": "PASS",
                    "runtime_seconds": 0.25,
                },
                {
                    "scenario": "known gap",
                    "status": "FAIL",
                    "classification": "peer_divergence",
                },
            ],
        }
        self.retgo = {
            "artifact_root": "/home/runner/work/repo/target/independent-reticulum-go",
            "peer": {"revision": "go-pin"},
            "summary": {"counts": {"PASS": 1, "UNSUPPORTED": 1}},
            "scenarios": [{"scenario": "resource", "status": "PASS"}],
        }
        self.parity = {
            "summary": {"total": 3, "complete": 2, "not-applicable": 1},
            "items": [
                {"implementation": "complete"},
                {"implementation": "complete"},
                {"implementation": "not-applicable"},
            ],
        }

    def test_bundle_preserves_pins_and_divergences(self) -> None:
        bundle = build_bundle("v-test", self.rns, self.retgo, None, self.parity)
        self.assertEqual(bundle["interop"]["rns-rs"]["peer"]["revision"], "rns-pin")
        self.assertNotIn("artifact_root", bundle["interop"]["rns-rs"])
        self.assertNotIn("artifact_root", bundle["interop"]["Reticulum-Go"])
        self.assertIn("artifact_root", self.rns)
        self.assertEqual(len(bundle["known_peer_divergences"]), 1)
        self.assertEqual(bundle["readiness_axes"]["performance_evidence"]["status"], "NOT_RUN")
        self.assertEqual(
            bundle["readiness_axes"]["rns_1_4_2_software_parity"]["applicable"], 2
        )

    def test_renderers_embed_results_and_escape_html(self) -> None:
        self.rns["scenarios"][0]["scenario"] = "packet <proof>"
        bundle = build_bundle("v-test", self.rns, self.retgo, None, self.parity)
        markdown = render_markdown(bundle)
        page = render_html(bundle)
        self.assertIn("rns-pin", markdown)
        self.assertIn("packet &lt;proof&gt;", page)
        self.assertIn("Readiness axes", page)
        embedded = page.split('<script id="independent-evidence" type="application/json">', 1)[1]
        json.loads(embedded.split("</script>", 1)[0])


if __name__ == "__main__":
    unittest.main()
