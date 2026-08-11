#!/usr/bin/env python3
"""Pinned Reticulum-Go control-plane adapter and two-node live scenarios."""

from __future__ import annotations

import base64
import importlib.util
import json
import os
import random
import socket
import threading
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

from independent_interop_rns_rs import deterministic_payload, msgpack_binary, rust_event
from independent_interop_support import (
    Evidence,
    ManagedProcess,
    RustProbe,
    b64,
    free_port,
    sha256,
    wait_until,
)
from independent_interop_topology import (
    exchange_rust_endpoint_announces,
    link_rust_via_rns_rs,
    packet_rust_via_rns_rs,
    resource_rust_via_rns_rs,
    teardown_rust_link,
)


TOPOLOGY_RUST_GO_RUST = "LXMF-rs — Reticulum-Go — LXMF-rs"


class FastBufferedSocket:
    """Linear-time reader compatible with the pinned example client's API."""

    def __init__(self, sock: socket.socket, initial: bytes = b"") -> None:
        self._sock = sock
        self._buffer = bytearray(initial)
        self._offset = 0

    def _fill(self) -> None:
        chunk = self._sock.recv(1024 * 1024)
        if not chunk:
            raise ConnectionError("control API closed the connection")
        self._buffer.extend(chunk)

    def _compact(self) -> None:
        if self._offset >= 1024 * 1024 and self._offset * 2 >= len(self._buffer):
            del self._buffer[: self._offset]
            self._offset = 0

    def read_exact(self, size: int) -> bytes:
        while len(self._buffer) - self._offset < size:
            self._fill()
        end = self._offset + size
        data = bytes(self._buffer[self._offset : end])
        self._offset = end
        self._compact()
        return data

    def read_until(self, delimiter: bytes) -> bytes:
        while True:
            index = self._buffer.find(delimiter, self._offset)
            if index >= 0:
                end = index + len(delimiter)
                data = bytes(self._buffer[self._offset : end])
                self._offset = end
                self._compact()
                return data
            self._fill()


class ReticulumGoControl:
    """Use the pinned peer's stdlib-only example client as its public adapter."""

    def __init__(self, peer_root: Path, port: int, token: str) -> None:
        example = peer_root / "examples/control-client/client.py"
        spec = importlib.util.spec_from_file_location("reticulum_go_control_client", example)
        if spec is None or spec.loader is None:
            raise RuntimeError(f"cannot load Reticulum-Go control client {example}")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self._module = module
        self.port = port
        self.token = token
        _, session = module.http_json("127.0.0.1", port, "POST", "/v1/sessions", token, {})
        self.session_id = session["session_id"]
        self.identity_hash = session["identity_hash"]
        self.sock, peer_buffered = module.ws_connect(
            "127.0.0.1", port, f"/v1/sessions/{self.session_id}/events", token
        )
        self.buffered = FastBufferedSocket(
            self.sock, bytes(getattr(peer_buffered, "_buf", b""))
        )
        self.sock.settimeout(0.25)
        self._events: list[dict[str, Any]] = []
        self._events_lock = threading.Lock()
        self._stop = threading.Event()
        self._reader = threading.Thread(target=self._read_events, daemon=True)
        self._reader.start()
        self.command({"type": "subscribe_announces"})

    def _read_events(self) -> None:
        while not self._stop.is_set():
            try:
                opcode, payload = self._module.ws_recv(self.buffered)
            except socket.timeout:
                continue
            except (ConnectionError, OSError):
                return
            if opcode == 0x8:
                return
            if opcode == 0x1:
                event = json.loads(payload.decode("utf-8"))
                with self._events_lock:
                    self._events.append(event)

    def command(self, value: dict[str, Any]) -> None:
        self._module.ws_send_text(self.sock, json.dumps(value).encode("utf-8"))

    def http(self, method: str, path: str, body: Any = None) -> Any:
        return self._module.http_json(
            "127.0.0.1", self.port, method, path, self.token, body
        )[1]

    def register_destination(self) -> str:
        result = self.http(
            "POST",
            f"/v1/sessions/{self.session_id}/destinations",
            {"app_name": "interop", "aspects": ["probe"], "accepts_links": True},
        )
        return result["destination_hash"]

    def announce(self, destination: str, app_data: bytes) -> None:
        self.http(
            "POST",
            f"/v1/sessions/{self.session_id}/destinations/{destination}/announce",
            {"app_data": b64(app_data)},
        )

    def events(self, clear: bool = False) -> list[dict[str, Any]]:
        with self._events_lock:
            result = list(self._events)
            if clear:
                self._events.clear()
            return result

    def paths(self) -> list[dict[str, Any]]:
        result = self.http("GET", "/v1/paths")
        return result["paths"] if isinstance(result, dict) else result

    def close(self) -> None:
        self._stop.set()
        try:
            self.sock.close()
        finally:
            self._reader.join(timeout=1)
            try:
                self.http("DELETE", f"/v1/sessions/{self.session_id}")
            except Exception:
                pass


def write_go_config(path: Path, rust_port: int, control_port: int, token: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "[reticulum]\n"
        "  enable_transport = No\n"
        "  share_instance = No\n"
        "  enable_sandbox = No\n"
        "  panic_on_interface_error = Yes\n"
        f"  rpc_key = {token}\n"
        "  enable_control_api = Yes\n"
        "  control_api_host = 127.0.0.1\n"
        f"  control_api_port = {control_port}\n\n"
        "[logging]\n"
        "  loglevel = 4\n"
        "  destination = stderr\n\n"
        "[interfaces]\n\n"
        "  [[LXMF-rs]]\n"
        "    type = TCPClientInterface\n"
        "    enabled = Yes\n"
        "    target_host = 127.0.0.1\n"
        f"    target_port = {rust_port}\n",
        encoding="utf-8",
    )


def write_go_transport_config(
    path: Path,
    endpoint_a_port: int,
    endpoint_c_port: int,
    control_port: int,
    token: str,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "[reticulum]\n"
        "  enable_transport = Yes\n"
        "  share_instance = No\n"
        "  enable_sandbox = No\n"
        "  panic_on_interface_error = Yes\n"
        f"  rpc_key = {token}\n"
        "  enable_control_api = Yes\n"
        "  control_api_host = 127.0.0.1\n"
        f"  control_api_port = {control_port}\n\n"
        "[logging]\n"
        "  loglevel = 4\n"
        "  destination = stderr\n\n"
        "[interfaces]\n\n"
        "  [[LXMF-rs endpoint A]]\n"
        "    type = TCPClientInterface\n"
        "    enabled = Yes\n"
        "    target_host = 127.0.0.1\n"
        f"    target_port = {endpoint_a_port}\n\n"
        "  [[LXMF-rs endpoint C]]\n"
        "    type = TCPClientInterface\n"
        "    enabled = Yes\n"
        "    target_host = 127.0.0.1\n"
        f"    target_port = {endpoint_c_port}\n",
        encoding="utf-8",
    )


@contextmanager
def two_node_session(
    root: Path, repository_root: Path, peer_root: Path, peer_binary: Path
) -> Iterator[tuple[RustProbe, ReticulumGoControl, str, str]]:
    rust_port = free_port()
    rust_control_port = free_port()
    go_control_port = free_port()
    token = "6c786d662d72732d696e646570656e64656e742d696e7465726f702d676f"
    config = root / "config/reticulum-go/config"
    write_go_config(config, rust_port, go_control_port, token)
    logs = root / "logs"
    processes: list[ManagedProcess] = []
    control: ReticulumGoControl | None = None
    try:
        processes.append(
            ManagedProcess(
                "LXMF-rs Reticulum-Go probe",
                [
                    str(repository_root / "target/release/independent-interop-node"),
                    "--name",
                    "lxmf-rs-reticulum-go",
                    "--identity-seed",
                    "lxmf-rs-independent-reticulum-go",
                    "--control",
                    f"127.0.0.1:{rust_control_port}",
                    "--listen",
                    f"127.0.0.1:{rust_port}",
                ],
                repository_root,
                logs / "reticulum-go-lxmf-rs.log",
                {"RUST_LOG": "info"},
            )
        )
        rust = RustProbe(rust_control_port)
        rust_status = wait_until("LXMF-rs control", lambda: rust.call("status"), timeout=15)
        processes.append(
            ManagedProcess(
                "Reticulum-Go peer",
                [str(peer_binary), "daemon", "--config", str(config)],
                peer_root,
                logs / "reticulum-go-peer.log",
            )
        )
        wait_until(
            "Reticulum-Go control API",
            lambda: _go_health(peer_root, go_control_port, token),
            timeout=20,
        )
        control = ReticulumGoControl(peer_root, go_control_port, token)
        go_destination = control.register_destination()
        yield rust, control, rust_status["destination_hash"], go_destination
    finally:
        if control is not None:
            control.close()
        try:
            RustProbe(rust_control_port).call("shutdown")
        except Exception:
            pass
        for process in reversed(processes):
            process.stop()


def _go_health(peer_root: Path, port: int, token: str) -> Any:
    example = peer_root / "examples/control-client/client.py"
    spec = importlib.util.spec_from_file_location("reticulum_go_health_client", example)
    if spec is None or spec.loader is None:
        return None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    try:
        return module.http_json("127.0.0.1", port, "GET", "/v1/health", token)[1]
    except OSError:
        return None


def run_two_node(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    with two_node_session(root, repository_root, peer_root, peer_binary) as (
        rust,
        go,
        rust_destination,
        go_destination,
    ):
        evidence.run(
            "path unknown, request, response, establishment and cache",
            "bidirectional",
            lambda: discover_paths(rust, go, rust_destination, go_destination),
        )
        evidence.run(
            "announce identity signature and app-data",
            "bidirectional",
            lambda: exchange_announces(
                rust, go, rust_destination, go_destination
            ),
        )
        link = evidence.run(
            "Link establishment proof RTT",
            "Reticulum-Go -> LXMF-rs",
            lambda: link_go_to_rust(rust, go, rust_destination),
        )
        if link is None:
            return
        link_id = link["link_id"]
        evidence.run(
            "Link data",
            "bidirectional",
            lambda: exchange_link_data(rust, go, link_id),
        )
        evidence.run(
            "packet-sized request response and correlation",
            "Reticulum-Go -> LXMF-rs",
            lambda: request_go_to_rust(rust, go, link_id),
        )
        payload = deterministic_payload(4096)
        evidence.run(
            "Resource small",
            "Reticulum-Go -> LXMF-rs",
            lambda: resource_go_to_rust(rust, go, link_id, payload),
            expected_bytes=len(payload),
            content_hash=sha256(payload),
        )
        evidence.run(
            "Resource small",
            "LXMF-rs -> Reticulum-Go",
            lambda: resource_rust_to_go(rust, go, link_id, payload),
            expected_bytes=len(payload),
            content_hash=sha256(payload),
        )
        one_mib = random.Random(0x52474F31).randbytes(1024 * 1024)
        evidence.run(
            "Resource 1 MiB",
            "LXMF-rs -> Reticulum-Go",
            lambda: resource_rust_to_go(rust, go, link_id, one_mib),
            expected_bytes=len(one_mib),
            content_hash=sha256(one_mib),
        )
        evidence.record(
            "Resource 1 MiB",
            "Reticulum-Go control API -> LXMF-rs",
            "UNSUPPORTED",
            "peer control API caps inbound WebSocket commands at 1 MiB before base64 expansion",
            failure_owner="Reticulum-Go",
            classification="peer_surface_unavailable",
        )
        if level in {"nightly", "release"}:
            fifty_mib = random.Random(0x52474F50).randbytes(50 * 1024 * 1024)
            evidence.run(
                "Resource 50 MiB",
                "LXMF-rs -> Reticulum-Go",
                lambda: resource_rust_to_go(rust, go, link_id, fifty_mib),
                expected_bytes=len(fifty_mib),
                content_hash=sha256(fifty_mib),
            )
            evidence.record(
                "Resource 50 MiB",
                "Reticulum-Go control API -> LXMF-rs",
                "UNSUPPORTED",
                "peer control API caps inbound WebSocket commands at 1 MiB before base64 expansion",
                failure_owner="Reticulum-Go",
                classification="peer_surface_unavailable",
            )
        evidence.run(
            "Link teardown",
            "Reticulum-Go -> LXMF-rs",
            lambda: close_link(rust, go, link_id),
        )
        reverse = evidence.run(
            "Link establishment proof RTT",
            "LXMF-rs -> Reticulum-Go",
            lambda: link_rust_to_go(rust, go, go_destination),
        )
        if reverse is not None:
            evidence.run(
                "packet-sized request response and correlation",
                "LXMF-rs -> Reticulum-Go",
                lambda: request_rust_to_go(
                    rust, go, go_destination, reverse["link_id"]
                ),
            )
            evidence.run(
                "Link teardown",
                "LXMF-rs -> Reticulum-Go",
                lambda: close_link(rust, go, reverse["link_id"]),
            )


def run_multi_hop(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    del level
    with rust_go_rust_topology(
        root, repository_root, peer_root, peer_binary
    ) as (endpoint_a, endpoint_c, destination_a, destination_c):
        evidence.run(
            "multi-hop announce propagation, path expiry and rediscovery",
            "bidirectional",
            lambda: exchange_go_transport_announces(
                endpoint_a, endpoint_c, destination_a, destination_c
            ),
            topology=TOPOLOGY_RUST_GO_RUST,
        )
        for direction, sender, receiver, destination, payload in (
            (
                "left -> right",
                endpoint_a,
                endpoint_c,
                destination_c,
                b"rust-a-via-reticulum-go-to-rust-c",
            ),
            (
                "right -> left",
                endpoint_c,
                endpoint_a,
                destination_a,
                b"rust-c-via-reticulum-go-to-rust-a",
            ),
        ):
            evidence.run(
                "multi-hop encrypted packet and delivery proof",
                direction,
                lambda s=sender, r=receiver, d=destination, p=payload: (
                    packet_rust_via_rns_rs(s, r, d, p)
                ),
                topology=TOPOLOGY_RUST_GO_RUST,
                expected_bytes=len(payload),
                content_hash=sha256(payload),
            )

        link = evidence.run(
            "multi-hop Link establishment proof",
            "left -> right",
            lambda: link_rust_via_rns_rs(endpoint_a, endpoint_c, destination_c),
            topology=TOPOLOGY_RUST_GO_RUST,
        )
        if link is None:
            return
        link_id = link["link_id"]
        payload = deterministic_payload(1024 * 1024)
        digest = sha256(payload)
        for direction, sender, receiver in (
            ("left -> right", endpoint_a, endpoint_c),
            ("right -> left", endpoint_c, endpoint_a),
        ):
            evidence.run(
                "multi-hop Resource 1 MiB",
                direction,
                lambda s=sender, r=receiver: resource_rust_via_rns_rs(
                    s, r, link_id, payload
                ),
                topology=TOPOLOGY_RUST_GO_RUST,
                expected_bytes=len(payload),
                content_hash=digest,
            )
        evidence.run(
            "Link teardown",
            "bidirectional",
            lambda: teardown_rust_link(endpoint_a, endpoint_c, link_id),
            topology=TOPOLOGY_RUST_GO_RUST,
        )
        reverse_link = evidence.run(
            "multi-hop Link establishment proof",
            "right -> left",
            lambda: link_rust_via_rns_rs(endpoint_c, endpoint_a, destination_a),
            topology=TOPOLOGY_RUST_GO_RUST,
        )
        if reverse_link is not None:
            evidence.run(
                "Link teardown",
                "right -> left",
                lambda: teardown_rust_link(
                    endpoint_c, endpoint_a, reverse_link["link_id"]
                ),
                topology=TOPOLOGY_RUST_GO_RUST,
            )
        else:
            evidence.record(
                "Link teardown",
                "right -> left",
                "BLOCKED",
                "Link teardown cannot execute because reverse Link activation failed",
                topology=TOPOLOGY_RUST_GO_RUST,
                classification="dependency_failed",
            )


@contextmanager
def rust_go_rust_topology(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
) -> Iterator[tuple[RustProbe, RustProbe, str, str]]:
    port_ab = free_port()
    port_bc = free_port()
    control_a = free_port()
    control_c = free_port()
    go_control_port = free_port()
    token = "6c786d662d72732d696e646570656e64656e742d676f2d7472616e73706f7274"
    config = root / "config/reticulum-go-transport/config"
    write_go_transport_config(config, port_ab, port_bc, go_control_port, token)
    logs = root / "logs"
    processes: list[ManagedProcess] = []
    control: ReticulumGoControl | None = None
    try:
        for label, name, seed, control_port, listen_port, log_name in (
            (
                "left",
                "lxmf-rs-go-transport-a",
                "lxmf-rs-independent-go-transport-a",
                control_a,
                port_ab,
                "multi-hop-rust-a-go-transport.log",
            ),
            (
                "right",
                "lxmf-rs-go-transport-c",
                "lxmf-rs-independent-go-transport-c",
                control_c,
                port_bc,
                "multi-hop-rust-c-go-transport.log",
            ),
        ):
            processes.append(
                ManagedProcess(
                    f"{label} LXMF-rs endpoint",
                    [
                        str(repository_root / "target/release/independent-interop-node"),
                        "--name",
                        name,
                        "--identity-seed",
                        seed,
                        "--control",
                        f"127.0.0.1:{control_port}",
                        "--listen",
                        f"127.0.0.1:{listen_port}",
                    ],
                    repository_root,
                    logs / log_name,
                    {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
                )
            )
        endpoint_a = RustProbe(control_a)
        endpoint_c = RustProbe(control_c)
        status_a = wait_until(
            "left LXMF-rs endpoint control",
            lambda: endpoint_a.call("status"),
            timeout=15,
        )
        status_c = wait_until(
            "right LXMF-rs endpoint control",
            lambda: endpoint_c.call("status"),
            timeout=15,
        )
        processes.append(
            ManagedProcess(
                "Reticulum-Go transport B",
                [str(peer_binary), "daemon", "--config", str(config)],
                peer_root,
                logs / "multi-hop-reticulum-go-transport-b.log",
            )
        )
        wait_until(
            "Reticulum-Go transport control API",
            lambda: _go_health(peer_root, go_control_port, token),
            timeout=20,
        )
        control = ReticulumGoControl(peer_root, go_control_port, token)
        wait_until(
            "Reticulum-Go transport interfaces",
            lambda: _go_transport_ready(control),
            timeout=20,
        )
        yield (
            endpoint_a,
            endpoint_c,
            status_a["destination_hash"],
            status_c["destination_hash"],
        )
    finally:
        if control is not None:
            control.close()
        for control_port in (control_a, control_c):
            try:
                RustProbe(control_port).call("shutdown")
            except Exception:
                pass
        for process in reversed(processes):
            process.stop()


def _go_transport_ready(control: ReticulumGoControl) -> dict[str, Any] | None:
    status = control.http("GET", "/v1/status")
    online = [item for item in status.get("interfaces", []) if item.get("status")]
    if len(online) >= 2:
        return {"online_interfaces": len(online)}
    return None


def exchange_go_transport_announces(
    endpoint_a: RustProbe,
    endpoint_c: RustProbe,
    destination_a: str,
    destination_c: str,
) -> dict[str, Any]:
    result = exchange_rust_endpoint_announces(
        endpoint_a, endpoint_c, destination_a, destination_c
    )
    rediscovered: dict[str, Any] = {}
    for label, endpoint, destination in (
        ("left", endpoint_a, destination_c),
        ("right", endpoint_c, destination_a),
    ):
        expired = endpoint.call("expire_path", {"destination_hash": destination})
        if expired.get("expired") is not True:
            raise AssertionError(f"{label} endpoint did not expire path {destination}")
        endpoint.call("request_path", {"destination_hash": destination})
        path = wait_until(
            f"{label} endpoint Reticulum-Go path response",
            lambda endpoint=endpoint, destination=destination: (
                candidate
                if (candidate := endpoint.call(
                    "has_path", {"destination_hash": destination}
                )).get("path_found")
                and candidate.get("hops") == 2
                and candidate.get("next_hop") != destination
                else None
            ),
            timeout=20,
            interval=0.25,
        )
        rediscovered[f"{label}_rediscovered_path"] = path
    result.update(rediscovered)
    return result


def exchange_announces(
    rust: RustProbe,
    go: ReticulumGoControl,
    rust_destination: str,
    go_destination: str,
) -> dict[str, Any]:
    go.events(clear=True)
    rust_app = b"lxmf-rs-to-reticulum-go"
    go_app = b"reticulum-go-to-lxmf-rs"
    rust.call("announce", {"app_data": b64(rust_app)})
    go_seen = wait_until(
        "LXMF-rs announce at Reticulum-Go",
        lambda: next(
            (
                event
                for event in go.events()
                if event.get("type") == "announce"
                and event.get("destination_hash") == rust_destination
                and event.get("app_data") == b64(rust_app)
            ),
            None,
        ),
        timeout=20,
    )
    rust_seen = rust_event(
        rust,
        lambda event: event.get("type") == "announce"
        and event.get("destination_hash") == go_destination
        and event.get("app_data") == b64(go_app),
        "Reticulum-Go announce at LXMF-rs",
        timeout=20,
    )
    go_path = next(item for item in go.paths() if item.get("hash") == rust_destination)
    rust_path = rust.call("has_path", {"destination_hash": go_destination})
    return {
        "reticulum_go_hops": go_seen["hops"],
        "lxmf_rs_hops": rust_seen["hops"],
        "reticulum_go_path_hops": go_path["hops"],
        "lxmf_rs_path_hops": rust_path["hops"],
    }


def discover_paths(
    rust: RustProbe,
    go: ReticulumGoControl,
    rust_destination: str,
    go_destination: str,
) -> dict[str, Any]:
    rust_unknown = rust.call("has_path", {"destination_hash": go_destination})
    go_unknown = next(
        (item for item in go.paths() if item.get("hash") == rust_destination), None
    )
    if rust_unknown.get("path_found") or go_unknown is not None:
        raise AssertionError("fresh Reticulum-Go topology unexpectedly started with cached paths")

    go.http(
        "POST",
        f"/v1/sessions/{go.session_id}/path/request",
        {"destination_hash": rust_destination},
    )
    go_path = wait_until(
        "Reticulum-Go path response from LXMF-rs",
        lambda: next(
            (item for item in go.paths() if item.get("hash") == rust_destination), None
        ),
        timeout=20,
    )
    # Prime the peer's cached local announce, then explicitly remove the Rust
    # route so the next observation can only come from a network path request.
    go.announce(go_destination, b"reticulum-go-to-lxmf-rs")
    wait_until(
        "initial Reticulum-Go path at LXMF-rs",
        lambda: (
            value
            if (value := rust.call("has_path", {"destination_hash": go_destination})).get(
                "path_found"
            )
            else None
        ),
        timeout=20,
    )
    expired = rust.call("expire_path", {"destination_hash": go_destination})
    if not expired.get("expired"):
        raise AssertionError("LXMF-rs did not expire the primed Reticulum-Go path")
    rust.call("request_path", {"destination_hash": go_destination})
    rust_path = wait_until(
        "LXMF-rs path response from Reticulum-Go",
        lambda: (
            value
            if (value := rust.call("has_path", {"destination_hash": go_destination})).get(
                "path_found"
            )
            else None
        ),
        timeout=20,
    )
    return {
        "unknown_confirmed": True,
        "reticulum_go_hops": go_path["hops"],
        "lxmf_rs_hops": rust_path["hops"],
        "cached_paths": True,
    }


def request_go_to_rust(
    rust: RustProbe,
    go: ReticulumGoControl,
    link_id: str,
) -> dict[str, Any]:
    go.events(clear=True)
    rust.events(clear=True)
    go_payload = b"request-reticulum-go-to-lxmf-rs"
    go.command(
        {
            "type": "link.request",
            "link_id": link_id,
            "path": "/interop/echo",
            "data": b64(go_payload),
            "timeout_ms": 5000,
        }
    )
    at_rust = rust_event(
        rust,
        lambda event: event.get("type") == "data"
        and event.get("context") == 9
        and event.get("application_data") == b64(go_payload),
        "Reticulum-Go request at LXMF-rs",
        timeout=15,
    )
    rust.call(
        "respond",
        {
            "link_id": link_id,
            "request_id": at_rust["request_id"],
            "data": b64(msgpack_binary(go_payload)),
        },
    )
    response = wait_until(
        "LXMF-rs correlated response at Reticulum-Go",
        lambda: next(
            (
                event
                for event in go.events()
                if event.get("type") == "request.response"
                and event.get("link_id") == link_id
                and event.get("data") == b64(go_payload)
            ),
            None,
        ),
        timeout=15,
    )

    return {
        "request_id": response["request_id"],
        "response_sha256": sha256(go_payload),
    }


def request_rust_to_go(
    rust: RustProbe,
    go: ReticulumGoControl,
    go_destination: str,
    link_id: str,
) -> dict[str, Any]:
    go.http(
        "POST",
        f"/v1/sessions/{go.session_id}/destinations/{go_destination}/requests",
        {"path": "/interop/echo"},
    )
    rust_payload = b"request-lxmf-rs-to-reticulum-go"
    rust.events(clear=True)
    go.events(clear=True)
    sent = rust.call(
        "request",
        {
            "link_id": link_id,
            "path": "/interop/echo",
            "data": b64(msgpack_binary(rust_payload)),
        },
    )
    at_go = wait_until(
        "LXMF-rs request at Reticulum-Go",
        lambda: next(
            (
                event
                for event in go.events()
                if event.get("type") == "request.incoming"
                and event.get("link_id") == link_id
                and event.get("path") == "/interop/echo"
                and event.get("data") == b64(rust_payload)
            ),
            None,
        ),
        timeout=15,
    )
    go.command(
        {
            "type": "request.respond",
            "request_id": at_go["request_id"],
            "data": b64(rust_payload),
        }
    )
    rust_response = rust_event(
        rust,
        lambda event: event.get("type") == "data"
        and event.get("context") == 10
        and event.get("request_id") == sent["request_id"]
        and event.get("application_data") == b64(rust_payload),
        "Reticulum-Go correlated response at LXMF-rs",
        timeout=15,
    )
    return {
        "rust_request_id": sent["request_id"],
        "rust_response_request_id": rust_response["request_id"],
        "response_sha256": sha256(rust_payload),
    }


def link_go_to_rust(
    rust: RustProbe, go: ReticulumGoControl, rust_destination: str
) -> dict[str, Any]:
    go.command({"type": "link.open", "destination_hash": rust_destination})
    event = wait_until(
        "Reticulum-Go outbound Link",
        lambda: next(
            (event for event in go.events() if event.get("type") == "link.established"),
            None,
        ),
        timeout=20,
    )
    rust_link = wait_until(
        "LXMF-rs inbound Link",
        lambda: next(
            (
                item
                for item in rust.call("links")["links"]
                if item.get("link_id") == event["link_id"]
                and item.get("state") == "activated"
            ),
            None,
        ),
        timeout=20,
    )
    return {"link_id": event["link_id"], "lxmf_rs_state": rust_link["state"]}


def exchange_link_data(
    rust: RustProbe, go: ReticulumGoControl, link_id: str
) -> dict[str, Any]:
    go.events(clear=True)
    rust.events(clear=True)
    from_go = b"reticulum-go-link-data"
    go.command({"type": "link.send", "link_id": link_id, "data": b64(from_go)})
    rust_event(
        rust,
        lambda event: event.get("type") == "link"
        and event.get("event", {}).get("state") == "data",
        "Reticulum-Go Link data at LXMF-rs",
        timeout=20,
    )
    from_rust = b"lxmf-rs-link-data"
    rust.call("link_send", {"link_id": link_id, "data": b64(from_rust)})
    received = wait_until(
        "LXMF-rs Link data at Reticulum-Go",
        lambda: next(
            (
                event
                for event in go.events()
                if event.get("type") == "link.data"
                and event.get("link_id") == link_id
                and event.get("data") == b64(from_rust)
            ),
            None,
        ),
        timeout=20,
    )
    return {
        "reticulum_go_payload_sha256": sha256(from_go),
        "lxmf_rs_payload_sha256": sha256(base64.b64decode(received["data"])),
    }


def resource_go_to_rust(
    rust: RustProbe, go: ReticulumGoControl, link_id: str, payload: bytes
) -> dict[str, Any]:
    rust.events(clear=True)
    go.command(
        {"type": "link.send_resource", "link_id": link_id, "data": b64(payload)}
    )
    event = rust_event(
        rust,
        lambda event: event.get("type") == "resource"
        and event.get("link_id") == link_id
        and event.get("details", {}).get("state") == "complete"
        and event.get("details", {}).get("bytes") == len(payload)
        and event.get("details", {}).get("sha256") == sha256(payload),
        "Reticulum-Go Resource at LXMF-rs",
        timeout=60,
    )
    return {
        "received_bytes": event["details"]["bytes"],
        "received_sha256": event["details"]["sha256"],
    }


def resource_rust_to_go(
    rust: RustProbe, go: ReticulumGoControl, link_id: str, payload: bytes
) -> dict[str, Any]:
    rust.events(clear=True)
    go.events(clear=True)
    sent = rust.call("resource", {"link_id": link_id, "data": b64(payload)})
    event = wait_until(
        "LXMF-rs Resource at Reticulum-Go",
        lambda: next(
            (
                event
                for event in go.events()
                if event.get("type") == "resource.concluded"
                and event.get("link_id") == link_id
                and event.get("success") is True
                and event.get("data")
            ),
            None,
        ),
        # Reticulum-Go exposes large incoming Resources as a chain of roughly
        # 1 MiB transfers. The release profile's deterministic 50 MiB payload
        # takes about six minutes on the loopback reference topology.
        timeout=600 if len(payload) >= 50 * 1024 * 1024 else 240,
        interval=0.1,
    )
    received = base64.b64decode(event["data"])
    digest = sha256(received)
    if len(received) != len(payload) or digest != sha256(payload):
        raise AssertionError(
            f"Reticulum-Go Resource mismatch: bytes={len(received)}, sha256={digest}"
        )
    sender = rust_event(
        rust,
        lambda event: event.get("type") == "resource"
        and event.get("resource_hash") == sent["resource_hash"]
        and event.get("details", {}).get("state") == "outbound_complete",
        "LXMF-rs Resource proof from Reticulum-Go",
        timeout=30,
    )
    return {
        "resource_hash": sent["resource_hash"],
        "received_bytes": len(received),
        "received_sha256": digest,
        "sender_state": sender["details"]["state"],
    }


def link_rust_to_go(
    rust: RustProbe, go: ReticulumGoControl, go_destination: str
) -> dict[str, Any]:
    go.events(clear=True)
    created = rust.call("link", {"destination_hash": go_destination})
    link_id = created["link_id"]
    rust_link = wait_until(
        "LXMF-rs outbound Link to Reticulum-Go",
        lambda: next(
            (
                item
                for item in rust.call("links")["links"]
                if item.get("link_id") == link_id and item.get("state") == "activated"
            ),
            None,
        ),
        timeout=20,
    )
    go_link = wait_until(
        "Reticulum-Go inbound Link",
        lambda: next(
            (
                event
                for event in go.events()
                if event.get("type") == "link.established"
                and event.get("link_id") == link_id
            ),
            None,
        ),
        timeout=20,
    )
    return {
        "link_id": link_id,
        "lxmf_rs_state": rust_link["state"],
        "reticulum_go_state": go_link["type"],
    }


def close_link(
    rust: RustProbe, go: ReticulumGoControl, link_id: str
) -> dict[str, Any]:
    go.command({"type": "link.close", "link_id": link_id})
    go_closed = wait_until(
        "Reticulum-Go Link close",
        lambda: next(
            (
                event
                for event in go.events()
                if event.get("type") == "link.closed" and event.get("link_id") == link_id
            ),
            None,
        ),
        timeout=15,
    )
    rust_closed = wait_until(
        "LXMF-rs Link close",
        lambda: next(
            (
                item
                for item in rust.call("links")["links"]
                if item.get("link_id") == link_id and item.get("state") == "closed"
            ),
            None,
        ),
        timeout=15,
    )
    return {"go": go_closed["type"], "lxmf_rs": rust_closed["state"]}
