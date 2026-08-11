#!/usr/bin/env python3
"""Tests for the Reticulum-Go independent interop control adapter."""

from __future__ import annotations

import unittest

from independent_interop_reticulum_go import FastBufferedSocket


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


if __name__ == "__main__":
    unittest.main()
