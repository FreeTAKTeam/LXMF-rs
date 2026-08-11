#!/usr/bin/env python3
"""Bounded HDLC-frame fault injection for independent interop evidence."""

from __future__ import annotations

import socket
import hashlib
import threading
import time
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterator

from independent_interop_rns_rs import (
    packet_rns_to_rust,
    resource_rns_to_rust,
    rns_link,
    write_rns_rs_config,
)
from independent_interop_support import (
    Evidence,
    ManagedProcess,
    RnsRsControl,
    RnsRsNode,
    RustProbe,
    b64,
    free_port,
    wait_until,
)


@dataclass
class FaultPolicy:
    drop_every: int = 0
    latency_seconds: float = 0.0
    duplicate_every: int = 0
    reorder_every: int = 0


class HdlcFaultProxy:
    def __init__(self, listen_port: int, target_port: int) -> None:
        self.listen_port = listen_port
        self.target_port = target_port
        self._policy = FaultPolicy()
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._listener: socket.socket | None = None
        self._sockets: list[socket.socket] = []
        self._thread: threading.Thread | None = None
        self._counters = {"frames": 0, "dropped": 0, "duplicated": 0, "reordered": 0}

    def start(self) -> None:
        listener = socket.socket()
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", self.listen_port))
        listener.listen(1)
        listener.settimeout(0.25)
        self._listener = listener
        self._thread = threading.Thread(target=self._accept, daemon=True)
        self._thread.start()

    def set_policy(self, policy: FaultPolicy) -> None:
        with self._lock:
            self._policy = policy
            self._counters = {"frames": 0, "dropped": 0, "duplicated": 0, "reordered": 0}

    def counters(self) -> dict[str, int]:
        with self._lock:
            return dict(self._counters)

    def stop(self) -> None:
        self._stop.set()
        for stream in self._sockets:
            try:
                stream.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            stream.close()
        if self._listener is not None:
            self._listener.close()
        if self._thread is not None:
            self._thread.join(timeout=2)

    def _accept(self) -> None:
        assert self._listener is not None
        while not self._stop.is_set():
            try:
                client, _ = self._listener.accept()
                target = socket.create_connection(("127.0.0.1", self.target_port), timeout=5)
            except (OSError, TimeoutError):
                continue
            self._sockets.extend((client, target))
            for source, destination, impaired in (
                (client, target, True),
                (target, client, False),
            ):
                threading.Thread(
                    target=self._pipe,
                    args=(source, destination, impaired),
                    daemon=True,
                ).start()
            return

    def _pipe(
        self, source: socket.socket, destination: socket.socket, impaired: bool
    ) -> None:
        frame = bytearray()
        started = False
        held: bytes | None = None
        try:
            while not self._stop.is_set():
                chunk = source.recv(64 * 1024)
                if not chunk:
                    break
                for byte in chunk:
                    if byte == 0x7E:
                        if started:
                            frame.append(byte)
                            if impaired:
                                held = self._emit(destination, bytes(frame), held)
                            else:
                                destination.sendall(bytes(frame))
                            frame = bytearray((0x7E,))
                        else:
                            frame = bytearray((0x7E,))
                            started = True
                    elif started:
                        frame.append(byte)
                    else:
                        destination.sendall(bytes((byte,)))
            if held is not None:
                destination.sendall(held)
        except OSError:
            return

    def _emit(
        self, destination: socket.socket, frame: bytes, held: bytes | None
    ) -> bytes | None:
        with self._lock:
            self._counters["frames"] += 1
            ordinal = self._counters["frames"]
            policy = self._policy
            if policy.drop_every and ordinal % policy.drop_every == 0:
                self._counters["dropped"] += 1
                return held
            duplicate = bool(policy.duplicate_every and ordinal % policy.duplicate_every == 0)
            reorder = bool(policy.reorder_every and ordinal % policy.reorder_every == 0)
            if duplicate:
                self._counters["duplicated"] += 1
            if reorder:
                self._counters["reordered"] += 1
        if policy.latency_seconds:
            time.sleep(policy.latency_seconds)
        if reorder and held is None:
            return frame
        destination.sendall(frame)
        if duplicate:
            destination.sendall(frame)
        if held is not None:
            destination.sendall(held)
            return None
        return held


@contextmanager
def chaos_session(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    control_binary: Path,
    label: str,
) -> Iterator[tuple[RustProbe, RnsRsNode, HdlcFaultProxy, str]]:
    rust_port, proxy_port, rust_control, http_port, peer_control = (
        free_port() for _ in range(5)
    )
    config_dir = root / "config" / label
    write_rns_rs_config(config_dir / "config", proxy_port)
    logs = root / "logs"
    rust_process = ManagedProcess(
        "LXMF-rs chaos endpoint",
        [
            str(repository_root / "target/release/independent-interop-node"),
            "--name",
            "lxmf-rs-chaos",
            "--identity-seed",
            "lxmf-rs-chaos",
            "--control",
            f"127.0.0.1:{rust_control}",
            "--listen",
            f"127.0.0.1:{rust_port}",
        ],
        repository_root,
        logs / f"lxmf-rs-chaos-{label}.log",
        {"RUST_LOG": "warn"},
    )
    proxy = HdlcFaultProxy(proxy_port, rust_port)
    proxy.start()
    peer_process: ManagedProcess | None = None
    try:
        rust = RustProbe(rust_control)
        rust_status = wait_until("LXMF-rs chaos control", lambda: rust.call("status"), timeout=15)
        peer_process = ManagedProcess(
            "rns-rs chaos endpoint",
            [
                str(control_binary),
                "http",
                "--disable-auth",
                "--host",
                "127.0.0.1",
                "--port",
                str(http_port),
                "--interop-control",
                f"127.0.0.1:{peer_control}",
                "--config",
                str(config_dir),
            ],
            peer_root,
            logs / f"rns-rs-chaos-{label}.log",
            {"RUST_LOG": "warn"},
        )
        rns = RnsRsNode(http_port)
        control = RnsRsControl(peer_control)
        wait_until("rns-rs chaos control", lambda: control.call("health"), timeout=15)
        wait_until("rns-rs chaos HTTP", lambda: rns.get("/health"), timeout=15)
        yield rust, rns, proxy, rust_status["destination_hash"]
    finally:
        try:
            RustProbe(rust_control).call("shutdown")
        except Exception:
            pass
        if peer_process is not None:
            peer_process.stop()
        proxy.stop()
        rust_process.stop()


def establish_link(rust: RustProbe, rns: RnsRsNode, destination: str) -> str:
    rns.post("/api/path/request", {"dest_hash": destination})
    wait_until(
        "chaos path",
        lambda: any(
            row.get("hash") == destination
            for row in rns.get(f"/api/paths?dest_hash={destination}")["paths"]
        ),
    )
    created = rns.post("/api/link", {"dest_hash": destination})
    rns_link(rns, created["link_id"])
    wait_until(
        "chaos Link at LXMF-rs",
        lambda: next(
            (
                row
                for row in rust.call("links")["links"]
                if row["link_id"] == created["link_id"] and row["state"] == "activated"
            ),
            None,
        ),
    )
    return str(created["link_id"])


def resource_with_faults(
    rust: RustProbe,
    rns: RnsRsNode,
    proxy: HdlcFaultProxy,
    link_id: str,
    policy: FaultPolicy,
    repeats: int = 1,
) -> dict[str, Any]:
    proxy.set_policy(policy)
    payload = chaos_payload(1_048_576)
    result: dict[str, Any] = {}
    for _ in range(repeats):
        result = resource_rns_to_rust(rust, rns, link_id, payload)
    counters = proxy.counters()
    if policy.drop_every and counters["dropped"] == 0:
        raise AssertionError("configured frame-loss workload did not inject a dropped frame")
    result.update(counters)
    result["bytes"] = len(payload)
    result["transfers"] = repeats
    return result


def chaos_payload(size: int) -> bytes:
    payload = bytearray()
    counter = 0
    seed = b"lxmf-rs-independent-chaos"
    while len(payload) < size:
        payload.extend(hashlib.sha256(seed + counter.to_bytes(8, "big")).digest())
        counter += 1
    return bytes(payload[:size])


def packet_with_latency(
    rust: RustProbe,
    rns: RnsRsNode,
    proxy: HdlcFaultProxy,
    destination: str,
    latency_ms: int,
) -> dict[str, Any]:
    proxy.set_policy(FaultPolicy(latency_seconds=latency_ms / 1000.0))
    started = time.monotonic()
    result = packet_rns_to_rust(rust, rns, destination)
    result["observed_seconds"] = time.monotonic() - started
    result.update(proxy.counters())
    return result


def run_resource_case(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    control_binary: Path,
    label: str,
    policy: FaultPolicy,
    repeats: int = 1,
) -> dict[str, Any]:
    with chaos_session(
        root, repository_root, peer_root, control_binary, label
    ) as (rust, rns, proxy, destination):
        link_id = establish_link(rust, rns, destination)
        return resource_with_faults(
            rust, rns, proxy, link_id, policy, repeats=repeats
        )


def run_latency_case(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    control_binary: Path,
    label: str,
    milliseconds: int,
) -> dict[str, Any]:
    with chaos_session(
        root, repository_root, peer_root, control_binary, label
    ) as (rust, rns, proxy, destination):
        establish_link(rust, rns, destination)
        return packet_with_latency(rust, rns, proxy, destination, milliseconds)


def run_resource_timeout_case(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    control_binary: Path,
) -> dict[str, Any]:
    with chaos_session(
        root, repository_root, peer_root, control_binary, "resource-timeout"
    ) as (rust, rns, proxy, destination):
        link_id = establish_link(rust, rns, destination)
        rns.get("/api/resource_events?clear=true")
        proxy.set_policy(FaultPolicy(drop_every=1))
        payload = chaos_payload(4096)
        started = time.monotonic()
        rns.post("/api/resource", {"link_id": link_id, "data": b64(payload)})
        event = wait_until(
            "rns-rs terminal Resource timeout",
            lambda: next(
                (
                    row
                    for row in rns.get("/api/resource_events")["resource_events"]
                    if row.get("link_id") == link_id and row.get("event_type") == "failed"
                ),
                None,
            ),
            timeout=30,
            interval=0.25,
        )
        counters = proxy.counters()
        if counters["dropped"] == 0:
            raise AssertionError("Resource timeout workload did not drop a frame")
        if "timeout" not in str(event.get("error", "")).lower():
            raise AssertionError(f"Resource failed without timeout classification: {event}")
        return {
            "elapsed_seconds": time.monotonic() - started,
            "peer_error": event["error"],
            "payload_bytes": len(payload),
            "payload_sha256": hashlib.sha256(payload).hexdigest(),
            **counters,
        }


def run_chaos_scenarios(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    _peer_binary: Path,
    control_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    losses = (1,) if level == "pr" else (1, 5, 10)
    payload_hash = hashlib.sha256(chaos_payload(1_048_576)).hexdigest()
    for percent in losses:
        evidence.run(
            f"Resource recovery with {percent}% deterministic frame loss",
            "rns-rs -> LXMF-rs",
            lambda value=percent: run_resource_case(
                root,
                repository_root,
                peer_root,
                control_binary,
                f"loss-{value}",
                FaultPolicy(drop_every=round(100 / value)),
                repeats=3 if value == 1 else 1,
            ),
            topology="rns-rs — HDLC fault proxy — LXMF-rs",
            expected_bytes=1_048_576,
            content_hash=payload_hash,
        )
    evidence.run(
        "Resource terminal timeout under complete frame loss",
        "rns-rs -> LXMF-rs",
        lambda: run_resource_timeout_case(
            root, repository_root, peer_root, control_binary
        ),
        topology="rns-rs — HDLC fault proxy — LXMF-rs",
    )
    latencies = (50,) if level == "pr" else (50, 250, 500)
    for milliseconds in latencies:
        evidence.run(
            f"packet and proof with {milliseconds} ms per-frame latency",
            "rns-rs -> LXMF-rs",
            lambda value=milliseconds: run_latency_case(
                root,
                repository_root,
                peer_root,
                control_binary,
                f"latency-{value}",
                value,
            ),
            topology="rns-rs — HDLC fault proxy — LXMF-rs",
        )
    if level in {"nightly", "release"}:
        evidence.run(
            "Resource robustness with duplicated frames",
            "rns-rs -> LXMF-rs",
            lambda: run_resource_case(
                root,
                repository_root,
                peer_root,
                control_binary,
                "duplicate",
                FaultPolicy(duplicate_every=3),
            ),
            topology="rns-rs — HDLC fault proxy — LXMF-rs",
            expected_bytes=1_048_576,
            content_hash=payload_hash,
        )
        evidence.run(
            "Resource robustness with reordered adjacent frames",
            "rns-rs -> LXMF-rs",
            lambda: run_resource_case(
                root,
                repository_root,
                peer_root,
                control_binary,
                "reorder",
                FaultPolicy(reorder_every=4),
            ),
            topology="rns-rs — HDLC fault proxy — LXMF-rs",
            expected_bytes=1_048_576,
            content_hash=payload_hash,
        )
