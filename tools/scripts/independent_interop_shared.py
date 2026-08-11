#!/usr/bin/env python3
"""Shared-instance daemon, client, restart, and independent-peer evidence."""

from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Any

from independent_interop_support import (
    Evidence,
    ManagedProcess,
    RustProbe,
    b64,
    free_port,
    sha256,
    wait_until,
)


TOPOLOGY = "rns-rs local client — LXMF-rs reticulumd — LXMF-rs remote endpoint"


def run_shared_instance_scenarios(
    root: Path,
    repository_root: Path,
    peer_root: Path,
    shared_client_binary: Path,
    evidence: Evidence,
    level: str,
) -> None:
    del level
    shared_port, peer_port, rpc_port, peer_control, client_control = (
        free_port() for _ in range(5)
    )
    config_root = root / "config/shared-instance"
    config_root.mkdir(parents=True, exist_ok=True)
    daemon_config = config_root / "reticulumd.toml"
    daemon_config.write_text(
        "[reticulum]\n"
        "enable_transport = true\n"
        "share_instance = true\n"
        "shared_instance_type = \"tcp\"\n"
        f"shared_instance_port = {shared_port}\n"
        "force_shared_instance_bitrate = 1000000000\n\n"
        "[[interfaces]]\n"
        "type = \"tcp_client\"\n"
        "enabled = true\n"
        "name = \"independent-rns-rs\"\n"
        "host = \"127.0.0.1\"\n"
        f"port = {peer_port}\n",
        encoding="utf-8",
    )
    logs = root / "logs"
    peer = ManagedProcess(
        "LXMF-rs shared-instance remote endpoint",
        [
            str(repository_root / "target/release/independent-interop-node"),
            "--name",
            "lxmf-rs-shared-remote",
            "--identity-seed",
            "lxmf-rs-independent-shared-remote",
            "--control",
            f"127.0.0.1:{peer_control}",
            "--listen",
            f"127.0.0.1:{peer_port}",
        ],
        repository_root,
        logs / "shared-lxmf-rs-remote.log",
        {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "info")},
    )
    daemon: ManagedProcess | None = None
    client: ManagedProcess | None = None
    try:
        remote = RustProbe(peer_control)
        peer_destination = wait_until(
            "shared remote control", lambda: remote.call("status"), timeout=15
        )["destination_hash"]
        daemon = start_daemon(
            repository_root,
            daemon_config,
            config_root / "reticulum.db",
            rpc_port,
            logs / "shared-reticulumd-initial.log",
        )
        client = ManagedProcess(
            "pinned rns-rs shared-instance client",
            [
                str(shared_client_binary),
                str(shared_port),
                f"127.0.0.1:{client_control}",
            ],
            peer_root,
            logs / "shared-rns-rs-client.log",
            {"RUST_LOG": os.environ.get("LXMF_INTEROP_PEER_LOG", "info")},
        )
        client_probe = RustProbe(client_control)
        initial = evidence.run(
            "shared daemon starts, local client attaches, and remote peer is discovered",
            "bidirectional discovery",
            lambda: discover(client_probe, remote, peer_destination),
            topology=TOPOLOGY,
        )
        if initial is not None:
            exchange(evidence, client_probe, remote, peer_destination, "before restart")

        initial_daemon_pid = daemon.process.pid
        daemon.stop()
        daemon = None
        time.sleep(1.0)
        disconnected = wait_until(
            "rns-rs client observes daemon disconnect",
            lambda: not client_probe.call("status")["connected"],
            timeout=20,
        )
        daemon = start_daemon(
            repository_root,
            daemon_config,
            config_root / "reticulum.db",
            rpc_port,
            logs / "shared-reticulumd-restarted.log",
        )
        restarted = evidence.run(
            "daemon restart preserves client identity and reconnects shared-instance traffic",
            "daemon restart",
            lambda: rediscover_after_restart(
                client_probe,
                remote,
                peer_destination,
                disconnected,
                initial_daemon_pid,
                daemon.process.pid,
                initial or {},
            ),
            topology=TOPOLOGY,
        )
        if restarted is not None:
            exchange(evidence, client_probe, remote, peer_destination, "after restart")
    finally:
        if client is not None:
            client.stop()
        if daemon is not None:
            daemon.stop()
        peer.stop()


def start_daemon(
    repository_root: Path,
    config: Path,
    database: Path,
    rpc_port: int,
    log: Path,
) -> ManagedProcess:
    process = ManagedProcess(
        "LXMF-rs shared reticulumd",
        [
            str(repository_root / "target/release/reticulumd"),
            "--rpc",
            f"127.0.0.1:{rpc_port}",
            "--rpc-unix",
            str(config.parent / "rpc.sock"),
            "--db",
            str(database),
            "--config",
            str(config),
            "--strict-interface-startup",
        ],
        repository_root,
        log,
        {"RUST_LOG": os.environ.get("LXMF_INTEROP_RUST_LOG", "rns_transport=debug,info")},
    )
    wait_until(
        "shared reticulumd listener",
        lambda: _port_ready(int(config.read_text().split("shared_instance_port = ")[1].splitlines()[0])),
        timeout=20,
    )
    return process


def _port_ready(port: int) -> bool:
    import socket

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.25):
            return True
    except OSError:
        return False


def discover(
    client: RustProbe, remote: RustProbe, peer_destination: str
) -> dict[str, Any]:
    status = wait_until(
        "rns-rs shared client attachment",
        lambda: connected_status(client),
        timeout=20,
    )
    client_destination = status["destination_hash"]
    client_discovery = discover_shared_client(
        client, remote, client_destination, timeout=30
    )
    remote.call("announce", {"app_data": b64(b"shared-lxmf-rs")})
    wait_until(
        "rns-rs shared client discovers remote LXMF-rs",
        lambda: client.call("known", {"destination_hash": peer_destination})["known"],
        timeout=30,
    )
    return {
        "client_connected": True,
        "client_destination": client_destination,
        "client_reconnects": status["reconnects"],
        "client_discovery_attempts": client_discovery["attempts"],
        "peer_destination": peer_destination,
    }


def discover_shared_client(
    client: RustProbe,
    remote: RustProbe,
    client_destination: str,
    *,
    timeout: float,
) -> dict[str, Any]:
    """Discover the shared client without racing announce propagation limits."""
    remote.events(clear=True)
    deadline = time.monotonic() + timeout
    attempts = 0
    while time.monotonic() < deadline:
        attempts += 1
        client.call("announce", {"app_data": b64(b"shared-rns-rs-client")})

        # A reconnect can happen inside the normal announce rebroadcast window.
        # In that case the daemon accepts the client's announce but correctly
        # suppresses another broadcast.  An explicit path request provides the
        # deterministic RNS discovery mechanism and yields a PathResponse from
        # the newly attached client instead of depending on that timer.
        remote.call("expire_path", {"destination_hash": client_destination})
        remote.call("request_path", {"destination_hash": client_destination})
        attempt_deadline = min(deadline, time.monotonic() + 3.0)
        while time.monotonic() < attempt_deadline:
            event = next(
                (
                    event
                    for event in remote.events()
                    if event.get("type") == "announce"
                    and event.get("destination_hash") == client_destination
                ),
                None,
            )
            if event is not None:
                return {"attempts": attempts, "announce": event}
            time.sleep(0.1)
    raise TimeoutError("timed out waiting for remote endpoint discovers shared client")


def rediscover_after_restart(
    client: RustProbe,
    remote: RustProbe,
    peer_destination: str,
    disconnected: bool,
    initial_daemon_pid: int,
    restarted_daemon_pid: int,
    initial: dict[str, Any],
) -> dict[str, Any]:
    status = wait_until(
        "rns-rs client reconnects",
        lambda: connected_status(client),
        timeout=30,
    )
    discovered = discover(client, remote, peer_destination)
    if initial.get("client_destination") != status["destination_hash"]:
        raise AssertionError("shared client destination identity changed across daemon restart")
    if initial_daemon_pid == restarted_daemon_pid:
        raise AssertionError("daemon process did not restart")
    if status["reconnects"] <= initial.get("client_reconnects", 0):
        raise AssertionError("shared client did not observe a new interface-up event")
    return {
        **discovered,
        "disconnect_observed": disconnected,
        "initial_daemon_pid": initial_daemon_pid,
        "restarted_daemon_pid": restarted_daemon_pid,
        "identity_continuity": True,
    }


def connected_status(client: RustProbe) -> dict[str, Any] | None:
    status = client.call("status")
    return status if status["connected"] else None


def exchange(
    evidence: Evidence,
    client: RustProbe,
    remote: RustProbe,
    peer_destination: str,
    phase: str,
) -> None:
    status = client.call("status")
    client_destination = status["destination_hash"]
    for direction, action, payload in (
        (
            "LXMF-rs remote -> rns-rs local client",
            lambda: remote_to_client(client, remote, client_destination, phase),
            f"lxmf-rs-{phase}".encode(),
        ),
        (
            "rns-rs local client -> LXMF-rs remote",
            lambda: client_to_remote(client, remote, peer_destination, phase),
            f"rns-rs-{phase}".encode(),
        ),
    ):
        evidence.run(
            f"shared-instance encrypted packet and delivery proof {phase}",
            direction,
            action,
            topology=TOPOLOGY,
            expected_bytes=len(payload),
            content_hash=sha256(payload),
        )


def remote_to_client(
    client: RustProbe, remote: RustProbe, destination: str, phase: str
) -> dict[str, Any]:
    payload = f"lxmf-rs-{phase}".encode()
    client.call("clear")
    remote.events(clear=True)
    sent = remote.call("send", {"destination_hash": destination, "data": b64(payload)})
    received = wait_until(
        "shared client packet",
        lambda: next(
            (
                row
                for row in client.call("status")["received"]
                if row.get("sha256") == sha256(payload)
            ),
            None,
        ),
        timeout=30,
    )
    wait_until(
        "shared client delivery proof at remote LXMF-rs",
        lambda: next(
            (
                event
                for event in remote.events()
                if event.get("type") == "receipt"
                and event.get("packet_hash") == sent["packet_hash"]
            ),
            None,
        ),
        timeout=30,
    )
    return {"packet_hash": sent["packet_hash"], "received_sha256": received["sha256"]}


def client_to_remote(
    client: RustProbe, remote: RustProbe, destination: str, phase: str
) -> dict[str, Any]:
    payload = f"rns-rs-{phase}".encode()
    remote.events(clear=True)
    sent = client.call("send", {"destination_hash": destination, "data": b64(payload)})
    packet = wait_until(
        "shared rns-rs packet at remote LXMF-rs",
        lambda: next(
            (
                event
                for event in remote.events()
                if event.get("type") == "data"
                and event.get("destination_hash") == destination
                and event.get("data") == b64(payload)
            ),
            None,
        ),
        timeout=30,
    )
    wait_until(
        "rns-rs delivery proof at shared rns-rs client",
        lambda: sent["token"] in client.call("status")["proofs"],
        timeout=30,
    )
    return {"packet_hash": sent["packet_hash"], "received_sha256": packet["sha256"]}
