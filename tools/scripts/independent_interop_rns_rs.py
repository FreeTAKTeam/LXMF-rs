#!/usr/bin/env python3
"""rns-rs adapter and network scenarios for independent interop evidence."""

from __future__ import annotations

import json
import random
import struct
import time
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

from independent_interop_support import (
    Evidence,
    ManagedProcess,
    RnsRsControl,
    RnsRsNode,
    RustProbe,
    b64,
    free_port,
    sha256,
    wait_until,
)


def write_rns_rs_config(path: Path, rust_port: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "[reticulum]\n"
        "enable_transport = No\n"
        "share_instance = No\n\n"
        "[interfaces]\n"
        "  [[LXMF-rs]]\n"
        "    type = TCPClientInterface\n"
        "    target_host = 127.0.0.1\n"
        f"    target_port = {rust_port}\n",
        encoding="utf-8",
    )


def rust_event(
    rust: RustProbe,
    predicate: Any,
    label: str,
    timeout: float = 20.0,
) -> dict[str, Any]:
    return wait_until(
        label,
        lambda: next((event for event in rust.events() if predicate(event)), None),
        timeout=timeout,
    )


def rns_link(rns: RnsRsNode, link_id: str, state: str = "active") -> dict[str, Any]:
    event_type = "established" if state == "active" else state
    return wait_until(
        f"rns-rs link {link_id} {state}",
        lambda: next(
            (
                event
                for event in rns.get("/api/link_events")["link_events"]
                if event.get("link_id") == link_id
                and event.get("event_type") == event_type
            ),
            None,
        ),
        timeout=12.0,
    )


def run_two_node(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    peer_control_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    with two_node_session(
        root, repository_root, peer_root, peer_control_binary, "path"
    ) as (rust, rns, _rns_control, rust_destination, rns_destination):
        evidence.run(
            "path unknown",
            "bidirectional",
            lambda: assert_paths_unknown(rust, rns, rust_destination, rns_destination),
        )
        evidence.run(
            "path request and establishment",
            "LXMF-rs -> rns-rs",
            lambda: request_path_rust_to_rns(rust, rns_destination),
            failure_owner="rns-rs",
            classification="peer_divergence",
            normative_reference="Python RNS 1.5.2 answers the same LXMF-rs path request",
        )
        evidence.run(
            "path request and establishment",
            "rns-rs -> LXMF-rs",
            lambda: request_path_rns_to_rust(rns, rust_destination),
        )

    with two_node_session(
        root, repository_root, peer_root, peer_control_binary, "payload"
    ) as (rust, rns, rns_control, rust_destination, rns_destination):
        evidence.run(
            "announce identity signature and app-data",
            "bidirectional",
            lambda: exchange_announces(rust, rns, rust_destination, rns_destination),
        )
        evidence.run(
            "cached path",
            "bidirectional",
            lambda: assert_cached_paths(rust, rns, rust_destination, rns_destination),
        )
        evidence.run(
            "encrypted data packet and delivery proof",
            "rns-rs -> LXMF-rs",
            lambda: packet_rns_to_rust(rust, rns, rust_destination),
            expected_bytes=len(b"rns-rs-to-lxmf-rs"),
            content_hash=sha256(b"rns-rs-to-lxmf-rs"),
        )
        evidence.run(
            "encrypted data packet and delivery proof",
            "LXMF-rs -> rns-rs",
            lambda: packet_rust_to_rns(rust, rns, rns_destination),
            expected_bytes=len(b"lxmf-rs-to-rns-rs"),
            content_hash=sha256(b"lxmf-rs-to-rns-rs"),
        )
        rns_link_details = evidence.run(
            "Link establishment proof RTT",
            "rns-rs -> LXMF-rs",
            lambda: link_rns_to_rust(rust, rns, rust_destination),
        )
        active_link = None if rns_link_details is None else rns_link_details["link_id"]
        if active_link:
            run_request_scenarios(evidence, rust, rns, rns_control, active_link)
            run_resource_lifecycle_scenarios(evidence, rust, rns, rns_control, active_link)
            link_usable = run_link_payload_scenarios(
                evidence, rust, rns, active_link, level
            )
            if link_usable:
                close_bidirectional_link(evidence, rust, rns, active_link)
            else:
                evidence.record(
                    "Link teardown",
                    "rns-rs -> LXMF-rs",
                    "BLOCKED",
                    "Channel proof divergence closed the Link before teardown",
                    classification="dependency_failed",
                    failure_owner="rns-rs",
                )

    with two_node_session(
        root, repository_root, peer_root, peer_control_binary, "rust-link"
    ) as (rust, rns, _rns_control, rust_destination, rns_destination):
        rust_link = evidence.run(
            "Link establishment proof RTT",
            "LXMF-rs -> rns-rs",
            lambda: establish_announces_then_rust_link(
                rust, rns, rust_destination, rns_destination
            ),
            failure_owner="rns-rs",
            classification="peer_divergence",
            normative_reference="Python RNS 1.5.2 activates the same LXMF-rs-initiated Link",
        )
        if rust_link:
            close_bidirectional_link(evidence, rust, rns, rust_link["link_id"])
        else:
            evidence.record(
                "Link teardown",
                "LXMF-rs -> rns-rs",
                "BLOCKED",
                "Link teardown cannot execute because the peer did not activate the Link",
                classification="dependency_failed",
                failure_owner="rns-rs",
            )


@contextmanager
def two_node_session(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    peer_binary: Path,
    session: str,
) -> Iterator[tuple[RustProbe, RnsRsNode, RnsRsControl, str, str]]:
    rust_rns_port = free_port()
    rust_control_port = free_port()
    rns_control_port = free_port()
    rns_interop_control_port = free_port()
    config_dir = root / "config" / f"rns-rs-{session}"
    write_rns_rs_config(config_dir / "config", rust_rns_port)
    logs = root / "logs"
    rust_process = ManagedProcess(
        f"LXMF-rs probe ({session})",
        [
            str(repository_root / "target/release/independent-interop-node"),
            "--name",
            "lxmf-rs-a",
            "--identity-seed",
            "lxmf-rs-independent-a",
            "--control",
            f"127.0.0.1:{rust_control_port}",
            "--listen",
            f"127.0.0.1:{rust_rns_port}",
        ],
        repository_root,
        logs / f"lxmf-rs-a-{session}.log",
        {"RUST_LOG": "rns_transport=debug,info"},
    )
    rns_process: ManagedProcess | None = None
    try:
        rust = RustProbe(rust_control_port)
        rust_status = wait_until("LXMF-rs control", lambda: rust.call("status"), timeout=15)
        rns_process = ManagedProcess(
            f"rns-rs node ({session})",
            [
                str(peer_binary),
                "http",
                "--disable-auth",
                "--host",
                "127.0.0.1",
                "--port",
                str(rns_control_port),
                "--interop-control",
                f"127.0.0.1:{rns_interop_control_port}",
                "--config",
                str(config_dir),
            ],
            peer_root,
            logs / f"rns-rs-{session}.log",
            {"RUST_LOG": "rns_net=debug,rns_ctl=info"},
        )
        rns = RnsRsNode(rns_control_port)
        rns_control = RnsRsControl(rns_interop_control_port)
        wait_until("rns-rs interop control", lambda: rns_control.call("health"), timeout=15)
        wait_until("rns-rs HTTP health", lambda: rns.get("/health"), timeout=15)
        rns_destination = rns.post(
            "/api/destination",
            {
                "type": "single",
                "app_name": "interop",
                "aspects": ["probe"],
                "direction": "in",
                "proof_strategy": "all",
            },
        )["dest_hash"]
        rust_destination = rust_status["destination_hash"]
        yield rust, rns, rns_control, rust_destination, rns_destination
    finally:
        try:
            RustProbe(rust_control_port).call("shutdown")
        except Exception:
            pass
        if rns_process is not None:
            rns_process.stop()
        rust_process.stop()


def establish_announces_then_rust_link(
    rust: RustProbe,
    rns: RnsRsNode,
    rust_destination: str,
    rns_destination: str,
) -> dict[str, Any]:
    exchange_announces(rust, rns, rust_destination, rns_destination)
    return link_rust_to_rns(rust, rns, rns_destination)


def assert_paths_unknown(
    rust: RustProbe,
    rns: RnsRsNode,
    rust_destination: str,
    rns_destination: str,
) -> dict[str, Any]:
    if rust.call("has_path", {"destination_hash": rns_destination})["path_found"]:
        raise AssertionError("LXMF-rs unexpectedly had a path before discovery")
    paths = rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
    if any(path.get("hash") == rust_destination for path in paths):
        raise AssertionError("rns-rs unexpectedly had a path before discovery")
    return {"unknown_confirmed": True}


def request_path_rust_to_rns(
    rust: RustProbe,
    rns_destination: str,
) -> dict[str, Any]:
    rust.call("request_path", {"destination_hash": rns_destination})
    rust_path = wait_until(
        "LXMF-rs path",
        lambda: rust.call("has_path", {"destination_hash": rns_destination})["path_found"],
    )
    status = rust.call("has_path", {"destination_hash": rns_destination})
    return {"path_found": rust_path, "hops": status["hops"]}


def request_path_rns_to_rust(
    rns: RnsRsNode,
    rust_destination: str,
) -> dict[str, Any]:
    rns.post("/api/path/request", {"dest_hash": rust_destination})
    rns_path = wait_until(
        "rns-rs path",
        lambda: any(
            path.get("hash") == rust_destination
            for path in rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
        ),
    )
    path = next(
        item
        for item in rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
        if item.get("hash") == rust_destination
    )
    return {"path_found": rns_path, "hops": path["hops"]}


def assert_cached_paths(
    rust: RustProbe,
    rns: RnsRsNode,
    rust_destination: str,
    rns_destination: str,
) -> dict[str, Any]:
    rust_path = rust.call("has_path", {"destination_hash": rns_destination})
    rns_paths = rns.get(f"/api/paths?dest_hash={rust_destination}")["paths"]
    if not rust_path["path_found"] or not any(
        path.get("hash") == rust_destination for path in rns_paths
    ):
        raise AssertionError("a discovered path was not cached")
    return {"lxmf_rs_hops": rust_path["hops"]}


def exchange_announces(
    rust: RustProbe,
    rns: RnsRsNode,
    rust_destination: str,
    rns_destination: str,
) -> dict[str, Any]:
    rust.events(clear=True)
    rns.get("/api/announces?clear=true")
    rust_app = b"lxmf-rs-app-data"
    rns_app = b"rns-rs-app-data"
    rust.call("announce", {"app_data": b64(rust_app)})
    rns.post("/api/announce", {"dest_hash": rns_destination, "app_data": b64(rns_app)})
    rust_seen = rust_event(
        rust,
        lambda event: event.get("type") == "announce"
        and event.get("destination_hash") == rns_destination
        and event.get("app_data") == b64(rns_app)
        and event.get("signature_verified") is True,
        "rns-rs announce at LXMF-rs",
    )
    rns_seen = wait_until(
        "LXMF-rs announce at rns-rs",
        lambda: next(
            (
                item
                for item in rns.get("/api/announces")["announces"]
                if item.get("dest_hash") == rust_destination
                and item.get("app_data") == b64(rust_app)
            ),
            None,
        ),
    )
    identity = rns.get(f"/api/identity/{rust_destination}")
    if identity.get("dest_hash") != rust_destination or not identity.get("public_key"):
        raise AssertionError("rns-rs did not retain the verified LXMF-rs identity")
    return {
        "lxmf_rs_hops": rust_seen["hops"],
        "rns_rs_hops": rns_seen["hops"],
        "identity_hash": identity["identity_hash"],
    }


def ensure_rns_outbound(rns: RnsRsNode, destination: str) -> None:
    rns.post(
        "/api/destination",
        {
            "type": "single",
            "app_name": "interop",
            "aspects": ["probe"],
            "direction": "out",
            "dest_hash": destination,
        },
    )


def packet_rns_to_rust(
    rust: RustProbe, rns: RnsRsNode, rust_destination: str
) -> dict[str, Any]:
    payload = b"rns-rs-to-lxmf-rs"
    rust.events(clear=True)
    rns.get("/api/proofs?clear=true")
    ensure_rns_outbound(rns, rust_destination)
    sent = rns.post("/api/send", {"dest_hash": rust_destination, "data": b64(payload)})
    rust_event(
        rust,
        lambda event: event.get("type") == "data"
        and event.get("data") == b64(payload)
        and event.get("sha256") == sha256(payload),
        "rns-rs packet at LXMF-rs",
    )
    proof = wait_until(
        "delivery proof at rns-rs",
        lambda: next(
            (
                item
                for item in rns.get("/api/proofs")["proofs"]
                if item.get("packet_hash") == sent["packet_hash"]
            ),
            None,
        ),
    )
    return {"packet_hash": sent["packet_hash"], "proof_rtt_seconds": proof["rtt"]}


def packet_rust_to_rns(
    rust: RustProbe, rns: RnsRsNode, rns_destination: str
) -> dict[str, Any]:
    payload = b"lxmf-rs-to-rns-rs"
    rust.events(clear=True)
    rns.get("/api/packets?clear=true")
    result = rust.call(
        "send", {"destination_hash": rns_destination, "data": b64(payload)}
    )
    if result["outcome"] != "SentDirect":
        raise AssertionError(f"LXMF-rs packet outcome was {result['outcome']}")
    packet = wait_until(
        "LXMF-rs packet at rns-rs",
        lambda: next(
            (
                item
                for item in rns.get("/api/packets")["packets"]
                if item.get("dest_hash") == rns_destination
            ),
            None,
        ),
    )
    receipt = rust_event(
        rust,
        lambda event: event.get("type") == "receipt"
        and event.get("packet_hash") == result["packet_hash"],
        "delivery proof at LXMF-rs",
    )
    return {
        "packet_hash": packet["packet_hash"],
        "proof_packet_hash": receipt["packet_hash"],
    }


def link_rust_to_rns(
    rust: RustProbe, rns: RnsRsNode, rns_destination: str
) -> dict[str, Any]:
    created = rust.call("link", {"destination_hash": rns_destination})
    link = rns_link(rns, created["link_id"])
    rust_active = wait_until(
        "LXMF-rs outbound Link active",
        lambda: next(
            (
                item
                for item in rust.call("links")["links"]
                if item["link_id"] == created["link_id"] and item["state"] == "activated"
            ),
            None,
        ),
    )
    return {"link_id": created["link_id"], "rtt_seconds": link["rtt"], "rust": rust_active}


def link_rns_to_rust(
    rust: RustProbe, rns: RnsRsNode, rust_destination: str
) -> dict[str, Any]:
    created = rns.post("/api/link", {"dest_hash": rust_destination})
    link = rns_link(rns, created["link_id"])
    wait_until(
        "LXMF-rs inbound Link active",
        lambda: next(
            (
                item
                for item in rust.call("links")["links"]
                if item["link_id"] == created["link_id"] and item["state"] == "activated"
            ),
            None,
        ),
    )
    return {"link_id": created["link_id"], "rtt_seconds": link["rtt"]}


def run_link_payload_scenarios(
    evidence: Evidence,
    rust: RustProbe,
    rns: RnsRsNode,
    link_id: str,
    level: str,
) -> bool:
    evidence.run(
        "Link data",
        "bidirectional",
        lambda: exchange_link_data(rust, rns, link_id),
    )
    sizes = [("small", 4096), ("1 MiB", 1024 * 1024)]
    if level in {"nightly", "release"}:
        sizes.append(("50 MiB", 50 * 1024 * 1024))
    for label, size in sizes:
        payload = deterministic_payload(size)
        digest = sha256(payload)
        evidence.run(
            f"Resource {label}",
            "rns-rs -> LXMF-rs",
            lambda data=payload: resource_rns_to_rust(rust, rns, link_id, data),
            expected_bytes=size,
            content_hash=digest,
        )
        evidence.run(
            f"Resource {label}",
            "LXMF-rs -> rns-rs",
            lambda data=payload: resource_rust_to_rns(rust, rns, link_id, data),
            expected_bytes=size,
            content_hash=digest,
        )
    return (
        evidence.run(
            "Channel ordered exchange and proof",
            "bidirectional",
            lambda: exchange_channel(rust, rns, link_id),
            failure_owner="rns-rs",
            classification="peer_divergence",
            normative_reference=(
                "Python RNS 1.5.2 proves the same bidirectional LXMF-rs Channel exchange"
            ),
        )
        is not None
    )


def run_request_scenarios(
    evidence: Evidence,
    rust: RustProbe,
    rns: RnsRsNode,
    rns_control: RnsRsControl,
    link_id: str,
) -> None:
    evidence.run(
        "packet-sized request response and correlation",
        "LXMF-rs -> rns-rs",
        lambda: request_rust_to_rns(rust, link_id),
    )
    evidence.run(
        "packet-sized request response and correlation",
        "rns-rs -> LXMF-rs",
        lambda: request_rns_to_rust(rust, rns, rns_control, link_id),
    )
    response_size = 1024 * 1024
    evidence.run(
        "compressed resource-sized request response",
        "LXMF-rs -> rns-rs -> LXMF-rs",
        lambda: resource_response_rns_to_rust(rust, rns, link_id, response_size),
        expected_bytes=response_size,
        content_hash=sha256(deterministic_payload(response_size)),
    )
    evidence.run(
        "request timeout",
        "rns-rs -> LXMF-rs",
        lambda: request_timeout(rust, rns, rns_control, link_id),
    )


def run_resource_lifecycle_scenarios(
    evidence: Evidence,
    rust: RustProbe,
    rns: RnsRsNode,
    rns_control: RnsRsControl,
    link_id: str,
) -> None:
    evidence.run(
        "Resource cancellation",
        "rns-rs receiver -> LXMF-rs sender",
        lambda: cancel_resource_rns_to_rust(rust, rns, rns_control, link_id),
    )


def msgpack_binary(data: bytes) -> bytes:
    if len(data) <= 0xFF:
        return bytes((0xC4, len(data))) + data
    if len(data) <= 0xFFFF:
        return b"\xC5" + struct.pack(">H", len(data)) + data
    return b"\xC6" + struct.pack(">I", len(data)) + data


def msgpack_u64(value: int) -> bytes:
    return b"\xCF" + struct.pack(">Q", value)


def request_rust_to_rns(rust: RustProbe, link_id: str) -> dict[str, Any]:
    payload = b"request-lxmf-rs-to-rns-rs"
    application = msgpack_binary(payload)
    rust.events(clear=True)
    sent = rust.call(
        "request",
        {"link_id": link_id, "path": "/interop/echo", "data": b64(application)},
    )
    response = rust_event(
        rust,
        lambda event: event.get("type") == "data"
        and event.get("context") == 10
        and event.get("request_id") == sent["request_id"]
        and event.get("application_data") == b64(payload),
        "correlated rns-rs packet response at LXMF-rs",
    )
    return {
        "request_id": sent["request_id"],
        "response_request_id": response["request_id"],
        "response_sha256": sha256(payload),
    }


def request_rns_to_rust(
    rust: RustProbe,
    rns: RnsRsNode,
    rns_control: RnsRsControl,
    link_id: str,
) -> dict[str, Any]:
    payload = b"request-rns-rs-to-lxmf-rs"
    application = msgpack_binary(payload)
    rust.events(clear=True)
    rns.get("/api/packets?clear=true")
    rns_control.call(
        "request",
        {"link_id": link_id, "path": "/interop/echo", "data": b64(application)},
    )
    request = rust_event(
        rust,
        lambda event: event.get("type") == "data"
        and event.get("context") == 9
        and event.get("application_data") == b64(payload),
        "rns-rs packet request at LXMF-rs",
    )
    request_id = request["request_id"]
    rust.call(
        "respond",
        {"link_id": link_id, "request_id": request_id, "data": b64(application)},
    )
    response = wait_until(
        "correlated LXMF-rs packet response at rns-rs",
        lambda: next(
            (
                item
                for item in rns.get("/api/packets")["packets"]
                if item.get("dest_hash") == f"response:{link_id}:{request_id}"
                and item.get("data_base64") == b64(application)
            ),
            None,
        ),
    )
    return {
        "request_id": request_id,
        "response_destination": response["dest_hash"],
        "response_sha256": sha256(payload),
    }


def resource_response_rns_to_rust(
    rust: RustProbe,
    rns: RnsRsNode,
    link_id: str,
    response_size: int,
) -> dict[str, Any]:
    rust.events(clear=True)
    rns.get("/api/resource_events?clear=true")
    sent = rust.call(
        "request",
        {
            "link_id": link_id,
            "path": "/interop/resource",
            "data": b64(msgpack_u64(response_size)),
        },
    )
    expected = deterministic_payload(response_size)
    response = rust_event(
        rust,
        lambda event: event.get("type") == "resource"
        and event.get("details", {}).get("state") == "complete"
        and event.get("details", {}).get("is_response") is True
        and event.get("details", {}).get("request_id") == sent["request_id"]
        and event.get("details", {}).get("application_bytes") == response_size
        and event.get("details", {}).get("application_sha256") == sha256(expected),
        "correlated rns-rs Resource response at LXMF-rs",
        timeout=180,
    )
    progress = [
        event
        for event in rns.get("/api/resource_events")["resource_events"]
        if event.get("link_id") == link_id and event.get("event_type") == "progress"
    ]
    if not progress:
        raise AssertionError("compressed Resource response emitted no transfer progress evidence")
    transfer_parts = max(int(event["total"]) for event in progress)
    if transfer_parts * 500 >= response_size:
        raise AssertionError(
            f"auto-compressed Resource used {transfer_parts} MTU-bounded parts for {response_size} bytes"
        )
    return {
        "request_id": sent["request_id"],
        "response_request_id": response["details"]["request_id"],
        "received_bytes": response["details"]["application_bytes"],
        "received_sha256": response["details"]["application_sha256"],
        "resource_envelope_bytes": response["details"]["bytes"],
        "resource_envelope_sha256": response["details"]["sha256"],
        "compressed_transfer_parts": transfer_parts,
        "compressed_transfer_upper_bound_bytes": transfer_parts * 500,
        "auto_compress": True,
    }


def request_timeout(
    rust: RustProbe,
    rns: RnsRsNode,
    rns_control: RnsRsControl,
    link_id: str,
) -> dict[str, Any]:
    application = msgpack_binary(b"intentionally-unhandled")
    rust.events(clear=True)
    rns.get("/api/packets?clear=true")
    started = time.monotonic()
    rns_control.call(
        "request",
        {"link_id": link_id, "path": "/interop/unhandled", "data": b64(application)},
    )
    timeout = 3.0
    while time.monotonic() - started < timeout:
        if any(
            item.get("dest_hash", "").startswith(f"response:{link_id}:")
            for item in rns.get("/api/packets")["packets"]
        ):
            raise AssertionError("unhandled request unexpectedly received a response")
        time.sleep(0.1)
    return {"timeout_observed_seconds": round(time.monotonic() - started, 3)}


def cancel_resource_rns_to_rust(
    rust: RustProbe,
    rns: RnsRsNode,
    rns_control: RnsRsControl,
    link_id: str,
) -> dict[str, Any]:
    payload = random.Random(0x4C584D46).randbytes(1024 * 1024)
    rust.events(clear=True)
    rns.get("/api/resource_events?clear=true")
    rns_control.call("set_resource_strategy", {"link_id": link_id, "strategy": 0})
    sent = rust.call("resource", {"link_id": link_id, "data": b64(payload)})
    try:
        sender = rust_event(
            rust,
            lambda event: event.get("type") == "resource"
            and event.get("resource_hash") == sent["resource_hash"]
            and event.get("details", {}).get("state") == "outbound_cancelled",
            "LXMF-rs outbound Resource cancellation",
            timeout=30,
        )
    finally:
        rns_control.call("set_resource_strategy", {"link_id": link_id, "strategy": 1})
    return {
        "resource_hash": sent["resource_hash"],
        "sender_state": sender["details"]["state"],
        "peer_strategy": "AcceptNone",
        "cancelled_payload_bytes": len(payload),
        "cancelled_payload_sha256": sha256(payload),
    }


def deterministic_payload(size: int) -> bytes:
    pattern = bytes(range(251))
    return (pattern * ((size + len(pattern) - 1) // len(pattern)))[:size]


def exchange_link_data(rust: RustProbe, rns: RnsRsNode, link_id: str) -> dict[str, Any]:
    rust.events(clear=True)
    from_rns = b"link-rns-rs-to-lxmf-rs"
    rns.post("/api/link/send", {"link_id": link_id, "data": b64(from_rns), "context": 0})
    rust_event(
        rust,
        lambda event: event.get("type") == "data" and event.get("data") == b64(from_rns),
        "rns-rs Link data at LXMF-rs",
    )
    from_rust = b"link-lxmf-rs-to-rns-rs"
    before = next(
        item for item in rns.get("/api/links")["links"] if item["link_id"] == link_id
    )
    rust.call("link_send", {"link_id": link_id, "data": b64(from_rust)})
    after = wait_until(
        "LXMF-rs Link data at rns-rs",
        lambda: next(
            (
                item
                for item in rns.get("/api/links")["links"]
                if item.get("link_id") == link_id
                and item.get("rx_packets", 0) > before.get("rx_packets", 0)
                and item.get("rx_bytes", 0) > before.get("rx_bytes", 0)
            ),
            None,
        ),
    )
    return {
        "rns_rs_rx_packets_delta": after["rx_packets"] - before["rx_packets"],
        "rns_rs_rx_bytes_delta": after["rx_bytes"] - before["rx_bytes"],
        "lxmf_rs_received_sha256": sha256(from_rns),
        "rns_rs_received_payload_bytes": len(from_rust),
    }


def exchange_channel(rust: RustProbe, rns: RnsRsNode, link_id: str) -> dict[str, Any]:
    rust.events(clear=True)
    rns.get("/api/packets?clear=true")
    from_rns = b"channel-rns-rs-to-lxmf-rs"
    rns.post(
        "/api/channel",
        {"link_id": link_id, "msgtype": 0xCAFE, "payload": b64(from_rns)},
    )
    received = rust_event(
        rust,
        lambda event: event.get("type") == "channel"
        and event.get("payload") == b64(from_rns),
        "rns-rs Channel message at LXMF-rs",
    )
    from_rust = b"channel-lxmf-rs-to-rns-rs"
    sent = rust.call("channel", {"link_id": link_id, "payload": b64(from_rust)})
    wait_until(
        "LXMF-rs Channel message at rns-rs",
        lambda: next(
            (
                item
                for item in rns.get("/api/packets")["packets"]
                if item.get("dest_hash") == f"channel:{link_id}:51966"
                and item.get("data_base64") == b64(from_rust)
            ),
            None,
        ),
    )
    sender = wait_until(
        "LXMF-rs Channel delivery proof",
        lambda: channel_terminal_state(rust, link_id, sent["sequence"]),
        timeout=15.0,
    )
    if sender["state"] != "delivered":
        raise AssertionError(
            f"LXMF-rs Channel message reached terminal state {sender['state']!r}"
        )
    return {
        "received_sequence": received["sequence"],
        "sent_sequence": sent["sequence"],
        "sender_state": sender["state"],
    }


def channel_terminal_state(
    rust: RustProbe, link_id: str, sequence: int
) -> dict[str, Any] | None:
    state = rust.call("channel_state", {"link_id": link_id, "sequence": sequence})
    return state if state.get("state") in {"delivered", "failed"} else None


def resource_rns_to_rust(
    rust: RustProbe, rns: RnsRsNode, link_id: str, payload: bytes
) -> dict[str, Any]:
    rust.events(clear=True)
    rns.post("/api/resource", {"link_id": link_id, "data": b64(payload)})
    event = rust_event(
        rust,
        lambda item: item.get("type") == "resource"
        and item.get("link_id") == link_id
        and item.get("details", {}).get("state") == "complete"
        and item.get("details", {}).get("sha256") == sha256(payload),
        "rns-rs Resource at LXMF-rs",
        timeout=180.0,
    )
    return {"resource_hash": event["resource_hash"]}


def resource_rust_to_rns(
    rust: RustProbe, rns: RnsRsNode, link_id: str, payload: bytes
) -> dict[str, Any]:
    rns.get("/api/resource_events?clear=true")
    sent = rust.call("resource", {"link_id": link_id, "data": b64(payload)})
    event = wait_until(
        "LXMF-rs Resource at rns-rs",
        lambda: next(
            (
                item
                for item in rns.get("/api/resource_events")["resource_events"]
                if item.get("link_id") == link_id
                and item.get("event_type") == "received"
                and item.get("data_base64") == b64(payload)
            ),
            None,
        ),
        timeout=180.0,
        interval=0.25,
    )
    decoded_size = len(event["data_base64"])
    return {"resource_hash": sent["resource_hash"], "encoded_bytes": decoded_size}


def close_bidirectional_link(
    evidence: Evidence, rust: RustProbe, rns: RnsRsNode, link_id: str
) -> None:
    evidence.run(
        "Link teardown",
        "bidirectional",
        lambda: teardown(rust, rns, link_id),
    )


def teardown(rust: RustProbe, rns: RnsRsNode, link_id: str) -> dict[str, Any]:
    try:
        rns.post("/api/link/close", {"link_id": link_id})
    except RuntimeError:
        rust.call("close_link", {"link_id": link_id})
    wait_until(
        "Link teardown at LXMF-rs",
        lambda: next(
            (
                item
                for item in rust.call("links")["links"]
                if item["link_id"] == link_id and item["state"] == "closed"
            ),
            None,
        ),
    )
    return {"link_id": link_id}
