import tempfile
import unittest
from pathlib import Path

from run import AppendOnlyLines, LedgerCounts, subject_log_is_error


class LedgerCountsTest(unittest.TestCase):
    def test_ready_events_are_tracked_per_peer_and_cannot_repeat(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'sender.jsonl').write_text('{"event":"node_ready","monotonic_ns":1}\n')
            (root / 'receiver.jsonl').write_text('{"event":"node_ready","monotonic_ns":2}\n')
            counter = LedgerCounts(root)
            counter.snapshot()
            self.assertEqual(counter.ready, {'sender': 1, 'receiver': 2})
            with (root / 'sender.jsonl').open('a') as output:
                output.write('{"event":"node_ready","monotonic_ns":3}\n')
            with self.assertRaisesRegex(RuntimeError, 'more than once'):
                counter.snapshot()

    def test_counts_new_complete_lines_once_and_rejects_truncation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            counter = LedgerCounts(root)
            sender = root / "sender.jsonl"
            receiver = root / "receiver.jsonl"
            sender.write_bytes(b'{"event":"attempted"}\n{"event":"sender_final",')
            self.assertEqual(counter.snapshot()["attempted"], 1)
            self.assertEqual(counter.snapshot()["accepted"], 0)
            with sender.open("ab") as output:
                output.write(b'"state":4}\n{"event":"sender_final","state":255}\n')
            receiver.write_bytes(b'{"event":"received"}\n')
            result = counter.snapshot()
            self.assertEqual((result["attempted"], result["accepted"], result["received"]), (1, 1, 1))
            self.assertEqual(counter.snapshot(), result)
            sender.write_bytes(b'')
            with self.assertRaisesRegex(RuntimeError, "truncated"):
                counter.snapshot()

    def test_log_tail_includes_final_unterminated_line_once(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "subject.log"
            tail = AppendOnlyLines(path)
            path.write_bytes(b'first line\nunfinished')
            self.assertEqual(tail.read(), [b'first line'])
            self.assertEqual(tail.read(final=True), [b'unfinished'])
            self.assertEqual(tail.read(final=True), [])

    def test_log_tail_rejects_replacement_and_changed_regrowth(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "subject.log"
            path.write_bytes(b'original line\n')
            tail = AppendOnlyLines(path)
            tail.read()
            path.write_bytes(b'different longer contents\n')
            with self.assertRaisesRegex(RuntimeError, "truncated or rewritten"):
                tail.read()
            replacement = path.with_suffix('.new')
            replacement.write_bytes(b'some longer replacement\n')
            replacement.replace(path)
            with self.assertRaisesRegex(RuntimeError, "replaced"):
                tail.read()
            path.unlink()
            with self.assertRaisesRegex(RuntimeError, "disappeared"):
                tail.read(final=True)

    def test_log_error_detection_distinguishes_severity_from_prose(self):
        self.assertTrue(subject_log_is_error('[Error] transport failed'))
        self.assertTrue(subject_log_is_error('ERROR transport failed'))
        self.assertTrue(subject_log_is_error('2026-11-01T12:30:00Z ERROR daemon: transport failed'))
        self.assertTrue(subject_log_is_error("thread 'worker' panicked at source.rs:1"))
        self.assertFalse(subject_log_is_error('No error was reported'))
        self.assertFalse(subject_log_is_error('INFO payload=ERROR handled'))


if __name__ == "__main__":
    unittest.main()
