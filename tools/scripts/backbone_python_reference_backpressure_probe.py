#!/usr/bin/env python3
"""Probe pinned Python Reticulum BackboneInterface slow-reader behavior.

This imports the real Python Reticulum BackboneClientInterface from a checked
out reference tree and drives its HDLC transmit path against a loopback TCP peer
that stops reading after the first byte. The evidence complements the Rust
Backbone bounded-channel slow-reader test with actual Python Backbone epoll
behavior instead of a stdlib-only selector approximation.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import select
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


class _Owner:
    def inbound(self, _data: bytes, _iface: object) -> None:
        pass


class _Parent:
    def __init__(self) -> None:
        self.txb = 0
        self.rxb = 0
        self.spawned_interfaces: list[object] = []


def _set_small_buffers(sock: socket.socket, size: int) -> None:
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, size)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, size)


def _accept_loopback_pair(buffer_size: int) -> tuple[socket.socket, socket.socket]:
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    writer = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    _set_small_buffers(writer, buffer_size)
    writer.connect(listener.getsockname())
    reader, _ = listener.accept()
    listener.close()
    _set_small_buffers(reader, buffer_size)
    writer.setblocking(False)
    reader.setblocking(False)
    return writer, reader


def _read_one_byte(sock: socket.socket, timeout: float) -> bytes:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            data = sock.recv(1)
            if data:
                return data
        except BlockingIOError:
            pass
        time.sleep(0.01)
    raise RuntimeError("slow reader did not observe an initial byte")


def _git_revision(path: Path) -> str | None:
    try:
        return subprocess.check_output(
            ["git", "-C", str(path), "rev-parse", "HEAD"],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
    except Exception:
        return None


def _load_backbone_classes(python_rns_path: Path) -> tuple[Any, Any, Any]:
    sys.path.insert(0, str(python_rns_path))
    import RNS  # type: ignore
    from RNS.Interfaces.BackboneInterface import BackboneClientInterface, BackboneInterface  # type: ignore

    return RNS, BackboneInterface, BackboneClientInterface


def run_probe(
    *,
    python_rns_path: Path,
    buffer_size: int,
    payload_size: int,
    frames: int,
    initial_read_timeout: float,
    stable_wait: float,
    require_epoll: bool,
) -> dict[str, Any]:
    if require_epoll and platform.system() == "Linux" and not hasattr(select, "epoll"):
        raise RuntimeError("expected select.epoll on Linux")

    rns, backbone_interface, backbone_client_interface = _load_backbone_classes(python_rns_path)
    writer, reader = _accept_loopback_pair(buffer_size)
    fileno = writer.fileno()
    first_observed = b""
    started = time.monotonic()
    iface = None
    parent = _Parent()

    try:
        iface = backbone_client_interface(
            _Owner(),
            {"name": "python-reference-backpressure", "target_host": None, "target_port": None},
            connected_socket=writer,
        )
        iface.online = True
        iface.parent_interface = parent
        iface.target_ip = "127.0.0.1"
        iface.target_port = 0
        backbone_interface.add_client_socket(writer, iface)

        if require_epoll and platform.system() == "Linux":
            epoll = getattr(backbone_interface, "epoll", None)
            if epoll is None or type(epoll).__module__ != "select":
                raise RuntimeError(f"expected Python BackboneInterface select.epoll, got {type(epoll)!r}")

        payload = b"x" * payload_size
        for index in range(frames):
            iface.process_outgoing(payload)
            if index == 0:
                first_observed = _read_one_byte(reader, initial_read_timeout)
            time.sleep(0.001)

        # Let Python Backbone's epoll worker drain what the kernel accepts,
        # then verify the slow reader leaves a stable pending transmit buffer.
        time.sleep(stable_wait)
        pending_before = len(iface.transmit_buffer)
        txb_before = iface.txb
        parent_txb_before = parent.txb
        time.sleep(stable_wait)
        pending_after = len(iface.transmit_buffer)
        txb_after = iface.txb
        parent_txb_after = parent.txb
    finally:
        try:
            if fileno >= 0:
                backbone_interface.deregister_fileno(fileno)
                backbone_interface.spawned_interface_filenos.pop(fileno, None)
        except Exception:
            pass
        try:
            writer.close()
        except Exception:
            pass
        try:
            reader.close()
        except Exception:
            pass

    stalled = pending_after > 0 and txb_after == txb_before and parent_txb_after == parent_txb_before
    elapsed_ms = int((time.monotonic() - started) * 1000)
    evidence = {
        "result": "backpressured" if stalled else "not_observed",
        "platform": platform.platform(),
        "python_rns_path": str(python_rns_path),
        "python_rns_revision": _git_revision(python_rns_path),
        "rns_module": getattr(rns, "__file__", None),
        "backbone_class": "BackboneClientInterface",
        "epoll_type": type(getattr(backbone_interface, "epoll", None)).__name__,
        "buffer_size": buffer_size,
        "payload_size": payload_size,
        "frames_enqueued": frames,
        "first_observed_byte_hex": first_observed.hex(),
        "pending_transmit_buffer_before_stable_wait": pending_before,
        "pending_transmit_buffer_after_stable_wait": pending_after,
        "txb_before_stable_wait": txb_before,
        "txb_after_stable_wait": txb_after,
        "parent_txb_before_stable_wait": parent_txb_before,
        "parent_txb_after_stable_wait": parent_txb_after,
        "elapsed_ms": elapsed_ms,
    }
    if evidence["result"] != "backpressured":
        raise RuntimeError(f"Python BackboneInterface slow-reader backpressure not observed: {evidence}")
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--python-rns-path",
        type=Path,
        default=os.environ.get("PYTHON_RNS_PATH") or os.environ.get("RETICULUM_PY_REPO"),
    )
    parser.add_argument("--buffer-size", type=int, default=4096)
    parser.add_argument("--payload-size", type=int, default=65536)
    parser.add_argument("--frames", type=int, default=20)
    parser.add_argument("--initial-read-timeout", type=float, default=2.0)
    parser.add_argument("--stable-wait", type=float, default=0.25)
    parser.add_argument("--require-epoll", action="store_true")
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()

    if args.python_rns_path is None:
        raise SystemExit("--python-rns-path or PYTHON_RNS_PATH is required")
    python_rns_path = args.python_rns_path.resolve()
    if not (python_rns_path / "RNS" / "Interfaces" / "BackboneInterface.py").exists():
        raise SystemExit(f"{python_rns_path} does not look like a Python Reticulum checkout")

    evidence = run_probe(
        python_rns_path=python_rns_path,
        buffer_size=args.buffer_size,
        payload_size=args.payload_size,
        frames=args.frames,
        initial_read_timeout=args.initial_read_timeout,
        stable_wait=args.stable_wait,
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
