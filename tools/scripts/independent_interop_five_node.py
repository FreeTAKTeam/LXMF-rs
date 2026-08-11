#!/usr/bin/env python3
"""Five-node all-LXMF-rs and mixed rns-rs topology evidence."""

from __future__ import annotations

import os
import time
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

from independent_interop_rns_rs import deterministic_payload, rust_event
from independent_interop_support import (
    Evidence,
    ManagedProcess,
    RnsRsNode,
    RustProbe,
    b64,
    free_port,
    sha256,
    wait_until,
)
from independent_interop_topology import (
    link_rust_via_rns_rs,
    resource_rust_via_rns_rs,
    rns_transport_ready,
    teardown_rust_link,
)


def run_five_node_scenarios(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    del level
    for mixed in (False, True):
        label = "mixed LXMF-rs/rns-rs" if mixed else "all LXMF-rs"
        topology_name = f"A — B — C — D — E ({label})"
        with five_node_topology(
            root, repository_root, peer_root, peer_binary, mixed
        ) as topology:
            discovered = evidence.run(
                "five-node path convergence",
                "bidirectional",
                topology.discover,
                topology=topology_name,
            )
            if discovered is None:
                continue
            for direction, sender, receiver, destination, payload in (
                (
                    "A -> E",
                    topology.endpoint_a,
                    topology.endpoint_e,
                    topology.destination_e,
                    b"five-node-a-to-e",
                ),
                (
                    "E -> A",
                    topology.endpoint_e,
                    topology.endpoint_a,
                    topology.destination_a,
                    b"five-node-e-to-a",
                ),
            ):
                evidence.run(
                    "five-node encrypted packet and proof",
                    direction,
                    lambda s=sender, r=receiver, d=destination, p=payload: packet_five_node(
                        s, r, d, p
                    ),
                    topology=topology_name,
                    expected_bytes=len(payload),
                    content_hash=sha256(payload),
                )
            link = evidence.run(
                "five-node Link establishment proof",
                "A -> E",
                lambda: link_rust_via_rns_rs(
                    topology.endpoint_a,
                    topology.endpoint_e,
                    topology.destination_e,
                ),
                topology=topology_name,
            )
            if link is not None:
                payload = deterministic_payload(1024 * 1024)
                for direction, sender, receiver in (
                    ("A -> E", topology.endpoint_a, topology.endpoint_e),
                    ("E -> A", topology.endpoint_e, topology.endpoint_a),
                ):
                    evidence.run(
                        "five-node Resource 1 MiB",
                        direction,
                        lambda s=sender, r=receiver: resource_rust_via_rns_rs(
                            s, r, link["link_id"], payload
                        ),
                        topology=topology_name,
                        expected_bytes=len(payload),
                        content_hash=sha256(payload),
                    )
                evidence.run(
                    "five-node Link teardown",
                    "bidirectional",
                    lambda: teardown_rust_link(
                        topology.endpoint_a, topology.endpoint_e, link["link_id"]
                    ),
                    topology=topology_name,
                )
            restarted = evidence.run(
                "five-node intermediate C restart",
                "C restart",
                topology.restart_middle,
                topology=topology_name,
            )
            if restarted is not None:
                recovered = b"five-node-after-c-restart"
                evidence.run(
                    "five-node path and traffic recovery",
                    "A -> E",
                    lambda: topology.recover(recovered),
                    topology=topology_name,
                    expected_bytes=len(recovered),
                    content_hash=sha256(recovered),
                )


class FiveNodeTopology:
    def __init__(
        self,
        root: Path,
        repository_root: Path,
        peer_root: Path,
        peer_binary: Path,
        mixed: bool,
    ) -> None:
        self.root = root
        self.repository_root = repository_root
        self.peer_root = peer_root
        self.peer_binary = peer_binary
        self.mixed = mixed
        self.ports = [free_port() for _ in range(4)]
        self.controls = [free_port() for _ in range(5)]
        self.logs = root / "logs"
        self.processes: dict[str, ManagedProcess] = {}
        self.endpoint_a = RustProbe(self.controls[0])
        self.endpoint_e = RustProbe(self.controls[4])
        self.destination_a = ""
        self.destination_e = ""
        self.middle_generation = 0
        self.config_c = root / "config/five-node-mixed-c"

    def rust_command(
        self,
        name: str,
        control: int,
        *,
        listens: tuple[int, ...] = (),
        connects: tuple[int, ...] = (),
        transport: bool = False,
    ) -> list[str]:
        command = [
            str(self.repository_root / "target/release/independent-interop-node"),
            "--name",
            f"lxmf-rs-five-{name}",
            "--identity-seed",
            f"lxmf-rs-independent-five-{name}",
            "--control",
            f"127.0.0.1:{control}",
        ]
        for port in listens:
            command.extend(("--listen", f"127.0.0.1:{port}"))
        for port in connects:
            command.extend(("--connect", f"127.0.0.1:{port}"))
        if transport:
            command.append("--transport")
        return command

    def start_rust(
        self,
        name: str,
        control: int,
        *,
        listens: tuple[int, ...] = (),
        connects: tuple[int, ...] = (),
        transport: bool = False,
        generation: int = 0,
    ) -> ManagedProcess:
        process = ManagedProcess(
            f"five-node LXMF-rs {name}",
            self.rust_command(
                name,
                control,
                listens=listens,
                connects=connects,
                transport=transport,
            ),
            self.repository_root,
            self.logs / f"five-node-{name}-{generation}.log",
            {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
        )
        wait_until(
            f"five-node {name} control",
            lambda: RustProbe(control).call("status"),
            timeout=15,
        )
        return process

    def write_mixed_config(self) -> None:
        self.config_c.mkdir(parents=True, exist_ok=True)
        (self.config_c / "config").write_text(
            "[reticulum]\n"
            "enable_transport = Yes\n"
            "share_instance = No\n\n"
            "[interfaces]\n"
            "  [[B]]\n"
            "    type = TCPClientInterface\n"
            "    enabled = Yes\n"
            "    target_host = 127.0.0.1\n"
            f"    target_port = {self.ports[1]}\n\n"
            "  [[D]]\n"
            "    type = TCPClientInterface\n"
            "    enabled = Yes\n"
            "    target_host = 127.0.0.1\n"
            f"    target_port = {self.ports[2]}\n",
            encoding="utf-8",
        )

    def start_middle(self) -> ManagedProcess:
        self.middle_generation += 1
        if not self.mixed:
            return self.start_rust(
                "c",
                self.controls[2],
                connects=(self.ports[1], self.ports[2]),
                transport=True,
                generation=self.middle_generation,
            )
        self.write_mixed_config()
        process = ManagedProcess(
            "five-node rns-rs C",
            [
                str(self.peer_binary),
                "http",
                "--disable-auth",
                "--host",
                "127.0.0.1",
                "--port",
                str(self.controls[2]),
                "--config",
                str(self.config_c),
            ],
            self.peer_root,
            self.logs / f"five-node-rns-rs-c-{self.middle_generation}.log",
            {"RUST_LOG": os.environ.get("LXMF_INTEROP_PEER_LOG", "info")},
        )
        middle = RnsRsNode(self.controls[2])
        wait_until(
            "five-node rns-rs C interfaces",
            lambda: rns_transport_ready(middle),
            timeout=20,
        )
        return process

    def start(self) -> None:
        self.processes["b"] = self.start_rust(
            "b",
            self.controls[1],
            listens=(self.ports[0], self.ports[1]),
            transport=True,
        )
        self.processes["d"] = self.start_rust(
            "d",
            self.controls[3],
            listens=(self.ports[2], self.ports[3]),
            transport=True,
        )
        self.processes["c"] = self.start_middle()
        self.processes["a"] = self.start_rust(
            "a", self.controls[0], connects=(self.ports[0],)
        )
        self.processes["e"] = self.start_rust(
            "e", self.controls[4], connects=(self.ports[3],)
        )
        self.destination_a = self.endpoint_a.call("status")["destination_hash"]
        self.destination_e = self.endpoint_e.call("status")["destination_hash"]
        time.sleep(1.0)

    def stop(self) -> None:
        for process in reversed(list(self.processes.values())):
            process.stop()

    def discover(self) -> dict[str, Any]:
        self.endpoint_a.events(clear=True)
        self.endpoint_e.events(clear=True)
        self.endpoint_e.call("announce", {"app_data": b64(b"five-node-e")})
        seen_a = rust_event(
            self.endpoint_a,
            lambda event: event.get("type") == "announce"
            and event.get("destination_hash") == self.destination_e
            and event.get("hops") == 4,
            "five-node E announce at A",
            timeout=45,
        )
        self.endpoint_a.call("announce", {"app_data": b64(b"five-node-a")})
        seen_e = rust_event(
            self.endpoint_e,
            lambda event: event.get("type") == "announce"
            and event.get("destination_hash") == self.destination_a
            and event.get("hops") == 4,
            "five-node A announce at E",
            timeout=45,
        )
        return {"a_hops": seen_a["hops"], "e_hops": seen_e["hops"]}

    def restart_middle(self) -> dict[str, Any]:
        old_pid = self.processes["c"].process.pid
        self.processes["c"].stop()
        self.processes["c"] = self.start_middle()
        return {"old_pid": old_pid, "new_pid": self.processes["c"].process.pid}

    def recover(self, payload: bytes) -> dict[str, Any]:
        self.endpoint_a.call("expire_path", {"destination_hash": self.destination_e})
        self.endpoint_e.call("expire_path", {"destination_hash": self.destination_a})
        convergence = self.discover()
        packet = packet_five_node(
            self.endpoint_a, self.endpoint_e, self.destination_e, payload
        )
        return {**convergence, **packet}


@contextmanager
def five_node_topology(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    mixed: bool,
) -> Iterator[FiveNodeTopology]:
    topology = FiveNodeTopology(root, repository_root, peer_root, peer_binary, mixed)
    try:
        topology.start()
        yield topology
    finally:
        topology.stop()


def packet_five_node(
    sender: RustProbe,
    receiver: RustProbe,
    destination: str,
    payload: bytes,
) -> dict[str, Any]:
    sender.events(clear=True)
    receiver.events(clear=True)
    sent = sender.call("send", {"destination_hash": destination, "data": b64(payload)})
    received = rust_event(
        receiver,
        lambda event: event.get("type") == "data"
        and event.get("destination_hash") == destination
        and event.get("data") == b64(payload)
        and event.get("sha256") == sha256(payload)
        and event.get("hops") == 4,
        "five-node packet",
        timeout=45,
    )
    receipt = rust_event(
        sender,
        lambda event: event.get("type") == "receipt"
        and event.get("packet_hash") == sent["packet_hash"],
        "five-node delivery proof",
        timeout=45,
    )
    return {
        "packet_hash": sent["packet_hash"],
        "proof_packet_hash": receipt["packet_hash"],
        "hops": received["hops"],
    }
