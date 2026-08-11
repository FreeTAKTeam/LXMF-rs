#!/usr/bin/env python3
"""Network-observable RNS 1.4.2 routing policy evidence."""

from __future__ import annotations

import os
import time
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

from independent_interop_rns_rs import ensure_rns_outbound, rust_event
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


GRAVITY_TOPOLOGY = "rns-rs — LXMF-rs transports — LXMF-rs selector"
BOUNDARY_TOPOLOGY = "rns-rs boundary — LXMF-rs — boundary/gateway endpoints"


def run_routing_scenarios(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    del level
    run_gravity_rebalance_and_failure(
        root, repository_root, peer_root, peer_binary, evidence
    )
    run_different_hop_gravity(root, repository_root, peer_root, peer_binary, evidence)
    run_boundary_requests(root, repository_root, peer_root, peer_binary, evidence)


def run_gravity_rebalance_and_failure(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
) -> None:
    with gravity_diamond(root, repository_root, peer_root, peer_binary) as (
        selector,
        peer,
        peer_destination,
        intermediates,
    ):
        initial = evidence.run(
            "same-hop path selection uses higher interface gravity",
            "rns-rs -> LXMF-rs",
            lambda: announce_and_select(selector, peer, peer_destination, 10),
            topology=GRAVITY_TOPOLOGY,
        )
        if initial is None:
            return
        evidence.run(
            "observable forwarding over gravity-selected path",
            "LXMF-rs -> rns-rs",
            lambda: packet_selector_to_peer(selector, peer, peer_destination, b"gravity-high"),
            topology=GRAVITY_TOPOLOGY,
            expected_bytes=len(b"gravity-high"),
            content_hash=sha256(b"gravity-high"),
        )
        changed = evidence.run(
            "dynamic path rebalancing after gravity change",
            "rns-rs -> LXMF-rs",
            lambda: rebalance_gravity(selector, peer, peer_destination),
            topology=GRAVITY_TOPOLOGY,
        )
        if changed is not None:
            evidence.run(
                "observable forwarding after dynamic rebalancing",
                "LXMF-rs -> rns-rs",
                lambda: packet_selector_to_peer(
                    selector, peer, peer_destination, b"gravity-rebalanced"
                ),
                topology=GRAVITY_TOPOLOGY,
                expected_bytes=len(b"gravity-rebalanced"),
                content_hash=sha256(b"gravity-rebalanced"),
            )
        failed = evidence.run(
            "interface failure selects viable alternate path",
            "LXMF-rs -> rns-rs",
            lambda: fail_selected_path(
                selector, intermediates, peer, peer_destination
            ),
            topology=GRAVITY_TOPOLOGY,
        )
        if failed is not None:
            evidence.run(
                "communication resumes after interface failure",
                "LXMF-rs -> rns-rs",
                lambda: packet_selector_to_peer(
                    selector, peer, peer_destination, b"gravity-fallback"
                ),
                topology=GRAVITY_TOPOLOGY,
                expected_bytes=len(b"gravity-fallback"),
                content_hash=sha256(b"gravity-fallback"),
            )


def run_different_hop_gravity(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
) -> None:
    with gravity_diamond(
        root,
        repository_root,
        peer_root,
        peer_binary,
        high_extra_hop=True,
    ) as (selector, peer, peer_destination, _intermediates):
        selected = evidence.run(
            "different hop count takes the shorter route despite higher alternate gravity",
            "rns-rs -> LXMF-rs",
            lambda: announce_select_hops(selector, peer, peer_destination, 1, 2),
            topology=GRAVITY_TOPOLOGY,
        )
        if selected is not None:
            payload = b"gravity-shorter-hop"
            evidence.run(
                "observable forwarding over shorter lower-gravity path",
                "LXMF-rs -> rns-rs",
                lambda: packet_selector_to_peer(
                    selector, peer, peer_destination, payload
                ),
                topology=GRAVITY_TOPOLOGY,
                expected_bytes=len(payload),
                content_hash=sha256(payload),
            )


@contextmanager
def gravity_diamond(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    high_extra_hop: bool = False,
) -> Iterator[tuple[RustProbe, RnsRsNode, str, list[RustProbe]]]:
    selector_ports = [free_port(), free_port()]
    peer_ports = [free_port(), free_port()]
    controls = [free_port(), free_port(), free_port()]
    if high_extra_hop:
        controls.append(free_port())
    peer_control = free_port()
    high_endpoint_port = free_port() if high_extra_hop else peer_ports[1]
    suffix = "different-hops" if high_extra_hop else "same-hops"
    config = root / f"config/routing-gravity-rns-rs-{suffix}/config"
    write_rns_clients(config, [("low gravity", peer_ports[0]), ("high gravity", high_endpoint_port)])
    logs = root / "logs"
    binary = repository_root / "target/release/independent-interop-node"
    processes: list[ManagedProcess] = []
    try:
        processes.append(
            ManagedProcess(
                "LXMF-rs gravity selector",
                [
                    str(binary),
                    "--name",
                    "lxmf-rs-gravity-selector",
                    "--identity-seed",
                    "lxmf-rs-independent-gravity-selector",
                    "--control",
                    f"127.0.0.1:{controls[0]}",
                    "--listen",
                    f"127.0.0.1:{selector_ports[0]}",
                    "--listen",
                    f"127.0.0.1:{selector_ports[1]}",
                    "--listen-gravity",
                    "1",
                    "--listen-gravity",
                    "10",
                ],
                repository_root,
                logs / f"routing-gravity-selector-{suffix}.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
            )
        )
        selector = RustProbe(controls[0])
        wait_until("gravity selector control", lambda: selector.call("status"), timeout=15)
        for index, label in enumerate(("low", "high")):
            processes.append(
                ManagedProcess(
                    f"LXMF-rs {label}-gravity transport",
                    [
                        str(binary),
                        "--name",
                        f"lxmf-rs-gravity-{label}",
                        "--identity-seed",
                        f"lxmf-rs-independent-gravity-{label}",
                        "--control",
                        f"127.0.0.1:{controls[index + 1]}",
                        "--connect",
                        f"127.0.0.1:{selector_ports[index]}",
                        "--listen",
                        f"127.0.0.1:{peer_ports[index]}",
                        "--transport",
                    ],
                    repository_root,
                    logs / f"routing-gravity-{label}-transport-{suffix}.log",
                    {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
                )
            )
            wait_until(
                f"{label}-gravity transport control",
                lambda port=controls[index + 1]: RustProbe(port).call("status"),
                timeout=15,
            )
        if high_extra_hop:
            processes.append(
                ManagedProcess(
                    "LXMF-rs high-gravity extra-hop transport",
                    [
                        str(binary),
                        "--name",
                        "lxmf-rs-gravity-high-extra-hop",
                        "--identity-seed",
                        "lxmf-rs-independent-gravity-high-extra-hop",
                        "--control",
                        f"127.0.0.1:{controls[3]}",
                        "--connect",
                        f"127.0.0.1:{peer_ports[1]}",
                        "--listen",
                        f"127.0.0.1:{high_endpoint_port}",
                        "--transport",
                    ],
                    repository_root,
                    logs / f"routing-gravity-high-extra-hop-{suffix}.log",
                    {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
                )
            )
            wait_until(
                "high-gravity extra-hop transport control",
                lambda: RustProbe(controls[3]).call("status"),
                timeout=15,
            )
        processes.append(
            ManagedProcess(
                "rns-rs gravity endpoint",
                [
                    str(peer_binary),
                    "http",
                    "--disable-auth",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    str(peer_control),
                    "--config",
                    str(config.parent),
                ],
                peer_root,
                logs / f"routing-gravity-rns-rs-{suffix}.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_PEER_LOG", "info")},
            )
        )
        peer = RnsRsNode(peer_control)
        wait_until("rns-rs gravity health", lambda: peer.get("/health"), timeout=15)
        wait_until(
            "rns-rs gravity interfaces",
            lambda: peer_interfaces_ready(peer, 2),
            timeout=20,
        )
        wait_until(
            "selector gravity child interfaces",
            lambda: selector_interfaces_ready(selector, 4),
            timeout=20,
        )
        destination = peer.post(
            "/api/destination",
            {
                "type": "single",
                "app_name": "interop",
                "aspects": ["probe"],
                "direction": "in",
                "proof_strategy": "all",
            },
        )["dest_hash"]
        yield selector, peer, destination, [RustProbe(port) for port in controls[1:]]
    finally:
        for control in controls:
            try:
                RustProbe(control).call("shutdown")
            except Exception:
                pass
        for process in reversed(processes):
            process.stop()


def write_rns_clients(path: Path, clients: list[tuple[str, int]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    interfaces = []
    for name, port in clients:
        interfaces.extend(
            [
                f"  [[{name}]]\n",
                "    type = TCPClientInterface\n",
                "    enabled = Yes\n",
                "    target_host = 127.0.0.1\n",
                f"    target_port = {port}\n\n",
            ]
        )
    path.write_text(
        "[reticulum]\n"
        "enable_transport = No\n"
        "share_instance = No\n\n"
        "[interfaces]\n"
        + "".join(interfaces),
        encoding="utf-8",
    )


def peer_interfaces_ready(peer: RnsRsNode, count: int) -> dict[str, Any] | None:
    status = peer.get("/api/interfaces")
    online = [item for item in status.get("interfaces", []) if item.get("status") == "up"]
    return {"online": len(online)} if len(online) >= count else None


def selector_interfaces_ready(selector: RustProbe, count: int) -> dict[str, Any] | None:
    interfaces = selector.call("interfaces")["interfaces"]
    return {"interfaces": interfaces} if len(interfaces) >= count else None


def path_with_gravity(
    selector: RustProbe, destination: str, gravity: int
) -> dict[str, Any] | None:
    path = selector.call("has_path", {"destination_hash": destination})
    if not path.get("path_found"):
        return None
    interface = next(
        (
            item
            for item in selector.call("interfaces")["interfaces"]
            if item.get("address") == path.get("interface")
        ),
        None,
    )
    if interface is None or interface.get("gravity") != gravity:
        return None
    return {"path": path, "interface_policy": interface}


def announce_and_select(
    selector: RustProbe, peer: RnsRsNode, destination: str, gravity: int
) -> dict[str, Any]:
    selector.events(clear=True)
    app_data = f"gravity-{gravity}".encode()
    peer.post("/api/announce", {"dest_hash": destination, "app_data": b64(app_data)})
    rust_event(
        selector,
        lambda event: event.get("type") == "announce"
        and event.get("destination_hash") == destination,
        "gravity announce at selector",
        timeout=30,
    )
    selected = wait_until(
        f"gravity {gravity} route selection",
        lambda: path_with_gravity(selector, destination, gravity),
        timeout=30,
    )
    return selected


def announce_select_hops(
    selector: RustProbe,
    peer: RnsRsNode,
    destination: str,
    gravity: int,
    hops: int,
) -> dict[str, Any]:
    selected = announce_and_select(selector, peer, destination, gravity)
    if selected["path"].get("hops") != hops:
        raise AssertionError(f"expected {hops}-hop path, got {selected['path']!r}")
    return selected


def packet_selector_to_peer(
    selector: RustProbe,
    peer: RnsRsNode,
    destination: str,
    payload: bytes,
) -> dict[str, Any]:
    selector.events(clear=True)
    peer.get("/api/packets?clear=true")
    sent = selector.call("send", {"destination_hash": destination, "data": b64(payload)})
    packet = wait_until(
        "gravity-routed packet at rns-rs",
        lambda: next(
            (
                item
                for item in peer.get("/api/packets")["packets"]
                if item.get("dest_hash") == destination
            ),
            None,
        ),
        timeout=30,
    )
    deadline = time.monotonic() + 3.0
    receipt = None
    while time.monotonic() < deadline and receipt is None:
        receipt = next(
            (
                event
                for event in selector.events()
                if event.get("type") == "receipt"
                and event.get("packet_hash") == sent["packet_hash"]
            ),
            None,
        )
        if receipt is None:
            time.sleep(0.1)
    return {
        "packet_hash": packet["packet_hash"],
        "proof_observed": receipt is not None,
        "proof_packet_hash": None if receipt is None else receipt["packet_hash"],
    }


def rebalance_gravity(
    selector: RustProbe, peer: RnsRsNode, destination: str
) -> dict[str, Any]:
    before = selector.call("has_path", {"destination_hash": destination})
    for interface in selector.call("interfaces")["interfaces"]:
        gravity = interface.get("gravity")
        replacement = 20 if gravity == 1 else 0 if gravity == 10 else None
        if replacement is not None:
            selector.call(
                "set_interface_policy",
                {"interface": interface["address"], "gravity": replacement},
            )
    selected = announce_and_select(selector, peer, destination, 20)
    if selected["path"]["interface"] == before.get("interface"):
        raise AssertionError("gravity change did not replace the selected path interface")
    return {"before": before, "after": selected}


def fail_selected_path(
    selector: RustProbe,
    intermediates: list[RustProbe],
    peer: RnsRsNode,
    destination: str,
) -> dict[str, Any]:
    selected = path_with_gravity(selector, destination, 20)
    if selected is None:
        raise AssertionError("expected gravity-20 path before interface failure")
    selected_interface = selected["path"]["interface"]
    selected_next_hop = selected["path"]["next_hop"]
    failed_intermediate = next(
        (
            intermediate
            for intermediate in intermediates
            if intermediate.call("status")["identity_hash"] == selected_next_hop
        ),
        None,
    )
    if failed_intermediate is None:
        raise AssertionError(
            f"no intermediary owns selected next hop {selected_next_hop}"
        )

    # Stopping only the selector's accepted TCP child lets the connecting
    # intermediary immediately create a replacement child.  Depending on the
    # scheduler, that can race the viable alternate route.  Shut down the
    # intermediary that owns the selected next hop so the interface failure is
    # persistent for the remainder of this scenario.
    failed_intermediate.call("shutdown")
    wait_until(
        "failed intermediary shutdown",
        lambda: probe_unavailable(failed_intermediate),
        timeout=10,
    )
    wait_until(
        "selected interface removal",
        lambda: all(
            interface["address"] != selected_interface
            for interface in selector.call("interfaces")["interfaces"]
        ),
        timeout=10,
    )
    selector.call("expire_path", {"destination_hash": destination})
    expired_intermediates = 0
    for intermediate in intermediates:
        if intermediate is failed_intermediate:
            continue
        expired_intermediates += int(
            intermediate.call(
                "expire_path", {"destination_hash": destination}
            )["expired"]
        )
    selector.events(clear=True)
    announcements_sent = 0
    fallback = None
    deadline = time.monotonic() + 30.0
    while time.monotonic() < deadline and fallback is None:
        peer.post(
            "/api/announce",
            {
                "dest_hash": destination,
                "app_data": b64(b"gravity-fallback"),
            },
        )
        announcements_sent += 1
        retry_deadline = min(deadline, time.monotonic() + 2.0)
        while time.monotonic() < retry_deadline:
            fallback = path_with_gravity(selector, destination, 0)
            if fallback is not None:
                break
            time.sleep(0.1)
    if fallback is None:
        raise TimeoutError("timed out waiting for gravity 0 route selection")
    return {
        "stopped_interfaces": [selected_interface],
        "failed_next_hop": selected_next_hop,
        "fallback": fallback,
        "announcements_sent": announcements_sent,
        "expired_intermediates": expired_intermediates,
    }


def probe_unavailable(probe: RustProbe) -> bool:
    try:
        probe.call("status")
    except Exception:
        return True
    return False


def run_boundary_requests(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
) -> None:
    for label in ("boundary", "gateway"):
        with boundary_topology(
            root, repository_root, peer_root, peer_binary, label
        ) as (requester, endpoints):
            endpoint, destination = endpoints[label]
            evidence.run(
                f"boundary path request to {label} interface",
                "rns-rs -> LXMF-rs",
                lambda endpoint=endpoint, destination=destination: boundary_path_and_packet(
                    requester, endpoint, destination, label
                ),
                topology=BOUNDARY_TOPOLOGY,
            )
    with boundary_topology(
        root, repository_root, peer_root, peer_binary, "full-suppression"
    ) as (requester, endpoints):
        evidence.run(
            "boundary path request suppresses full-interface recursion",
            "rns-rs -> LXMF-rs",
            lambda: assert_boundary_full_suppressed(requester, endpoints["full"][1]),
            topology=BOUNDARY_TOPOLOGY,
        )


@contextmanager
def boundary_topology(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    session: str,
) -> Iterator[tuple[RnsRsNode, dict[str, tuple[RustProbe, str]]]]:
    modes = ["boundary", "boundary", "gateway", "full"]
    ports = [free_port() for _ in modes]
    middle_control = free_port()
    endpoint_controls = [free_port() for _ in range(3)]
    requester_control = free_port()
    config = root / f"config/routing-boundary-rns-rs-{session}/config"
    write_rns_clients(config, [("boundary requester", ports[0])])
    logs = root / "logs"
    binary = repository_root / "target/release/independent-interop-node"
    middle_command = [
        str(binary),
        "--name",
        "lxmf-rs-boundary-middle",
        "--identity-seed",
        "lxmf-rs-independent-boundary-middle",
        "--control",
        f"127.0.0.1:{middle_control}",
        "--transport",
    ]
    for port, mode in zip(ports, modes):
        middle_command.extend(
            ["--listen", f"127.0.0.1:{port}", "--listen-mode", mode]
        )
    processes: list[ManagedProcess] = []
    try:
        processes.append(
            ManagedProcess(
                "LXMF-rs boundary middle",
                middle_command,
                repository_root,
                logs / f"routing-boundary-middle-{session}.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
            )
        )
        wait_until(
            "boundary middle control",
            lambda: RustProbe(middle_control).call("status"),
            timeout=15,
        )
        endpoints: dict[str, tuple[RustProbe, str]] = {}
        for index, label in enumerate(("boundary", "gateway", "full")):
            control = endpoint_controls[index]
            processes.append(
                ManagedProcess(
                    f"LXMF-rs {label} endpoint",
                    [
                        str(binary),
                        "--name",
                        f"lxmf-rs-{label}-endpoint",
                        "--identity-seed",
                        f"lxmf-rs-independent-{label}-endpoint",
                        "--control",
                        f"127.0.0.1:{control}",
                        "--connect",
                        f"127.0.0.1:{ports[index + 1]}",
                    ],
                    repository_root,
                    logs / f"routing-{label}-endpoint-{session}.log",
                    {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
                )
            )
            probe = RustProbe(control)
            status = wait_until(
                f"{label} endpoint control", lambda probe=probe: probe.call("status"), timeout=15
            )
            endpoints[label] = (probe, status["destination_hash"])
        processes.append(
            ManagedProcess(
                "rns-rs boundary requester",
                [
                    str(peer_binary),
                    "http",
                    "--disable-auth",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    str(requester_control),
                    "--config",
                    str(config.parent),
                ],
                peer_root,
                logs / f"routing-boundary-rns-rs-{session}.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_PEER_LOG", "info")},
            )
        )
        requester = RnsRsNode(requester_control)
        wait_until("rns-rs boundary health", lambda: requester.get("/health"), timeout=15)
        wait_until(
            "boundary topology interfaces",
            lambda: selector_interfaces_ready(RustProbe(middle_control), 8),
            timeout=20,
        )
        yield requester, endpoints
    finally:
        for control in [middle_control, *endpoint_controls]:
            try:
                RustProbe(control).call("shutdown")
            except Exception:
                pass
        for process in reversed(processes):
            process.stop()


def boundary_path_and_packet(
    requester: RnsRsNode,
    endpoint: RustProbe,
    destination: str,
    label: str,
) -> dict[str, Any]:
    endpoint.events(clear=True)
    requester.get("/api/proofs?clear=true")
    requester.post("/api/path/request", {"dest_hash": destination})
    path = wait_until(
        f"rns-rs {label} path",
        lambda: next(
            (
                item
                for item in requester.get(f"/api/paths?dest_hash={destination}")["paths"]
                if item.get("hash") == destination
            ),
            None,
        ),
        timeout=30,
    )
    ensure_rns_outbound(requester, destination)
    payload = f"boundary-to-{label}".encode()
    sent = requester.post("/api/send", {"dest_hash": destination, "data": b64(payload)})
    received = rust_event(
        endpoint,
        lambda event: event.get("type") == "data"
        and event.get("destination_hash") == destination
        and event.get("data") == b64(payload),
        f"boundary-routed packet at {label} endpoint",
        timeout=30,
    )
    proof = wait_until(
        f"boundary-routed proof at rns-rs for {label}",
        lambda: next(
            (
                item
                for item in requester.get("/api/proofs")["proofs"]
                if item.get("packet_hash") == sent["packet_hash"]
            ),
            None,
        ),
        timeout=30,
    )
    return {
        "path_hops": path["hops"],
        "packet_hops": received["hops"],
        "proof_rtt_seconds": proof["rtt"],
        "payload_sha256": sha256(payload),
    }


def assert_boundary_full_suppressed(requester: RnsRsNode, destination: str) -> dict[str, Any]:
    requester.post("/api/path/request", {"dest_hash": destination})
    started = time.monotonic()
    while time.monotonic() - started < 3.0:
        paths = requester.get(f"/api/paths?dest_hash={destination}")["paths"]
        if any(item.get("hash") == destination for item in paths):
            raise AssertionError("boundary request incorrectly recursed onto full interface")
        time.sleep(0.1)
    return {"suppression_observed_seconds": round(time.monotonic() - started, 3)}
