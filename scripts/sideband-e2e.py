#!/usr/bin/env python3
"""Run a bidirectional Reticulum daemon <-> Sideband interoperability smoke test."""

from __future__ import annotations

import argparse
import atexit
import http.client
import json
import os
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    repo_root = Path(__file__).resolve().parents[1]
    default_reticulum_rs = repo_root

    parser = argparse.ArgumentParser()
    parser.add_argument("--reticulum-rs-path", default=str(default_reticulum_rs))
    parser.add_argument("--reticulumd-bin", default=None)
    parser.add_argument("--sideband-path", default=None)
    parser.add_argument("--reticulum-py-path", default=None)
    parser.add_argument("--timeout-secs", type=int, default=75)
    parser.add_argument("--keep-artifacts", action="store_true")
    parser.add_argument("--artifact-root", default=None)
    return parser.parse_args()


def resolve_existing_path(candidates: list[Path], label: str) -> Path:
    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()
    joined = ", ".join(str(path) for path in candidates)
    raise RuntimeError(f"unable to resolve {label}; checked: {joined}")


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_until(predicate, timeout_secs: int, interval_secs: float = 0.5) -> bool:
    deadline = time.time() + timeout_secs
    while time.time() < deadline:
        if predicate():
            return True
        time.sleep(interval_secs)
    return False


def decode_text(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    if isinstance(value, str):
        return value
    return str(value)


def write_sideband_reticulum_config(config_dir: Path, daemon_transport_port: int) -> None:
    config_dir.mkdir(parents=True, exist_ok=True)
    config_path = config_dir / "config"
    config_text = f"""[reticulum]
  enable_transport = Yes
  share_instance = No
  panic_on_interface_error = No

[logging]
  loglevel = 4

[interfaces]
  [[Daemon TCP]]
    type = TCPClientInterface
    enabled = Yes
    target_host = 127.0.0.1
    target_port = {daemon_transport_port}
"""
    config_path.write_text(config_text, encoding="utf-8")


@dataclass
class RpcClient:
    host: str
    port: int
    msgpack: Any
    next_id: int = 1

    def call(self, method: str, params: Any | None = None) -> dict[str, Any]:
        request = {
            "id": self.next_id,
            "method": method,
            "params": params,
        }
        self.next_id += 1
        payload = self.msgpack.packb(request)
        framed = struct.pack(">I", len(payload)) + payload

        conn = http.client.HTTPConnection(self.host, self.port, timeout=8)
        conn.request(
            "POST",
            "/rpc",
            body=framed,
            headers={"Content-Type": "application/octet-stream"},
        )
        response = conn.getresponse()
        body = response.read()
        conn.close()

        if response.status != 200:
            raise RuntimeError(f"rpc {method} failed with http status {response.status}")
        if len(body) < 4:
            raise RuntimeError(f"rpc {method} returned an invalid framed body")

        (size,) = struct.unpack(">I", body[:4])
        payload = body[4:]
        if len(payload) < size:
            raise RuntimeError(f"rpc {method} returned truncated payload")
        decoded = self.msgpack.unpackb(payload[:size])
        response = normalize_rpc_response(decoded)
        if response["error"] is not None:
            raise RuntimeError(f"rpc {method} error: {response['error']}")
        return response


def normalize_rpc_response(decoded: Any) -> dict[str, Any]:
    if isinstance(decoded, dict):
        return {
            "id": decoded.get("id"),
            "result": decoded.get("result"),
            "error": decoded.get("error"),
        }
    if isinstance(decoded, (list, tuple)) and len(decoded) >= 3:
        return {
            "id": decoded[0],
            "result": decoded[1],
            "error": decoded[2],
        }
    raise RuntimeError(f"unsupported rpc response format: {type(decoded).__name__}")


def build_reticulumd_binary(reticulum_rs_path: Path, requested_bin: str | None) -> Path:
    if requested_bin:
        bin_path = Path(requested_bin).expanduser().resolve()
        if not bin_path.exists():
            raise RuntimeError(f"reticulumd binary not found: {bin_path}")
        return bin_path

    bin_path = reticulum_rs_path / "target" / "debug" / "reticulumd"
    if bin_path.exists():
        return bin_path

    subprocess.run(
        [
            "cargo",
            "build",
            "--manifest-path",
            str(reticulum_rs_path / "crates" / "reticulum-daemon" / "Cargo.toml"),
            "--bin",
            "reticulumd",
        ],
        check=True,
    )
    if not bin_path.exists():
        raise RuntimeError(f"reticulumd build succeeded but binary missing at {bin_path}")
    return bin_path


def main() -> int:
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[1]

    sideband_path = (
        Path(args.sideband_path).expanduser().resolve()
        if args.sideband_path
        else resolve_existing_path(
            [repo_root.parent / "Sideband", repo_root.parent / "sideband"],
            "Sideband path",
        )
    )
    reticulum_py_path = (
        Path(args.reticulum_py_path).expanduser().resolve()
        if args.reticulum_py_path
        else resolve_existing_path(
            [repo_root.parent / "Reticulum", repo_root.parent / "reticulum"],
            "Python Reticulum path",
        )
    )
    reticulum_rs_path = Path(args.reticulum_rs_path).expanduser().resolve()
    if not reticulum_rs_path.exists():
        raise RuntimeError(f"reticulum-rs path does not exist: {reticulum_rs_path}")

    for path in [str(sideband_path), str(repo_root), str(reticulum_py_path)]:
        if path not in sys.path:
            sys.path.insert(0, path)

    # Imports after sys.path mutation.
    import RNS.vendor.umsgpack as msgpack  # noqa: WPS433
    from sbapp.sideband.core import SidebandCore  # noqa: WPS433

    artifact_root = (
        Path(args.artifact_root).expanduser().resolve()
        if args.artifact_root
        else Path(tempfile.mkdtemp(prefix="sb-lxmf-e2e-"))
    )
    artifact_root.mkdir(parents=True, exist_ok=True)
    print(f"[sideband-e2e] artifact_root={artifact_root}", flush=True)

    daemon_log_path = artifact_root / "reticulumd.log"
    report_path = artifact_root / "report.json"

    sideband_config_root = artifact_root / "sideband-config"
    sideband_rns_root = artifact_root / "sideband-rns"
    daemon_db_path = artifact_root / "daemon.db"

    rpc_port = find_free_port()
    transport_port = find_free_port()
    rpc = RpcClient("127.0.0.1", rpc_port, msgpack)

    write_sideband_reticulum_config(sideband_rns_root, transport_port)
    reticulumd_bin = build_reticulumd_binary(reticulum_rs_path, args.reticulumd_bin)

    daemon_log_file = daemon_log_path.open("w", encoding="utf-8")
    daemon_proc = subprocess.Popen(
        [
            str(reticulumd_bin),
            "--rpc",
            f"127.0.0.1:{rpc_port}",
            "--db",
            str(daemon_db_path),
            "--transport",
            f"127.0.0.1:{transport_port}",
        ],
        cwd=str(repo_root),
        stdout=daemon_log_file,
        stderr=subprocess.STDOUT,
        text=True,
    )

    def cleanup_daemon() -> None:
        try:
            if daemon_proc.poll() is None:
                daemon_proc.send_signal(signal.SIGTERM)
                try:
                    daemon_proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    daemon_proc.kill()
        finally:
            daemon_log_file.close()

    atexit.register(cleanup_daemon)

    daemon_ready = wait_until(
        lambda: _rpc_ready(rpc),
        timeout_secs=max(10, min(args.timeout_secs, 45)),
        interval_secs=0.5,
    )
    if not daemon_ready:
        raise RuntimeError("reticulumd did not become ready in time")

    class DummyOwner:
        sideband = None

    owner = DummyOwner()
    sideband = SidebandCore(
        owner,
        config_path=str(sideband_config_root),
        is_client=False,
        verbose=False,
        quiet=True,
        rns_config_path=str(sideband_rns_root),
    )
    owner.sideband = sideband
    sideband.config["display_name"] = "sideband-e2e"
    sideband.start()

    daemon_status = rpc.call("daemon_status_ex")["result"]
    daemon_hash = daemon_status["identity_hash"]
    daemon_delivery_hash = daemon_status.get("delivery_destination_hash") or daemon_hash
    sideband_hash = sideband.lxmf_destination.hash.hex()
    daemon_delivery_hash_bytes = bytes.fromhex(daemon_delivery_hash)

    print(
        "[sideband-e2e] daemon_hash="
        f"{daemon_hash} daemon_delivery_hash={daemon_delivery_hash} sideband_hash={sideband_hash}",
        flush=True,
    )

    def peer_discovered() -> bool:
        try:
            peers = rpc.call("list_peers")["result"]["peers"]
        except Exception:
            return False
        return any(entry.get("peer") == sideband_hash for entry in peers)

    peer_discovery_deadline = time.time() + args.timeout_secs
    announce_round = 0
    while time.time() < peer_discovery_deadline:
        announce_round += 1
        rpc.call("announce_now")
        sideband.lxmf_announce()
        if peer_discovered():
            break
        time.sleep(1.0)

    peer_discovery_ok = peer_discovered()
    print(f"[sideband-e2e] peer_discovery={peer_discovery_ok}", flush=True)

    rust_to_sideband_content = f"rust->sideband e2e {int(time.time() * 1000)}"
    rust_to_sideband_id = f"rust-to-sideband-{int(time.time() * 1000)}"

    rust_send_ok = False
    if peer_discovery_ok:
        rpc.call(
            "send_message_v2",
            {
                "id": rust_to_sideband_id,
                "source": daemon_delivery_hash,
                "destination": sideband_hash,
                "title": "interop",
                "content": rust_to_sideband_content,
            },
        )
        rust_send_ok = wait_until(
            lambda: _sideband_has_message(
                sideband,
                daemon_delivery_hash_bytes,
                rust_to_sideband_content,
            ),
            timeout_secs=args.timeout_secs,
            interval_secs=0.5,
        )
    print(f"[sideband-e2e] rust_to_sideband={rust_send_ok}", flush=True)

    sideband_to_rust_content = f"sideband->rust e2e {int(time.time() * 1000)}"
    sideband_send_called = sideband.send_message(
        sideband_to_rust_content,
        daemon_delivery_hash_bytes,
        False,
        skip_fields=True,
        no_display=True,
    )
    sideband_to_rust_ok = False
    if sideband_send_called:
        sideband_to_rust_ok = wait_until(
            lambda: _daemon_has_message(rpc, sideband_to_rust_content, source_hash=sideband_hash),
            timeout_secs=args.timeout_secs,
            interval_secs=0.5,
        )
    print(
        "[sideband-e2e] sideband_send_called="
        f"{bool(sideband_send_called)} sideband_to_rust={sideband_to_rust_ok}",
        flush=True,
    )

    payload_probe_content = f"sideband payload probe {int(time.time() * 1000)}"
    payload_send_called = sideband.send_message(
        f"#!md\n{payload_probe_content}",
        daemon_delivery_hash_bytes,
        False,
        skip_fields=False,
        no_display=True,
        attachment=["note.txt", b"hello from sideband attachment"],
        image=[b"image/png", b"\x89PNG\r\n\x1a\n"],
        audio=[4, b"\x01\x02\x03\x04"],
    )
    sideband_payload_to_rust_ok = False
    if payload_send_called:
        sideband_payload_to_rust_ok = wait_until(
            lambda: _daemon_has_payload_fields(
                rpc,
                payload_probe_content,
                source_hash=sideband_hash,
                required_field_keys={"5", "6", "7", "15"},
            ),
            timeout_secs=args.timeout_secs,
            interval_secs=0.5,
        )
    print(
        "[sideband-e2e] payload_send_called="
        f"{bool(payload_send_called)} sideband_payload_to_rust={sideband_payload_to_rust_ok}",
        flush=True,
    )

    report = {
        "ok": bool(
            peer_discovery_ok
            and rust_send_ok
            and bool(sideband_send_called)
            and sideband_to_rust_ok
            and bool(payload_send_called)
            and sideband_payload_to_rust_ok
        ),
        "daemon_hash": daemon_hash,
        "daemon_delivery_hash": daemon_delivery_hash,
        "sideband_hash": sideband_hash,
        "peer_discovery": peer_discovery_ok,
        "rust_to_sideband": rust_send_ok,
        "sideband_send_called": bool(sideband_send_called),
        "sideband_to_rust": sideband_to_rust_ok,
        "payload_send_called": bool(payload_send_called),
        "sideband_payload_to_rust": sideband_payload_to_rust_ok,
        "artifact_root": str(artifact_root),
        "daemon_log": str(daemon_log_path),
    }
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2), flush=True)

    if not args.keep_artifacts and report["ok"]:
        pass

    return 0 if report["ok"] else 1


def _rpc_ready(rpc: RpcClient) -> bool:
    try:
        rpc.call("daemon_status_ex")
        return True
    except Exception:
        return False


def _sideband_has_message(sideband, context_dest: bytes, expected_content: str) -> bool:
    messages = sideband.list_messages(context_dest, limit=256) or []
    for entry in messages:
        if decode_text(entry.get("content")) == expected_content:
            return True
    return False


def _daemon_has_message(rpc: RpcClient, expected_content: str, source_hash: str) -> bool:
    result = rpc.call("list_messages")["result"]
    for message in result.get("messages", []):
        if message.get("direction") != "in":
            continue
        if message.get("source") != source_hash:
            continue
        if message.get("content") == expected_content:
            return True
    return False


def _daemon_has_payload_fields(
    rpc: RpcClient,
    expected_content: str,
    source_hash: str,
    required_field_keys: set[str],
) -> bool:
    result = rpc.call("list_messages")["result"]
    for message in result.get("messages", []):
        if message.get("direction") != "in":
            continue
        if message.get("source") != source_hash:
            continue
        if message.get("content") != expected_content:
            continue
        fields = message.get("fields")
        if not isinstance(fields, dict):
            return False
        present = set(fields.keys())
        return required_field_keys.issubset(present)
    return False


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(f"[sideband-e2e] ERROR: {exc}", file=sys.stderr)
        raise
