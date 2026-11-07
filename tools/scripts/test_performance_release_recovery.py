"""Exercise payload accounting, native proof expectations, and pinned overlays."""

import copy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import e2e_performance
from e2e_performance import WORKLOADS
from performance_fixture import payload_content, validate_opportunistic_sender
from performance_release_recovery import OVERLAY_PATHS, git, prepare


class PayloadTests(unittest.TestCase):
    def test_all_supported_workloads_keep_markers_inside_exact_content_size(self):
        for _, rust_case, python_case, size in WORKLOADS:
            for case in (rust_case, python_case):
                marker = f"smoke-message-{case}-1794012604"
                with self.subTest(case=case, size=size):
                    content = payload_content(marker, size)
                    self.assertTrue(content.startswith(marker))
                    self.assertEqual(len(content.encode()), size or len(marker.encode()))
        self.assertEqual(len(payload_content("é", 8).encode()), 8)

    def test_marker_is_never_truncated_to_satisfy_an_undersized_payload(self):
        with self.assertRaisesRegex(ValueError, "cannot contain the 7-byte smoke marker"):
            payload_content("marker", 6)

    def sender(self):
        packet_hash = "a" * 64
        return {"method": 1, "validated_proofs": [{
            "packet_hash": packet_hash, "proof_destination": packet_hash[:32],
            "proof_data_hex": packet_hash + "b" * 128,
        }]}

    def test_native_opportunistic_proof_passes_and_direct_fallback_is_rejected(self):
        sender = self.sender()
        validate_opportunistic_sender(sender)
        sender["method"] = 2
        with self.assertRaisesRegex(AssertionError, "expected OPPORTUNISTIC"):
            validate_opportunistic_sender(sender)

    def test_packet_hash_and_reverse_route_checks_remain_enforced(self):
        sender = self.sender()
        for field, expected in (("proof_destination", "reverse-routing"), ("proof_data_hex", "proof hash")):
            invalid = copy.deepcopy(sender)
            invalid["validated_proofs"][0][field] = "0" * 128
            with self.subTest(field=field), self.assertRaisesRegex(AssertionError, expected):
                validate_opportunistic_sender(invalid)
        sender["validated_proofs"] = []
        with self.assertRaisesRegex(AssertionError, "no validated packet proof"):
            validate_opportunistic_sender(sender)


class OverlayTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.candidate = Path(self.temporary.name) / "candidate"
        self.harness = Path(self.temporary.name) / "harness"
        for root in (self.candidate, self.harness):
            root.mkdir()
            git(root, "init", "--quiet")
            git(root, "config", "user.name", "Test")
            git(root, "config", "user.email", "test@example.invalid")
            (root / "VERSION").write_text("1.2.3\n")
            (root / "Cargo.toml").write_text(f"{root.name} product must stay untouched\n")
            for name in OVERLAY_PATHS:
                if root == self.candidate and name.endswith("performance_fixture.py"):
                    continue
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(root.name + " " + name + "\n")
            git(root, "add", ".")
            git(root, "commit", "--quiet", "-m", "fixture")
            git(root, "tag", "v1.2.3")
        self.harness_sha = git(self.harness, "rev-parse", "HEAD")

    def apply(self, publish=True):
        return prepare(self.candidate, self.harness, self.harness_sha, "v1.2.3", publish)

    def test_overlay_pins_both_revisions_and_preserves_product_and_tag(self):
        candidate_sha = git(self.candidate, "rev-parse", "HEAD")
        product = (self.candidate / "Cargo.toml").read_bytes()
        provenance = self.apply()
        self.assertEqual(provenance["candidate_sha"], candidate_sha)
        self.assertEqual(provenance["release_tag_sha"], candidate_sha)
        self.assertEqual(provenance["harness_sha"], self.harness_sha)
        self.assertEqual(git(self.candidate, "rev-parse", "HEAD"), candidate_sha)
        self.assertEqual((self.candidate / "Cargo.toml").read_bytes(), product)
        for item in provenance["overlay_files"]:
            actual = (self.candidate / item["path"]).read_bytes()
            self.assertEqual(item["harness_sha256"], hashlib.sha256(actual).hexdigest())
            self.assertEqual(actual, (self.harness / item["path"]).read_bytes())
        saved = self.candidate / "target/performance/lxmf-rs-performance-provenance.json"
        self.assertEqual(json.loads(saved.read_text()), provenance)

    def test_publication_rejects_candidate_that_differs_from_tag(self):
        (self.candidate / "Cargo.toml").write_text("unreleased product\n")
        git(self.candidate, "add", ".")
        git(self.candidate, "commit", "--quiet", "-m", "unreleased")
        with self.assertRaisesRegex(ValueError, "exact release tag commit"):
            self.apply()

    def test_mutable_or_mismatched_harness_ref_is_rejected(self):
        for ref in ("main", "0" * 40):
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                prepare(self.candidate, self.harness, ref, "v1.2.3", True)

    def test_dirty_product_and_uncommitted_harness_are_rejected(self):
        product = self.candidate / "Cargo.toml"
        original = product.read_bytes()
        product.write_text("dirty product\n")
        with self.assertRaisesRegex(ValueError, "tracked files must be clean"):
            self.apply()
        product.write_bytes(original)
        (self.harness / OVERLAY_PATHS[0]).write_text("dirty harness\n")
        with self.assertRaisesRegex(ValueError, "differs from pinned commit"):
            self.apply()


class ResumeTests(unittest.TestCase):
    def report(self, size=256):
        return {"proof": {"smoke_message_content": "x" * size}, "performance": {
            "boundary": "enqueue_to_receiver_evidence", "payload_size_bytes": 256,
            "requested_payload_size_bytes": 256, "enqueue_to_delivery_ns": 100,
            "cpu_seconds": 1.0, "peak_rss_bytes": 1024,
            "startup_included": False, "route_warmup_included": False,
        }}

    def test_stale_or_setup_inclusive_cached_samples_are_measured_again(self):
        old = self.report(310)
        del old["performance"]["requested_payload_size_bytes"]
        setup = self.report()
        setup["performance"]["startup_included"] = True
        for data in (old, self.report(310), setup):
            with self.subTest(data=data), tempfile.TemporaryDirectory() as temporary:
                output = Path(temporary)
                report = output / "raw/opportunistic/python/sample-01/report.json"
                report.parent.mkdir(parents=True)
                report.write_text(json.dumps(data))
                with mock.patch.object(e2e_performance, "RESUME_EXISTING", True), mock.patch(
                    "e2e_performance.subprocess.run", return_value=subprocess.CompletedProcess([], 1, "", "new sample")
                ) as run:
                    with self.assertRaisesRegex(RuntimeError, "sample 1 failed"):
                        e2e_performance.run_sample("opportunistic", "python", "opportunistic_python_to_rust", 256, 1, output)
                    run.assert_called_once()

    def test_valid_exact_size_sample_can_still_be_resumed(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            report = output / "raw/opportunistic/python/sample-01/report.json"
            report.parent.mkdir(parents=True)
            report.write_text(json.dumps(self.report()))
            with mock.patch.object(e2e_performance, "RESUME_EXISTING", True), mock.patch(
                "e2e_performance.subprocess.run"
            ) as run:
                result = e2e_performance.run_sample("opportunistic", "python", "opportunistic_python_to_rust", 256, 1, output)
                self.assertEqual(result["latency_ns"], 100)
                run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
