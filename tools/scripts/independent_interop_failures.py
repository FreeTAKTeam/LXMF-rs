#!/usr/bin/env python3
"""Restart and failure-recovery scenarios for the independent interop harness."""

from __future__ import annotations

import os
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

from independent_interop_support import (
    Evidence,
    ManagedProcess,
    RnsRsNode,
    RustProbe,
    free_port,
    sha256,
    wait_until,
)
from independent_interop_topology import (
    exchange_rust_endpoint_announces,
    packet_rust_via_rns_rs,
    rns_transport_ready,
    write_rns_transport_config,
)


RESTART_TOPOLOGY = "LXMF-rs — rns-rs — LXMF-rs restart topology"


def run_restart_scenarios(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    del level
    with restart_topology(root, repository_root, peer_root, peer_binary) as topology:
        evidence.run(
            "restart topology initial path establishment",
            "bidirectional",
            topology.exchange_announces,
            topology=RESTART_TOPOLOGY,
        )
        initial = b"before-restart"
        evidence.run(
            "traffic before restart",
            "left -> right",
            lambda: packet_rust_via_rns_rs(
                topology.endpoint_a,
                topology.endpoint_c,
                topology.destination_c,
                initial,
            ),
            topology=RESTART_TOPOLOGY,
            expected_bytes=len(initial),
            content_hash=sha256(initial),
        )
        endpoint = evidence.run(
            "endpoint restart, rediscovery and identity continuity",
            "right endpoint",
            topology.restart_endpoint_c,
            topology=RESTART_TOPOLOGY,
        )
        if endpoint is not None:
            endpoint_payload = b"after-endpoint-restart"
            evidence.run(
                "traffic resumes after endpoint restart",
                "left -> right",
                lambda: packet_rust_via_rns_rs(
                    topology.endpoint_a,
                    topology.endpoint_c,
                    topology.destination_c,
                    endpoint_payload,
                ),
                topology=RESTART_TOPOLOGY,
                expected_bytes=len(endpoint_payload),
                content_hash=sha256(endpoint_payload),
            )
        intermediary = evidence.run(
            "intermediate transport restart and rediscovery",
            "rns-rs intermediary",
            topology.restart_intermediary,
            topology=RESTART_TOPOLOGY,
        )
        if intermediary is not None:
            for direction, sender, receiver, destination, payload in (
                (
                    "left -> right",
                    topology.endpoint_a,
                    topology.endpoint_c,
                    topology.destination_c,
                    b"after-middle-restart-a-c",
                ),
                (
                    "right -> left",
                    topology.endpoint_c,
                    topology.endpoint_a,
                    topology.destination_a,
                    b"after-middle-restart-c-a",
                ),
            ):
                evidence.run(
                    "traffic resumes after intermediate restart",
                    direction,
                    lambda s=sender, r=receiver, d=destination, p=payload: (
                        packet_rust_via_rns_rs(s, r, d, p)
                    ),
                    topology=RESTART_TOPOLOGY,
                    expected_bytes=len(payload),
                    content_hash=sha256(payload),
                )


class RestartTopology:
    def __init__(
        self,
        root: Path,
        repository_root: Path,
        peer_root: Path,
        peer_binary: Path,
        port_ab: int,
        port_bc: int,
        control_a: int,
        control_b: int,
        control_c: int,
    ) -> None:
        self.root = root
        self.repository_root = repository_root
        self.peer_root = peer_root
        self.peer_binary = peer_binary
        self.port_ab = port_ab
        self.port_bc = port_bc
        self.control_a = control_a
        self.control_b = control_b
        self.control_c = control_c
        self.config = root / "config/restart-rns-rs"
        self.logs = root / "logs"
        self.process_a: ManagedProcess | None = None
        self.process_b: ManagedProcess | None = None
        self.process_c: ManagedProcess | None = None
        self.endpoint_a = RustProbe(control_a)
        self.endpoint_c = RustProbe(control_c)
        self.destination_a = ""
        self.destination_c = ""

    def rust_command(self, side: str) -> list[str]:
        control = self.control_a if side == "a" else self.control_c
        port = self.port_ab if side == "a" else self.port_bc
        return [
            str(self.repository_root / "target/release/independent-interop-node"),
            "--name",
            f"lxmf-rs-restart-{side}",
            "--identity-seed",
            f"lxmf-rs-independent-restart-{side}",
            "--control",
            f"127.0.0.1:{control}",
            "--listen",
            f"127.0.0.1:{port}",
        ]

    def start_endpoint(self, side: str, phase: str) -> ManagedProcess:
        process = ManagedProcess(
            f"LXMF-rs restart endpoint {side.upper()} ({phase})",
            self.rust_command(side),
            self.repository_root,
            self.logs / f"restart-lxmf-rs-{side}-{phase}.log",
            {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
        )
        control = self.control_a if side == "a" else self.control_c
        wait_until(
            f"restart endpoint {side} control",
            lambda: RustProbe(control).call("status"),
            timeout=15,
        )
        return process

    def start_intermediary(self, phase: str) -> ManagedProcess:
        process = ManagedProcess(
            f"rns-rs restart intermediary ({phase})",
            [
                str(self.peer_binary),
                "http",
                "--disable-auth",
                "--host",
                "127.0.0.1",
                "--port",
                str(self.control_b),
                "--config",
                str(self.config),
            ],
            self.peer_root,
            self.logs / f"restart-rns-rs-middle-{phase}.log",
            {"RUST_LOG": os.environ.get("LXMF_INTEROP_PEER_LOG", "info")},
        )
        middle = RnsRsNode(self.control_b)
        wait_until(
            "restarted rns-rs intermediary",
            lambda: rns_transport_ready(middle),
            timeout=30,
        )
        return process

    def exchange_announces(self) -> dict[str, Any]:
        return exchange_rust_endpoint_announces(
            self.endpoint_a,
            self.endpoint_c,
            self.destination_a,
            self.destination_c,
        )

    def restart_endpoint_c(self) -> dict[str, Any]:
        before = self.destination_c
        try:
            self.endpoint_c.call("shutdown")
        except Exception:
            pass
        if self.process_c is not None:
            self.process_c.stop()
        self.process_c = self.start_endpoint("c", "after-endpoint-restart")
        self.endpoint_c = RustProbe(self.control_c)
        status = self.endpoint_c.call("status")
        self.destination_c = status["destination_hash"]
        if self.destination_c != before:
            raise AssertionError(
                f"endpoint destination changed across restart: {before} -> {self.destination_c}"
            )
        self.endpoint_a.call("expire_path", {"destination_hash": self.destination_c})
        self.exchange_announces()
        return {"destination_before": before, "destination_after": self.destination_c}

    def restart_intermediary(self) -> dict[str, Any]:
        if self.process_b is not None:
            self.process_b.stop()
        self.process_b = self.start_intermediary("after-restart")
        for endpoint, destination in (
            (self.endpoint_a, self.destination_c),
            (self.endpoint_c, self.destination_a),
        ):
            endpoint.call("expire_path", {"destination_hash": destination})
        paths = self.exchange_announces()
        return {"rediscovered": paths}

    def stop(self) -> None:
        for control in (self.control_a, self.control_c):
            try:
                RustProbe(control).call("shutdown")
            except Exception:
                pass
        for process in (self.process_b, self.process_c, self.process_a):
            if process is not None:
                process.stop()


@contextmanager
def restart_topology(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
) -> Iterator[RestartTopology]:
    topology = RestartTopology(
        root,
        repository_root,
        peer_root,
        peer_binary,
        free_port(),
        free_port(),
        free_port(),
        free_port(),
        free_port(),
    )
    write_rns_transport_config(
        topology.config / "config", topology.port_ab, topology.port_bc
    )
    try:
        topology.process_a = topology.start_endpoint("a", "initial")
        topology.process_c = topology.start_endpoint("c", "initial")
        topology.destination_a = topology.endpoint_a.call("status")["destination_hash"]
        topology.destination_c = topology.endpoint_c.call("status")["destination_hash"]
        topology.process_b = topology.start_intermediary("initial")
        yield topology
    finally:
        topology.stop()
