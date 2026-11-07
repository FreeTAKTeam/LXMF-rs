import copy
import json
import shlex
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from python_compat_lxmf_evidence import merge_event_page, native_completion
import python_compat_lxmd


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
                "RUST_EVIDENCE_DIR=" + shlex.quote(str(root)), "PY_DIR=" + shlex.quote(str(root)),
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
                    "native_completion(json.load(open(sys.argv[2])),*sys.argv[3:8],{'cd'*16:431})"
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
            "method": "propagation", "peer": "node", "bytes": size, "link_id": "cd" * 16,
            "resource_hash" if resource else "packet_hash": "ab" * 32,
        }}]}

    def check(self, evidence):
        return native_completion(evidence, "outbound", "custody", "auto",
                                 "propagation", "node", {"cd" * 16: 431, "ab" * 32: 431})

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
                             ("packet_hash", "ab"), ("bytes", 0), ("updated", False),
                             ("link_id", "de" * 16)):
            with self.subTest(field=field):
                evidence = self.evidence()
                evidence["events"][0]["payload"][field] = wrong
                with self.assertRaises(AssertionError):
                    self.check(evidence)

    def test_negotiated_peer_mdu_controls_the_packet_resource_boundary(self):
        limits = {"cd" * 16: 943, "ab" * 32: 943}
        native_completion(self.evidence(558), "outbound", "custody", "auto",
                          "propagation", "node", limits)
        native_completion(self.evidence(944, True), "outbound", "custody", "auto",
                          "propagation", "node", limits)
        for evidence in (self.evidence(944), self.evidence(943, True)):
            with self.assertRaises(AssertionError):
                native_completion(evidence, "outbound", "custody", "auto",
                                  "propagation", "node", limits)
        with self.assertRaises(AssertionError):
            native_completion(self.evidence(), "outbound", "custody", "auto",
                              "propagation", "node", {})

    def test_python_qualification_preserves_callbacks_and_records_actual_native_limits(self):
        calls = []

        class Router:
            def propagation_link_established(self, link):
                calls.append(("link", link))

            def propagation_resource_concluded(self, resource):
                calls.append(("resource", resource))
                return "processed"

        link = SimpleNamespace(link_id=bytes.fromhex("cd" * 16), mtu=1024, mdu=943)
        resource = SimpleNamespace(link=link, get_hash=lambda: bytes.fromhex("ab" * 32))

        def run(*args, **kwargs):
            self.assertEqual(sys.argv[1:], ["--config", "configuration"])
            router = Router()
            router.propagation_link_established(link)
            self.assertEqual(router.propagation_resource_concluded(resource), "processed")

        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "limits.jsonl"
            with mock.patch.dict(sys.modules, {"LXMF": SimpleNamespace(LXMRouter=Router)}), \
                 mock.patch.object(sys, "argv", ["qualification", str(path), "--config", "configuration"]), \
                 mock.patch.object(python_compat_lxmd.runpy, "run_module", side_effect=run) as runner:
                python_compat_lxmd.main()
            rows = [json.loads(line) for line in path.read_text().splitlines()]
        runner.assert_called_once_with("LXMF.Utilities.lxmd", run_name="__main__", alter_sys=True)
        self.assertEqual(calls, [("link", link), ("resource", resource)])
        self.assertEqual(rows, [{"link_id": "cd" * 16, "mtu": 1024, "mdu": 943},
                                {"link_id": "cd" * 16, "mtu": 1024, "mdu": 943,
                                 "resource_hash": "ab" * 32}])

    def test_cli_uses_recorded_mdu_and_rejects_conflicting_link_evidence(self):
        helper = Path(__file__).with_name("python_compat_lxmf_evidence.py")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            batch = root / "events.json"
            batch.write_text(json.dumps(self.evidence(558)))
            limits = root / "limits.jsonl"
            observed = {"link_id": "cd" * 16, "mtu": 1024, "mdu": 943}
            command = [sys.executable, str(helper), str(batch), "outbound", "custody",
                       "auto", "propagation", "node", str(limits)]
            limits.write_text(json.dumps(observed) + "\n")
            valid = subprocess.run(command, text=True, capture_output=True, timeout=10)
            self.assertEqual(valid.returncode, 0, valid.stderr)
            self.assertEqual(json.loads(valid.stdout)["bytes"], 558)
            with limits.open("a") as stream:
                stream.write(json.dumps({**observed, "mdu": 431}) + "\n")
            conflict = subprocess.run(command, text=True, capture_output=True, timeout=10)
            self.assertNotEqual(conflict.returncode, 0)

    def test_duplicate_outcomes_are_not_accepted_as_custody(self):
        evidence = self.evidence()
        evidence["events"].append(copy.deepcopy(evidence["events"][0]))
        with self.assertRaises(AssertionError):
            self.check(evidence)

    def test_custody_followed_by_any_negative_terminal_is_not_success(self):
        for status in ("rejected", "cancelled", "expired", "failed: transfer"):
            with self.subTest(status=status):
                evidence = self.evidence()
                terminal = copy.deepcopy(evidence["events"][0])
                terminal["payload"]["status"] = status
                evidence["events"].append(terminal)
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
