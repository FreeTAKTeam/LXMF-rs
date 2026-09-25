#!/usr/bin/env python3
"""Deterministic UDP peer for the configured reticulumd loopback trace."""

from __future__ import annotations

import hashlib
import json
import pathlib
import socket
import sys
import time


forward_port, daemon_port, report_path = int(sys.argv[1]), int(sys.argv[2]), pathlib.Path(sys.argv[3])
deadline = time.monotonic() + 25
received: bytes | None = None
peer = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
peer.bind(("127.0.0.1", forward_port))
peer.settimeout(0.25)

while time.monotonic() < deadline:
    try:
        payload, _ = peer.recvfrom(65535)
    except TimeoutError:
        continue
    if not payload:
        continue
    if received is None:
        received = payload
        peer.sendto(payload, ("127.0.0.1", daemon_port))
    report_path.write_text(
        json.dumps(
            {
                "status": "pass" if received is not None else "waiting",
                "packets_observed": 1,
                "echoed_to_daemon": received is not None,
                "payload_bytes": len(received) if received is not None else 0,
                "payload_sha256": hashlib.sha256(received).hexdigest() if received is not None else None,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    if received is not None:
        break

peer.close()
if received is None:
    raise SystemExit("timed out waiting for a production UDP packet")
