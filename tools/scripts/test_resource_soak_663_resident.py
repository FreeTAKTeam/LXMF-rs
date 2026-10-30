"""Resident ownership must use reconciled kernel pages, never allocation requests."""
import os
import itertools
import hashlib
import json
from pathlib import Path
import subprocess
import signal
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import resource_soak_663_resident as resident


def mapping(start, name, anon, rss=None, flags="rw-p", device="00:00", inode=0):
    rss = anon if rss is None else rss
    return (f"{start:x}-{start + 16384:x} {flags} 00000000 {device} {inode} {name}\n"
            f"Size: 16 kB\nRss: {rss} kB\nAnonymous: {anon} kB\n"
            f"AnonHugePages: {anon} kB\nSwap: 4 kB\n")


class OwnershipTests(unittest.TestCase):
    def test_gate_accepts_exact_eighty_percent_and_rejects_one_bad_checkpoint(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "resident").mkdir()
            rows = []
            def write_capture(index, owned=16):
                smaps = mapping(0x1000, "[anon:mimalloc]", owned) + mapping(0x5000, "", 4)
                rollup = f"Anonymous: {owned + 4} kB\n"
                raw = {"schema": "lxmf-rs-resident-capture-v1", "index": index, "pid": 123,
                       "started_monotonic": index * 5 + .1,
                       "resumed_monotonic": index * 5 + .102, "pause_secs": .002,
                       "threads_before": {"123": "T"}, "threads_after": {"123": "T"},
                       "smaps": smaps, "rollup_before": rollup, "rollup_after": rollup}
                data = json.dumps(raw).encode()
                path = f"resident/{index:06}.json"
                (root / path).write_bytes(data)
                rows[index]["resident_memory"] = {
                    **resident.analyze_smaps(smaps, rollup, rollup), "raw_capture_path": path,
                    "raw_capture_sha256": hashlib.sha256(data).hexdigest(), "pause_secs": .002}
            def save_rows():
                (root / "samples.jsonl").write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            for index in range(720):
                rows.append({"index": index, "monotonic": index * 5,
                             "metrics": {"storage": {"maintenance": {
                                 name: {"runs_total": 1 if index < 120 else 2, "errors_total": 0}
                                 for name in ("expiry", "quota", "payload")}}}})
                write_capture(index)
            (root / "run.json").write_text(json.dumps({"daemon_pid": 123, "completed": True,
                "resident_memory": True, "duration_secs": 3600, "measured_duration_secs": 3600}))
            save_rows()
            self.assertTrue(resident.verify_resident_run(root)["resident_owner_80_percent_gate"])
            write_capture(200, 15)
            save_rows()
            self.assertFalse(resident.verify_resident_run(root)["resident_owner_80_percent_gate"])
            write_capture(200)
            save_rows()
            raw_path = root / "resident/000200.json"
            raw = json.loads(raw_path.read_text())
            raw["rollup_after"] = "Anonymous: 21 kB\n"
            data = json.dumps(raw).encode()
            raw_path.write_bytes(data)
            rows[200]["resident_memory"]["raw_capture_sha256"] = hashlib.sha256(data).hexdigest()
            save_rows()
            with self.assertRaisesRegex(ValueError, "reconcile"):
                resident.verify_resident_run(root)
            raw_path.unlink()
            with self.assertRaisesRegex(ValueError, "missing"):
                resident.verify_resident_run(root)

    def test_counts_only_named_private_anon_and_reconciles_every_byte(self):
        smaps = mapping(0x1000, "[anon:mimalloc]", 12) + mapping(0x5000, "/file", 4, 8)
        a = resident.analyze_smaps(smaps, "Anonymous: 16 kB\n", "Anonymous: 16 kB\n")
        self.assertEqual(a["total_anon_bytes"], 16384)
        self.assertEqual(a["mimalloc_anon_bytes"], 12288)
        self.assertEqual(a["unclassified_anon_bytes"], 4096)
        self.assertEqual(a["attributed_fraction"], .75)
        self.assertEqual(a["mimalloc_vmas"], 1)

    def test_similar_names_shared_and_file_backed_mappings_are_unclassified(self):
        for name, flags, device, inode in [
            ("[anon:mimalloc-extra]", "rw-p", "00:00", 0),
            ("[anon:mimalloc]", "rw-s", "00:00", 0),
            ("[anon:mimalloc]", "rw-p", "08:01", 9),
        ]:
            with self.subTest(name=name, flags=flags, device=device):
                a = resident.analyze_smaps(mapping(0x1000, name, 4, flags=flags, device=device, inode=inode), "Anonymous: 4 kB\n", "Anonymous: 4 kB\n")
                self.assertEqual(a["mimalloc_anon_bytes"], 0)
                self.assertEqual(a["unclassified_anon_bytes"], 4096)

    def test_missing_duplicate_overlapping_or_inconsistent_input_is_rejected(self):
        good = mapping(0x1000, "[anon:mimalloc]", 4)
        invalid = ["", good.replace("rw-p", "pppp"), good.replace("Anonymous: 4 kB\n", ""),
                   good + "Anonymous: 4 kB\n", good + good,
                   good.replace("Anonymous: 4", "Anonymous: 20")]
        for text in invalid:
            with self.subTest(text=text):
                with self.assertRaises(ValueError):
                    resident.analyze_smaps(text, "Anonymous: 4 kB\n", "Anonymous: 4 kB\n")
        for before, after in [(4, 8), (8, 8)]:
            with self.assertRaises(ValueError):
                resident.analyze_smaps(good, f"Anonymous: {before} kB\n", f"Anonymous: {after} kB\n")

    @unittest.skipUnless(sys.platform == "linux" and hasattr(os, "pidfd_open"), "Linux pidfd capture")
    def test_capture_resumes_owned_child_even_when_proc_read_fails(self):
        child = subprocess.Popen([sys.executable, "-c", "import threading,time; "
            "[threading.Thread(target=lambda: time.sleep(30),daemon=True).start() for _ in range(2)]; "
            "print('ready',flush=True); time.sleep(30)"], stdout=subprocess.PIPE, text=True)
        self.assertEqual(child.stdout.readline().strip(), "ready")
        descriptor = os.pidfd_open(child.pid)
        read_text = Path.read_text
        def fail_smaps(path, *args, **kwargs):
            if path.name == "smaps":
                raise OSError("injected smaps read failure")
            return read_text(path, *args, **kwargs)
        def assert_resumed():
            for _ in range(100):
                states = resident.thread_states(child.pid)
                if all(state != "T" for state in states.values()):
                    break
                time.sleep(.01)
            self.assertTrue(all(state != "T" for state in states.values()))
            self.assertIsNone(child.poll())
        try:
            with tempfile.TemporaryDirectory() as directory:
                summary = resident.capture_resident(child.pid, Path(directory), 0, descriptor)
                self.assertGreaterEqual(summary["vmas"], 1)
                raw = json.loads((Path(directory) / summary["raw_capture_path"]).read_text())
                self.assertGreaterEqual(len(raw["threads_before"]), 3)
                self.assertEqual(set(raw["threads_before"].values()), {"T"})
                with patch.object(Path, "read_text", fail_smaps):
                    with self.assertRaisesRegex(OSError, "injected"):
                        resident.capture_resident(child.pid, Path(directory), 0, descriptor)
                assert_resumed()
                with patch.object(resident, "thread_states", return_value={str(child.pid): "S"}):
                    with patch.object(resident.time, "monotonic", side_effect=itertools.count(step=10)):
                        with self.assertRaisesRegex(RuntimeError, "stop did not complete"):
                            resident.capture_resident(child.pid, Path(directory), 0, descriptor)
                assert_resumed()
                def cancel(_signum, _frame):
                    raise KeyboardInterrupt
                def interrupt_smaps(path, *args, **kwargs):
                    if path.name == "smaps":
                        os.kill(os.getpid(), signal.SIGTERM)
                    return read_text(path, *args, **kwargs)
                previous = signal.signal(signal.SIGTERM, cancel)
                try:
                    with patch.object(Path, "read_text", interrupt_smaps):
                        with self.assertRaises(KeyboardInterrupt):
                            resident.capture_resident(child.pid, Path(directory), 0, descriptor)
                finally:
                    signal.signal(signal.SIGTERM, previous)
            assert_resumed()
        finally:
            os.close(descriptor)
            child.terminate()
            child.wait(timeout=5)
            child.stdout.close()


if __name__ == "__main__":
    unittest.main()
