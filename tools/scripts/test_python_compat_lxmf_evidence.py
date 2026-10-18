import copy
import json
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from python_compat_lxmf_evidence import merge_event_page, native_completion


class NativeCompletionEvidenceTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("bash"), "Bash is required to exercise shell failure propagation")
    def test_new_stream_gap_cannot_fall_back_to_an_older_valid_completion(self):
        scripts = Path(__file__).resolve().parent
        source = (scripts / "python-lxmd-rust-lxmd-smoke.sh").read_text()
        names = ("capture_rust_message_evidence", "capture_rust_event_evidence", "wait_rust_native_completion")
        functions = []
        for name in names:
            start = source.index(name + "() {")
            end = source.index("\n}\n", start) + 3
            functions.append(source[start:end])
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            evidence_dir = root / "outbound"
            evidence_dir.mkdir()
            valid = self.evidence()
            valid.update(runtime_id="runtime", stream_id="events", next_cursor="old", dropped_count=0)
            valid["events"][0]["event_id"] = "already-complete"
            (evidence_dir / "sdk_poll_events_v2.json").write_text(json.dumps(valid))
            gap = {"runtime_id": "runtime", "stream_id": "events", "next_cursor": "new",
                   "dropped_count": 1, "events": []}
            script = "\n".join([
                "set -euo pipefail", *functions,
                "SCRIPT_DIR=" + shlex.quote(str(scripts)),
                "RUST_EVIDENCE_DIR=" + shlex.quote(str(root)),
                "ACTUAL_PYTHON=" + shlex.quote(sys.executable),
                "PYTHON_BIN=qualification_python", "RUST_RPC_ADDR=stub", "TIMEOUT_SECS=1",
                "rpc_call() { printf '%s\\n' " + shlex.quote(json.dumps(gap)) + "; }",
                # The unit isolates Bash's failure propagation. Call the real predicate
                # directly with the declared default Link MDU, without importing RNS.
                "qualification_python() {",
                'if [[ "$1" == "-c" || "$2" == "merge-events" ]]; then',
                '  "${ACTUAL_PYTHON}" "$@"',
                "else",
                '  "${ACTUAL_PYTHON}" -c ' + shlex.quote(
                    "import json,sys; from pathlib import Path; "
                    "sys.path.insert(0,str(Path(sys.argv[1]).parent)); "
                    "from python_compat_lxmf_evidence import native_completion; "
                    "native_completion(json.load(open(sys.argv[2])),*sys.argv[3:8],431)"
                ) + ' "$@"',
                "fi", "}",
                "if wait_rust_native_completion outbound custody auto propagation node; then exit 27; fi",
                "exit 0",
            ])
            result = subprocess.run([shutil.which("bash"), "-c", script], capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("SDK event history was truncated", result.stderr)

    def test_event_pages_reach_a_later_completion_without_losing_earlier_evidence(self):
        first = {"runtime_id": "runtime", "stream_id": "events", "next_cursor": "page1",
                 "dropped_count": 0, "events": [{"event_id": "first", "event_type": "announce"}]}
        later = {"runtime_id": "runtime", "stream_id": "events", "next_cursor": "page2",
                 "dropped_count": 0, "events": [{"event_id": "later", "event_type": "receipt"}]}
        merged = merge_event_page(later, first)
        self.assertEqual([row["event_id"] for row in merged["events"]], ["first", "later"])
        self.assertEqual(merged["next_cursor"], "page2")

    def test_event_gaps_and_stream_changes_fail_closed(self):
        page = {"runtime_id": "runtime", "stream_id": "events", "next_cursor": "page1",
                "dropped_count": 0, "events": [{"event_id": "first", "event_type": "receipt"}]}
        for changes in ({"dropped_count": 1},
                        {"events": [{"event_id": "gap", "event_type": "StreamGap"}]}):
            with self.assertRaises(AssertionError):
                merge_event_page({**page, **changes}, None)
        for changes in ({"runtime_id": "replacement"}, {"stream_id": "other"}, {}):
            with self.assertRaises(AssertionError):
                merge_event_page({**page, **changes}, page)

    def evidence(self, size=431, resource=False):
        return {"events": [{"event_type": "receipt", "payload": {
            "message_id": "outbound", "updated": True,
            "status": "sent: propagated resource" if resource else "sent: propagated",
            "delivery_kind": "link-resource" if resource else "link-packet",
            "method": "propagation", "peer": "node", "bytes": size,
            "resource_hash" if resource else "packet_hash": "ab" * 32,
        }}]}

    def check(self, evidence):
        return native_completion(evidence, "outbound", "custody", "auto",
                                 "propagation", "node", 431)

    def test_native_packet_and_resource_custody_at_the_boundary(self):
        self.check(self.evidence(431))
        self.check(self.evidence(432, True))
        with self.assertRaises(AssertionError):
            self.check(self.evidence(432))
        with self.assertRaises(AssertionError):
            self.check(self.evidence(431, True))

    def test_wrong_or_missing_native_binding_is_rejected(self):
        for field, wrong in (("peer", "another-node"), ("method", "direct"),
                             ("delivery_kind", "local-propagation-store"),
                             ("packet_hash", "ab"), ("bytes", 0), ("updated", False)):
            with self.subTest(field=field):
                evidence = self.evidence()
                evidence["events"][0]["payload"][field] = wrong
                with self.assertRaises(AssertionError):
                    self.check(evidence)

    def test_failed_or_duplicate_outcomes_are_not_accepted_as_custody(self):
        evidence = self.evidence()
        evidence["events"].append(copy.deepcopy(evidence["events"][0]))
        with self.assertRaises(AssertionError):
            self.check(evidence)
        evidence["events"][1]["payload"]["status"] = "failed: transfer"
        with self.assertRaises(AssertionError):
            self.check(evidence)

    def test_direct_resource_requires_actual_native_delivered_receipt(self):
        evidence = self.evidence(16384, True)
        evidence["events"][0]["payload"].update(status="delivered", method="direct")
        native_completion(evidence, "outbound", "delivered", "link-resource", "direct", "node")
        evidence["events"][0]["payload"]["status"] = "sent: link resource"
        with self.assertRaises(AssertionError):
            native_completion(evidence, "outbound", "delivered", "link-resource", "direct", "node")


if __name__ == "__main__":
    unittest.main()
