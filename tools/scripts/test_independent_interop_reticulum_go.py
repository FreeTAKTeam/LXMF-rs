#!/usr/bin/env python3
"""Tests for the Reticulum-Go independent interop control adapter."""

from __future__ import annotations

import json
from pathlib import Path
import signal
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

from independent_interop_reticulum_go import FastBufferedSocket, ReticulumGoControl


class ChunkSocket:
    def __init__(self, chunks: list[bytes]) -> None:
        self.chunks = list(chunks)

    def recv(self, _size: int) -> bytes:
        return self.chunks.pop(0) if self.chunks else b""


class FastBufferedSocketTests(unittest.TestCase):
    def test_reads_large_frames_without_retaining_consumed_prefixes(self) -> None:
        chunk = b"x" * (600 * 1024)
        buffered = FastBufferedSocket(ChunkSocket([chunk, chunk, chunk, chunk]), b"head")

        self.assertEqual(buffered.read_exact(4), b"head")
        self.assertEqual(buffered.read_exact(1200 * 1024), chunk + chunk)
        self.assertEqual(buffered.read_exact(1200 * 1024), chunk + chunk)
        self.assertLessEqual(len(buffered._buffer), 1200 * 1024)

    def test_read_until_preserves_following_frame_bytes(self) -> None:
        buffered = FastBufferedSocket(
            ChunkSocket([b"header\r\n", b"\r\npayload"])
        )

        self.assertEqual(buffered.read_until(b"\r\n\r\n"), b"header\r\n\r\n")
        self.assertEqual(buffered.read_exact(7), b"payload")


class FailureDiagnosticsTests(unittest.TestCase):
    def capture(self, process):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "peer.log"
            control = object.__new__(ReticulumGoControl)
            control._peer_process = SimpleNamespace(process=process, log_path=path)
            control.capture_failure_diagnostics("resource stalled")
            return json.loads(path.with_suffix(".diagnostics.json").read_text())

    @unittest.skipUnless(hasattr(signal, "SIGQUIT"), "Go goroutine signal requires Unix")
    def test_captures_only_the_owned_running_peer(self):
        process = mock.Mock(pid=123)
        process.poll.return_value = None
        process.wait.return_value = 2
        record = self.capture(process)
        process.send_signal.assert_called_once_with(signal.SIGQUIT)
        process.wait.assert_called_once_with(timeout=10)
        self.assertEqual(record, {"reason": "resource stalled", "pid": 123,
                                  "status": "peer_exited_after_signal", "exit_code": 2})

    def test_exited_peer_is_not_signalled(self):
        process = mock.Mock(pid=123)
        process.poll.return_value = 2
        self.assertEqual(self.capture(process)["status"], "already_exited")
        process.send_signal.assert_not_called()

    @unittest.skipUnless(hasattr(signal, "SIGQUIT"), "Go goroutine signal requires Unix")
    def test_capture_failure_is_recorded_without_replacing_the_scenario_result(self):
        process = mock.Mock(pid=123)
        process.poll.return_value = None
        process.wait.side_effect = subprocess.TimeoutExpired("peer", 10)
        record = self.capture(process)
        self.assertEqual(record["status"], "capture_failed")
        self.assertIn("timed out", record["error"])


if __name__ == "__main__":
    unittest.main()
