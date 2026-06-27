#!/usr/bin/env python3
"""Probe Python selector/epoll slow-reader TCP backpressure behavior.

This is intentionally stdlib-only. It is not a Reticulum BackboneInterface
emulation; it records the Python selector layer evidence that complements the
Rust Backbone HDLC bounded-channel slow-reader test.
"""

from __future__ import annotations

import argparse
import json
import platform
import selectors
import socket
import sys
import time
from pathlib import Path
from typing import Any


def _set_small_buffers(sock: socket.socket, size: int) -> None:
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, size)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, size)


def _accept_loopback_pair(buffer_size: int) -> tuple[socket.socket, socket.socket]:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    client = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    _set_small_buffers(client, buffer_size)
    client.setblocking(False)
    try:
        client.connect(listener.getsockname())
    except BlockingIOError:
        pass
    server, _ = listener.accept()
    listener.close()
    _set_small_buffers(server, buffer_size)
    client.setblocking(False)
    server.setblocking(False)
    return client, server


def _wait_writable(selector: selectors.BaseSelector, sock: socket.socket, timeout: float) -> bool:
    selector.modify(sock, selectors.EVENT_WRITE)
    return any(key.fileobj is sock for key, _ in selector.select(timeout))


def run_probe(
    *,
    buffer_size: int,
    payload_size: int,
    max_attempts: int,
    select_timeout: float,
    require_epoll: bool,
) -> dict[str, Any]:
    selector = selectors.DefaultSelector()
    selector_name = type(selector).__name__
    if require_epoll and platform.system() == "Linux" and selector_name != "EpollSelector":
        raise RuntimeError(f"expected EpollSelector on Linux, got {selector_name}")

    writer, reader = _accept_loopback_pair(buffer_size)
    selector.register(writer, selectors.EVENT_WRITE)

    payload = b"x" * payload_size
    total_sent = 0
    sends = 0
    backpressure_at: int | None = None
    non_writable_at: int | None = None
    started = time.monotonic()

    try:
        for attempt in range(1, max_attempts + 1):
            if not _wait_writable(selector, writer, select_timeout):
                non_writable_at = attempt
                break
            try:
                sent = writer.send(payload)
            except BlockingIOError:
                backpressure_at = attempt
                break
            if sent <= 0:
                backpressure_at = attempt
                break
            total_sent += sent
            sends += 1
            if sends == 1:
                observed = reader.recv(1)
                if observed != b"x":
                    raise RuntimeError(f"reader did not observe initial byte: {observed!r}")
                # Stop reading here. Subsequent successful sends must eventually
                # fill socket buffers and make the writer non-writable.
    finally:
        selector.unregister(writer)
        writer.close()
        reader.close()
        selector.close()

    elapsed_ms = int((time.monotonic() - started) * 1000)
    evidence = {
        "selector": selector_name,
        "platform": platform.platform(),
        "buffer_size": buffer_size,
        "payload_size": payload_size,
        "max_attempts": max_attempts,
        "select_timeout_ms": int(select_timeout * 1000),
        "sends_before_backpressure": sends,
        "bytes_sent_before_backpressure": total_sent,
        "blocking_error_attempt": backpressure_at,
        "non_writable_attempt": non_writable_at,
        "elapsed_ms": elapsed_ms,
        "result": "backpressured" if backpressure_at or non_writable_at else "not_observed",
    }
    if evidence["result"] != "backpressured":
        raise RuntimeError(f"writer stayed writable for {max_attempts} attempts")
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--buffer-size", type=int, default=4096)
    parser.add_argument("--payload-size", type=int, default=65536)
    parser.add_argument("--max-attempts", type=int, default=10_000)
    parser.add_argument("--select-timeout", type=float, default=0.01)
    parser.add_argument("--require-epoll", action="store_true")
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()

    evidence = run_probe(
        buffer_size=args.buffer_size,
        payload_size=args.payload_size,
        max_attempts=args.max_attempts,
        select_timeout=args.select_timeout,
        require_epoll=args.require_epoll,
    )
    text = json.dumps(evidence, indent=2, sort_keys=True) + "\n"
    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(text, encoding="utf-8")
    sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
