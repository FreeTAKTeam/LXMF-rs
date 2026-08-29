#!/usr/bin/env python3
"""Mixed implementation topology scenarios for independent interop evidence."""

from __future__ import annotations

import base64
from contextlib import contextmanager
import os
from pathlib import Path
from typing import Any, Iterator

from independent_interop_rns_rs import (
    close_bidirectional_link,
    deterministic_payload,
    ensure_rns_outbound,
    resource_rns_to_rust,
    rns_link,
    rust_event,
)
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


TOPOLOGY_RUST_RUST_RNS = "LXMF-rs — LXMF-rs — rns-rs"
TOPOLOGY_RUST_RNS_RUST = "LXMF-rs — rns-rs — LXMF-rs"


def run_multi_hop(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    del level
    with rust_rust_rns_topology(
        root, repository_root, peer_root, peer_binary
    ) as (endpoint_a, endpoint_c, destination_a, destination_c):
        evidence.run(
            "multi-hop announce propagation and path establishment",
            "bidirectional",
            lambda: exchange_multi_hop_announces(
                endpoint_a, endpoint_c, destination_a, destination_c
            ),
            topology=TOPOLOGY_RUST_RUST_RNS,
        )
        evidence.run(
            "multi-hop encrypted packet and delivery proof",
            "rns-rs -> LXMF-rs",
            lambda: packet_c_to_a(endpoint_a, endpoint_c, destination_a),
            topology=TOPOLOGY_RUST_RUST_RNS,
            expected_bytes=len(b"multi-hop-rns-rs-to-lxmf-rs"),
            content_hash=sha256(b"multi-hop-rns-rs-to-lxmf-rs"),
        )
        packet_to_c = evidence.run(
            "multi-hop encrypted packet",
            "LXMF-rs -> rns-rs",
            lambda: packet_a_to_c(endpoint_a, endpoint_c, destination_c),
            topology=TOPOLOGY_RUST_RUST_RNS,
            expected_bytes=len(b"multi-hop-lxmf-rs-to-rns-rs"),
            content_hash=sha256(b"multi-hop-lxmf-rs-to-rns-rs"),
        )
        if packet_to_c is not None:
            evidence.run(
                "multi-hop delivery proof",
                "rns-rs -> LXMF-rs",
                lambda: proof_c_to_a(endpoint_a, packet_to_c["packet_hash"]),
                topology=TOPOLOGY_RUST_RUST_RNS,
            )
        link = evidence.run(
            "multi-hop Link establishment proof RTT",
            "rns-rs -> LXMF-rs",
            lambda: link_c_to_a(endpoint_a, endpoint_c, destination_a),
            topology=TOPOLOGY_RUST_RUST_RNS,
        )
        if link is None:
            return
        link_id = link["link_id"]
        payload = deterministic_payload(1024 * 1024)
        digest = sha256(payload)
        evidence.run(
            "multi-hop Resource 1 MiB",
            "rns-rs -> LXMF-rs",
            lambda: resource_rns_to_rust(endpoint_a, endpoint_c, link_id, payload),
            topology=TOPOLOGY_RUST_RUST_RNS,
            expected_bytes=len(payload),
            content_hash=digest,
        )
        evidence.run(
            "multi-hop Resource 1 MiB",
            "LXMF-rs -> rns-rs",
            lambda: resource_a_to_c(endpoint_a, endpoint_c, link_id, payload),
            topology=TOPOLOGY_RUST_RUST_RNS,
            expected_bytes=len(payload),
            content_hash=digest,
        )
        close_bidirectional_link(evidence, endpoint_a, endpoint_c, link_id)
        evidence.scenarios[-1]["topology"] = TOPOLOGY_RUST_RUST_RNS

    run_rns_rs_transport_topology(root, repository_root, peer_root, peer_binary, evidence)


@contextmanager
def rust_rust_rns_topology(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
) -> Iterator[tuple[RustProbe, RnsRsNode, str, str]]:
    port_ab = free_port()
    port_bc = free_port()
    control_a = free_port()
    control_b = free_port()
    control_c = free_port()
    config_c = root / "config" / "multi-hop-rust-rust-rns-c"
    write_rns_client_config(config_c / "config", port_bc)
    logs = root / "logs"
    processes: list[ManagedProcess] = []
    try:
        processes.append(
            ManagedProcess(
                "LXMF-rs transport B",
                [
                    str(repository_root / "target/release/independent-interop-node"),
                    "--name",
                    "lxmf-rs-multi-hop-b",
                    "--identity-seed",
                    "lxmf-rs-independent-multi-hop-b",
                    "--control",
                    f"127.0.0.1:{control_b}",
                    "--listen",
                    f"127.0.0.1:{port_ab}",
                    "--listen",
                    f"127.0.0.1:{port_bc}",
                    "--transport",
                ],
                repository_root,
                logs / "multi-hop-lxmf-rs-b.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
            )
        )
        middle = RustProbe(control_b)
        wait_until("LXMF-rs middle control", lambda: middle.call("status"), timeout=15)
        processes.append(
            ManagedProcess(
                "LXMF-rs endpoint A",
                [
                    str(repository_root / "target/release/independent-interop-node"),
                    "--name",
                    "lxmf-rs-multi-hop-a",
                    "--identity-seed",
                    "lxmf-rs-independent-multi-hop-a",
                    "--control",
                    f"127.0.0.1:{control_a}",
                    "--connect",
                    f"127.0.0.1:{port_ab}",
                ],
                repository_root,
                logs / "multi-hop-lxmf-rs-a.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
            )
        )
        endpoint_a = RustProbe(control_a)
        status_a = wait_until(
            "LXMF-rs endpoint control", lambda: endpoint_a.call("status"), timeout=15
        )
        processes.append(
            ManagedProcess(
                "rns-rs endpoint C",
                [
                    str(peer_binary),
                    "http",
                    "--disable-auth",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    str(control_c),
                    "--config",
                    str(config_c),
                ],
                peer_root,
                logs / "multi-hop-rns-rs-c.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_PEER_LOG", "info")},
            )
        )
        endpoint_c = RnsRsNode(control_c)
        wait_until("rns-rs endpoint health", lambda: endpoint_c.get("/health"), timeout=15)
        destination_c = endpoint_c.post(
            "/api/destination",
            {
                "type": "single",
                "app_name": "interop",
                "aspects": ["probe"],
                "direction": "in",
                "proof_strategy": "all",
            },
        )["dest_hash"]
        yield endpoint_a, endpoint_c, status_a["destination_hash"], destination_c
    finally:
        for control in (control_a, control_b):
            try:
                RustProbe(control).call("shutdown")
            except Exception:
                pass
        for process in reversed(processes):
            process.stop()


def write_rns_client_config(path: Path, target_port: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "[reticulum]\n"
        "enable_transport = No\n"
        "share_instance = No\n\n"
        "[interfaces]\n"
        "  [[LXMF-rs transport B]]\n"
        "    type = TCPClientInterface\n"
        "    enabled = Yes\n"
        "    target_host = 127.0.0.1\n"
        f"    target_port = {target_port}\n",
        encoding="utf-8",
    )


def exchange_multi_hop_announces(
    endpoint_a: RustProbe,
    endpoint_c: RnsRsNode,
    destination_a: str,
    destination_c: str,
) -> dict[str, Any]:
    endpoint_a.events(clear=True)
    endpoint_c.get("/api/announces?clear=true")
    app_a = b"multi-hop-lxmf-rs-a"
    app_c = b"multi-hop-rns-rs-c"
    endpoint_a.call("announce", {"app_data": b64(app_a)})
    endpoint_c.post(
        "/api/announce", {"dest_hash": destination_c, "app_data": b64(app_c)}
    )
    seen_at_a = rust_event(
        endpoint_a,
        lambda event: event.get("type") == "announce"
        and event.get("destination_hash") == destination_c
        and event.get("app_data") == b64(app_c)
        and event.get("hops") == 2,
        "two-hop rns-rs announce at LXMF-rs endpoint",
        timeout=30,
    )
    seen_at_c = wait_until(
        "two-hop LXMF-rs announce at rns-rs endpoint",
        lambda: next(
            (
                item
                for item in endpoint_c.get("/api/announces")["announces"]
                if item.get("dest_hash") == destination_a
                and item.get("app_data") == b64(app_a)
                and item.get("hops") == 2
            ),
            None,
        ),
        timeout=30,
    )
    path_a = endpoint_a.call("has_path", {"destination_hash": destination_c})
    path_c = next(
        item
        for item in endpoint_c.get(f"/api/paths?dest_hash={destination_a}")["paths"]
        if item.get("hash") == destination_a
    )
    if path_a.get("hops") != 2 or path_c.get("hops") != 2:
        raise AssertionError(f"unexpected multi-hop paths: {path_a!r}, {path_c!r}")
    return {
        "lxmf_rs_hops": seen_at_a["hops"],
        "rns_rs_hops": seen_at_c["hops"],
        "observable_path_hops": 2,
    }


def packet_c_to_a(
    endpoint_a: RustProbe,
    endpoint_c: RnsRsNode,
    destination_a: str,
) -> dict[str, Any]:
    payload = b"multi-hop-rns-rs-to-lxmf-rs"
    endpoint_a.events(clear=True)
    endpoint_c.get("/api/proofs?clear=true")
    ensure_rns_outbound(endpoint_c, destination_a)
    sent = endpoint_c.post(
        "/api/send", {"dest_hash": destination_a, "data": b64(payload)}
    )
    event = rust_event(
        endpoint_a,
        lambda item: item.get("type") == "data"
        and item.get("data") == b64(payload)
        and item.get("hops") == 2,
        "two-hop rns-rs packet at LXMF-rs endpoint",
        timeout=30,
    )
    proof = wait_until(
        "two-hop delivery proof at rns-rs endpoint",
        lambda: next(
            (
                item
                for item in endpoint_c.get("/api/proofs")["proofs"]
                if item.get("packet_hash") == sent["packet_hash"]
            ),
            None,
        ),
        timeout=30,
    )
    return {
        "packet_hash": sent["packet_hash"],
        "proof_rtt_seconds": proof["rtt"],
        "hops": event["hops"],
    }


def packet_a_to_c(
    endpoint_a: RustProbe,
    endpoint_c: RnsRsNode,
    destination_c: str,
) -> dict[str, Any]:
    payload = b"multi-hop-lxmf-rs-to-rns-rs"
    endpoint_a.events(clear=True)
    endpoint_c.get("/api/packets?clear=true")
    result = endpoint_a.call(
        "send", {"destination_hash": destination_c, "data": b64(payload)}
    )
    packet = wait_until(
        "two-hop LXMF-rs packet at rns-rs endpoint",
        lambda: next(
            (
                item
                for item in endpoint_c.get("/api/packets")["packets"]
                if item.get("dest_hash") == destination_c
            ),
            None,
        ),
        timeout=30,
    )
    return {
        "packet_hash": result["packet_hash"],
        "peer_packet_hash": packet["packet_hash"],
    }


def proof_c_to_a(endpoint_a: RustProbe, packet_hash: str) -> dict[str, Any]:
    receipt = rust_event(
        endpoint_a,
        lambda item: item.get("type") == "receipt"
        and item.get("packet_hash") == packet_hash,
        "two-hop delivery proof at LXMF-rs endpoint",
        timeout=30,
    )
    return {"proof_packet_hash": receipt["packet_hash"]}


def link_c_to_a(
    endpoint_a: RustProbe,
    endpoint_c: RnsRsNode,
    destination_a: str,
) -> dict[str, Any]:
    created = endpoint_c.post("/api/link", {"dest_hash": destination_a})
    link = rns_link(endpoint_c, created["link_id"])
    rust_link = wait_until(
        "two-hop inbound Link active at LXMF-rs endpoint",
        lambda: next(
            (
                item
                for item in endpoint_a.call("links")["links"]
                if item.get("link_id") == created["link_id"]
                and item.get("state") == "activated"
            ),
            None,
        ),
        timeout=30,
    )
    return {
        "link_id": created["link_id"],
        "rtt_seconds": link["rtt"],
        "hops": 2,
        "lxmf_rs_state": rust_link["state"],
    }


def resource_a_to_c(
    endpoint_a: RustProbe,
    endpoint_c: RnsRsNode,
    link_id: str,
    payload: bytes,
) -> dict[str, Any]:
    endpoint_a.events(clear=True)
    endpoint_c.get("/api/resource_events?clear=true")
    sent = endpoint_a.call("resource", {"link_id": link_id, "data": b64(payload)})

    def result() -> dict[str, Any] | None:
        peer_events = endpoint_c.get("/api/resource_events")["resource_events"]
        completed = next(
            (
                item
                for item in peer_events
                if item.get("link_id") == link_id
                and item.get("event_type") == "received"
                and item.get("data_base64")
            ),
            None,
        )
        if completed is not None:
            received = base64.b64decode(completed["data_base64"])
            digest = sha256(received)
            if len(received) != len(payload) or digest != sha256(payload):
                return {
                    "failure": (
                        f"rns-rs Resource mismatch: bytes={len(received)}, "
                        f"sha256={digest}"
                    )
                }
            return {
                "resource_hash": sent["resource_hash"],
                "received_bytes": len(received),
                "received_sha256": digest,
            }

        rust_failure = next(
            (
                item
                for item in endpoint_a.events()
                if item.get("type") == "resource"
                and item.get("details", {}).get("state")
                in {"outbound_failed", "outbound_cancelled"}
            ),
            None,
        )
        if rust_failure is not None:
            progress = next(
                (
                    item
                    for item in reversed(peer_events)
                    if item.get("link_id") == link_id
                    and item.get("event_type") == "progress"
                ),
                None,
            )
            detail = "no peer progress"
            if progress is not None:
                detail = f"peer received {progress.get('received')}/{progress.get('total')} parts"
            state = rust_failure["details"]["state"]
            return {
                "failure": (
                    f"LXMF-rs Resource sender reported {state}; {detail}; "
                    f"control hash={sent['resource_hash']}, "
                    f"event hash={rust_failure.get('resource_hash')}"
                )
            }
        return None

    outcome = wait_until(
        "LXMF-rs Resource at rns-rs over two hops",
        result,
        timeout=180,
        interval=0.25,
    )
    if outcome.get("failure"):
        raise AssertionError(outcome["failure"])
    return outcome


def run_rns_rs_transport_topology(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    evidence: Evidence,
) -> None:
    with rust_rns_rust_topology(
        root, repository_root, peer_root, peer_binary
    ) as (endpoint_a, endpoint_c, destination_a, destination_c):
        evidence.run(
            "multi-hop announce propagation and path establishment",
            "bidirectional",
            lambda: exchange_rust_endpoint_announces(
                endpoint_a, endpoint_c, destination_a, destination_c
            ),
            topology=TOPOLOGY_RUST_RNS_RUST,
        )
        for direction, sender, receiver, destination, payload in (
            (
                "left -> right",
                endpoint_a,
                endpoint_c,
                destination_c,
                b"rust-a-via-rns-rs-to-rust-c",
            ),
            (
                "right -> left",
                endpoint_c,
                endpoint_a,
                destination_a,
                b"rust-c-via-rns-rs-to-rust-a",
            ),
        ):
            evidence.run(
                "multi-hop encrypted packet and delivery proof",
                direction,
                lambda s=sender, r=receiver, d=destination, p=payload: (
                    packet_rust_via_rns_rs(s, r, d, p)
                ),
                topology=TOPOLOGY_RUST_RNS_RUST,
                expected_bytes=len(payload),
                content_hash=sha256(payload),
            )

        link = evidence.run(
            "multi-hop Link establishment proof",
            "left -> right",
            lambda: link_rust_via_rns_rs(endpoint_a, endpoint_c, destination_c),
            topology=TOPOLOGY_RUST_RNS_RUST,
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
                topology=TOPOLOGY_RUST_RNS_RUST,
                expected_bytes=len(payload),
                content_hash=digest,
            )
        evidence.run(
            "Link teardown",
            "bidirectional",
            lambda: teardown_rust_link(endpoint_a, endpoint_c, link_id),
            topology=TOPOLOGY_RUST_RNS_RUST,
        )
        reverse_link = evidence.run(
            "multi-hop Link establishment proof",
            "right -> left",
            lambda: link_rust_via_rns_rs(endpoint_c, endpoint_a, destination_a),
            topology=TOPOLOGY_RUST_RNS_RUST,
        )
        if reverse_link is not None:
            evidence.run(
                "Link teardown",
                "right -> left",
                lambda: teardown_rust_link(
                    endpoint_c, endpoint_a, reverse_link["link_id"]
                ),
                topology=TOPOLOGY_RUST_RNS_RUST,
            )
        else:
            evidence.record(
                "Link teardown",
                "right -> left",
                "BLOCKED",
                "Link teardown cannot execute because reverse Link activation failed",
                topology=TOPOLOGY_RUST_RNS_RUST,
                classification="dependency_failed",
            )


@contextmanager
def rust_rns_rust_topology(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
) -> Iterator[tuple[RustProbe, RustProbe, str, str]]:
    port_ab = free_port()
    port_bc = free_port()
    control_a = free_port()
    control_b = free_port()
    control_c = free_port()
    config_b = root / "config" / "multi-hop-rust-rns-rust-b"
    write_rns_transport_config(config_b / "config", port_ab, port_bc)
    logs = root / "logs"
    processes: list[ManagedProcess] = []
    try:
        processes.append(
            ManagedProcess(
                "LXMF-rs endpoint A",
                [
                    str(repository_root / "target/release/independent-interop-node"),
                    "--name",
                    "lxmf-rs-rns-transport-a",
                    "--identity-seed",
                    "lxmf-rs-independent-rns-transport-a",
                    "--control",
                    f"127.0.0.1:{control_a}",
                    "--listen",
                    f"127.0.0.1:{port_ab}",
                ],
                repository_root,
                logs / "multi-hop-rust-a-rns-transport.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
            )
        )
        endpoint_a = RustProbe(control_a)
        status_a = wait_until(
            "left LXMF-rs endpoint control",
            lambda: endpoint_a.call("status"),
            timeout=15,
        )
        processes.append(
            ManagedProcess(
                "LXMF-rs endpoint C",
                [
                    str(repository_root / "target/release/independent-interop-node"),
                    "--name",
                    "lxmf-rs-rns-transport-c",
                    "--identity-seed",
                    "lxmf-rs-independent-rns-transport-c",
                    "--control",
                    f"127.0.0.1:{control_c}",
                    "--listen",
                    f"127.0.0.1:{port_bc}",
                ],
                repository_root,
                logs / "multi-hop-rust-c-rns-transport.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
            )
        )
        endpoint_c = RustProbe(control_c)
        status_c = wait_until(
            "right LXMF-rs endpoint control",
            lambda: endpoint_c.call("status"),
            timeout=15,
        )
        processes.append(
            ManagedProcess(
                "rns-rs transport B",
                [
                    str(peer_binary),
                    "http",
                    "--disable-auth",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    str(control_b),
                    "--config",
                    str(config_b),
                ],
                peer_root,
                logs / "multi-hop-rns-rs-transport-b.log",
                {"RUST_LOG": os.environ.get("LXMF_INTEROP_PEER_LOG", "info")},
            )
        )
        middle = RnsRsNode(control_b)
        wait_until(
            "rns-rs transport interfaces",
            lambda: rns_transport_ready(middle),
            timeout=20,
        )
        yield (
            endpoint_a,
            endpoint_c,
            status_a["destination_hash"],
            status_c["destination_hash"],
        )
    finally:
        for control in (control_a, control_c):
            try:
                RustProbe(control).call("shutdown")
            except Exception:
                pass
        for process in reversed(processes):
            process.stop()


def write_rns_transport_config(path: Path, endpoint_a_port: int, endpoint_c_port: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "[reticulum]\n"
        "enable_transport = Yes\n"
        "share_instance = No\n\n"
        "[interfaces]\n"
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


def rns_transport_ready(middle: RnsRsNode) -> dict[str, Any] | None:
    status = middle.get("/api/interfaces")
    interfaces = status.get("interfaces", [])
    online = [item for item in interfaces if item.get("status") == "up"]
    if status.get("transport_enabled") and len(online) >= 2:
        return {"online_interfaces": len(online)}
    return None


def exchange_rust_endpoint_announces(
    endpoint_a: RustProbe,
    endpoint_c: RustProbe,
    destination_a: str,
    destination_c: str,
) -> dict[str, Any]:
    endpoint_a.events(clear=True)
    endpoint_c.events(clear=True)
    app_a = b"rust-a-through-rns-rs"
    app_c = b"rust-c-through-rns-rs"
    # Exercise each propagation direction independently. Emitting both local
    # announces in the same scheduler tick makes an intermediary rebroadcast
    # race indistinguishable from a directional interop failure.
    endpoint_c.call("announce", {"app_data": b64(app_c)})
    seen_at_a = rust_event(
        endpoint_a,
        lambda event: event.get("type") == "announce"
        and event.get("destination_hash") == destination_c
        and event.get("app_data") == b64(app_c)
        and event.get("hops") == 2,
        "right endpoint announce through rns-rs",
        timeout=30,
    )
    # A reconnect can deliver an automatic announce while the first
    # direction is being observed.  Expire that path before the reverse
    # probe so the explicit announce is evaluated as rediscovery rather than
    # being discarded as a stale path refresh.
    endpoint_c.call("expire_path", {"destination_hash": destination_a})
    endpoint_a.call("announce", {"app_data": b64(app_a)})
    seen_at_c = rust_event(
        endpoint_c,
        lambda event: event.get("type") == "announce"
        and event.get("destination_hash") == destination_a
        and event.get("app_data") == b64(app_a)
        and event.get("hops") == 2,
        "left endpoint announce through rns-rs",
        timeout=30,
    )
    path_a = endpoint_a.call("has_path", {"destination_hash": destination_c})
    path_c = endpoint_c.call("has_path", {"destination_hash": destination_a})
    if path_a.get("hops") != 2 or path_c.get("hops") != 2:
        raise AssertionError(f"unexpected paths through rns-rs: {path_a!r}, {path_c!r}")
    return {"left_hops": seen_at_a["hops"], "right_hops": seen_at_c["hops"]}


def packet_rust_via_rns_rs(
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
        and event.get("hops") == 2,
        "two-hop packet through rns-rs",
        timeout=30,
    )
    receipt = rust_event(
        sender,
        lambda event: event.get("type") == "receipt"
        and event.get("packet_hash") == sent.get("packet_hash"),
        "two-hop proof through rns-rs",
        timeout=30,
    )
    return {
        "packet_hash": sent["packet_hash"],
        "proof_packet_hash": receipt["packet_hash"],
        "hops": received["hops"],
    }


def link_rust_via_rns_rs(
    initiator: RustProbe,
    responder: RustProbe,
    destination: str,
) -> dict[str, Any]:
    created = initiator.call("link", {"destination_hash": destination})
    link_id = created["link_id"]
    initiator_link = wait_until(
        "initiator Link through rns-rs",
        lambda: next(
            (
                item
                for item in initiator.call("links")["links"]
                if item.get("link_id") == link_id and item.get("state") == "activated"
            ),
            None,
        ),
        timeout=30,
    )
    responder_link = wait_until(
        "responder Link through rns-rs",
        lambda: next(
            (
                item
                for item in responder.call("links")["links"]
                if item.get("link_id") == link_id and item.get("state") == "activated"
            ),
            None,
        ),
        timeout=30,
    )
    return {"link_id": link_id, "initiator": initiator_link, "responder": responder_link}


def resource_rust_via_rns_rs(
    sender: RustProbe,
    receiver: RustProbe,
    link_id: str,
    payload: bytes,
) -> dict[str, Any]:
    sender.events(clear=True)
    receiver.events(clear=True)
    sent = sender.call("resource", {"link_id": link_id, "data": b64(payload)})
    received = rust_event(
        receiver,
        lambda event: event.get("type") == "resource"
        and event.get("link_id") == link_id
        and event.get("details", {}).get("state") == "complete"
        and event.get("details", {}).get("bytes") == len(payload)
        and event.get("details", {}).get("sha256") == sha256(payload),
        "Resource receiver through rns-rs",
        timeout=180,
    )
    completed = rust_event(
        sender,
        lambda event: event.get("type") == "resource"
        and event.get("resource_hash") == sent["resource_hash"]
        and event.get("details", {}).get("state") == "outbound_complete",
        "Resource sender proof through rns-rs",
        timeout=30,
    )
    return {
        "resource_hash": sent["resource_hash"],
        "received_bytes": received["details"]["bytes"],
        "received_sha256": received["details"]["sha256"],
        "sender_state": completed["details"]["state"],
    }


def teardown_rust_link(
    initiator: RustProbe,
    responder: RustProbe,
    link_id: str,
) -> dict[str, Any]:
    initiator.call("close_link", {"link_id": link_id})
    for label, endpoint in (("initiator", initiator), ("responder", responder)):
        wait_until(
            f"{label} Link teardown",
            lambda endpoint=endpoint: next(
                (
                    item
                    for item in endpoint.call("links")["links"]
                    if item.get("link_id") == link_id and item.get("state") == "closed"
                ),
                None,
            ),
            timeout=15,
        )
    return {"link_id": link_id}
