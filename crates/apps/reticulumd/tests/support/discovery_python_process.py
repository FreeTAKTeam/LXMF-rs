"""Native loopback discovery traces; bounds are frozen in DISCOVERY.md."""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import queue
import signal
import socket
import subprocess
import sys
import threading
import time

import LXMF
import RNS
import RNS.Discovery
from RNS.vendor import umsgpack as msgpack

READINESS = 15
TRAFFIC = 30
CHILDREN = []
RESERVED_PORTS = set()


def trace(**values):
    print("TRACE " + json.dumps(values, sort_keys=True), flush=True)


def wait_for(deadline, description, probe):
    while time.monotonic() < deadline:
        result = probe()
        if result:
            return result
        for child in CHILDREN:
            assert child.poll() is None, f"child {child.pid} exited: {child.returncode}"
        time.sleep(0.025)
    raise AssertionError(f"deadline waiting for {description}")


def reserve_port(kind=socket.SOCK_STREAM):
    while True:
        with socket.socket(socket.AF_INET, kind) as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        if port not in RESERVED_PORTS:
            RESERVED_PORTS.add(port)
            return port


def rpc(port, method, params=None):
    payload = msgpack.packb([1, method, params])
    body = len(payload).to_bytes(4, "big") + payload
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=0.5)
    try:
        conn.request("POST", "/rpc", body, {"Content-Type": "application/msgpack"})
        response = conn.getresponse()
        data = response.read()
        assert response.status == 200, (response.status, data)
        result = msgpack.unpackb(data[4:])
        assert not result[2], result
        return result[1]
    finally:
        conn.close()


def ready_rpc(port):
    try:
        return rpc(port, "daemon_status_ex")
    except (OSError, http.client.HTTPException):
        return None


def rust_listener(port):
    rows = rpc(port, "list_interfaces")["interfaces"]
    record = next(row for row in rows if row["name"] == "honest-rust")
    return record["settings"]["_runtime"]["tcp"]["listener_status"]


def start_rust(args, discover, udp_in, udp_out, backbone_port=None):
    root = args.root / "rust"
    root.mkdir()
    config = (
        "[reticulum]\nshare_instance=false\nenable_transport=true\n"
        f"discover_interfaces={str(discover).lower()}\nrequired_discovery_value=1\n"
        f"autoconnect_discovered_interfaces={4 if discover else 0}\n"
        "[[interfaces]]\ntype='udp'\nname='discovery-bus'\nenabled=true\n"
        f"host='127.0.0.1'\nport={udp_in}\n"
        f"target_host='127.0.0.1'\ntarget_port={udp_out}\n"
    )
    if backbone_port:
        config += (
            "[[interfaces]]\ntype='backbone'\nname='honest-rust'\nenabled=true\n"
            f"host='127.0.0.2'\nport={backbone_port}\ndiscoverable=true\n"
            "reachable_on='127.0.0.2'\ndiscovery_stamp_value=1\n"
        )
    path = root / "config.toml"
    path.write_text(config)
    port = reserve_port()
    with (root / "daemon.log").open("wb") as output:
        child = subprocess.Popen(
            [args.daemon, "--db", str(root / "state.db"), "--config", str(path),
             "--rpc", f"127.0.0.1:{port}", "--rpc-unix", str(root / "rpc.sock")],
            stdout=output, stderr=output, env={**os.environ, "RUST_LOG": "debug"},
        )
    CHILDREN.append(child)
    return port


def start_python(root, discover, override, udp_in, udp_out, backbone_port=None):
    root.mkdir()
    config = (
        "[reticulum]\nshare_instance=no\nenable_transport=yes\n"
        f"discover_interfaces={'yes' if discover else 'no'}\n"
        "required_discovery_value=1\nautoconnect_discovered_interfaces=4\n"
        f"autoconnect_unverified_implementations={'yes' if override else 'no'}\n"
        "[logging]\nloglevel=3\n[interfaces]\n[[discovery-bus]]\n"
        "type=UDPInterface\nenabled=yes\nlisten_ip=127.0.0.1\n"
        f"listen_port={udp_in}\nforward_ip=127.0.0.1\nforward_port={udp_out}\n"
    )
    if backbone_port:
        config += (
            "[[qualified-python]]\ntype=BackboneInterface\nenabled=yes\n"
            f"listen_ip=127.0.0.2\nlisten_port={backbone_port}\n"
            "discoverable=yes\nreachable_on=127.0.0.2\n"
            "discovery_name=qualified-python\ndiscovery_stamp_value=1\n"
        )
    (root / "config").write_text(config)
    instance = RNS.Reticulum(configdir=str(root), loglevel=3)
    assert not instance.is_connected_to_shared_instance
    assert RNS.__version__ == "1.5.5"
    assert LXMF.__version__ == "1.2.0"
    trace(rns=RNS.__version__, rns_module=RNS.__file__, lxmf_module=LXMF.__file__)
    return instance


def named_interface(name):
    return next((iface for iface in RNS.Transport.interfaces if iface.name == name), None)


def publish_python():
    server = named_interface("qualified-python")
    announcer = RNS.Transport.interface_announcer
    payload = announcer.get_interface_announce_data(server)
    assert payload
    announcer.discovery_destination.announce(
        app_data=payload, attached_interface=named_interface("discovery-bus")
    )
    return server


def native_autoconnects():
    return [i for i in RNS.Transport.interfaces if hasattr(i, "autoconnect_hash")]


def discovered_rust():
    rows = RNS.Transport.discovery_handler.list_discovered_interfaces()
    return next((row for row in rows if row["name"] == "honest-rust"), None)


def assert_persisted(info):
    path = Path(RNS.Transport.discovery_handler.storagepath) / info["discovery_hash"].hex()
    stored = msgpack.unpackb(path.read_bytes())
    assert stored["impl_name"] == "LXMF-rs", stored
    assert stored["version"] == os.environ["DISCOVERY_RUST_VERSION"], stored
    trace(persisted_implementation=stored["impl_name"], version=stored["version"],
          discovery_file=str(path), discovery_sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def remove_bus(instance):
    assert instance._detach_interface("discovery-bus", internal_forced=True)
    assert named_interface("discovery-bus") is None
    trace(discovery_udp="detached before destination announce")


def exact_lxmf_delivery(args, instance, rpc_port, peer, deadline, expected_parent=None):
    router = LXMF.LXMRouter(storagepath=str(args.root / "lxmf"))
    destination = router.register_delivery_identity(RNS.Identity(), stamp_cost=None)
    captured, delivered = {}, []
    lock = threading.Lock()

    def capture(data, packet):
        packed = packet.destination.hash + data
        with lock:
            captured[packed] = (packet.receiving_interface, packet.packet_hash.hex())
        router.delivery_packet(data, packet)

    def receive(message):
        with lock:
            delivered.append(message)

    destination.set_packet_callback(capture)
    router.register_delivery_callback(receive)
    remove_bus(instance)
    source = rpc(rpc_port, "daemon_status_ex")["delivery_destination_hash"]
    rpc(rpc_port, "announce_now")
    wait_for(deadline, "Rust source identity on selected native connection", lambda:
             RNS.Identity.recall(bytes.fromhex(source)) is not None
             and RNS.Transport.next_hop_interface(bytes.fromhex(source)) is peer)
    destination.announce(attached_interface=peer)
    path = wait_for(deadline, "Rust native path from selected connection", lambda:
                    next((p for p in rpc(rpc_port, "get_path_table")
                          if p["hash"] == destination.hash.hex()), None))
    rows = rpc(rpc_port, "list_interfaces")["interfaces"]
    selected_name = "qualified-python" if args.case == "rust-default" else "honest-rust"
    selected = next(r for r in rows if r["name"] == selected_name)
    selected_address = selected["settings"]["_runtime"]["iface"].strip("/")
    if args.case == "rust-default":
        assert path["interface_hash"][:32] == selected_address, (path, selected)
    else:
        native = wait_for(deadline, "Rust accepted child in native status", lambda:
                          next((row for row in rpc(rpc_port, "daemon_status_ex")
                                ["reticulum"]["transport"]["interfaces"]
                                if row["address"] == path["interface_hash"][:32]), None))
        assert native["parent"] == selected_address, (path, native, selected)
    title = "discovery\x00title"
    content = "native discovery bytes \x00 é 🛰"
    rpc(rpc_port, "send_message_v2", {
        "id": args.case, "source": source, "destination": destination.hash.hex(),
        "title": title, "content": content, "method": "opportunistic",
    })
    message = wait_for(deadline, "native LXMF callback", lambda: delivered[0] if delivered else None)
    assert message.method == LXMF.LXMessage.OPPORTUNISTIC
    assert message.source_hash.hex() == source
    assert message.destination_hash == destination.hash
    assert message.title == title.encode("utf-8")
    assert message.content == content.encode("utf-8")
    assert message.signature_validated
    received_interface, packet_hash = captured[message.packed]
    assert received_interface is peer, (received_interface, peer)
    if expected_parent is not None:
        assert received_interface.parent_interface is expected_parent
    trace(recipient_content_hex=message.content.hex(), title_hex=message.title.hex(),
          lxmf_hash=message.hash.hex(), packed_sha256=hashlib.sha256(message.packed).hexdigest(),
          packet_hash=packet_hash, receiving_interface=str(peer), native_path=path,
          signature_validated=message.signature_validated, method="opportunistic")
    sender = wait_for(deadline, "separate sender delivery receipt", lambda:
                      next((m for m in rpc(rpc_port, "list_messages", {"limit": 10})["messages"]
                            if m["id"] == args.case and m["receipt_status"] == "delivered"), None))
    trace(sender_message=sender)


def control_publisher(args):
    start_python(args.root / "control", False, False, reserve_port(socket.SOCK_DGRAM),
                 args.consumer_udp, args.backbone_port)
    destination = RNS.Destination(None, RNS.Destination.IN, RNS.Destination.PLAIN,
                                  "discovery-test", "control")
    received = []
    destination.set_packet_callback(lambda data, packet: received.append((data, packet)))
    trace(control_ready=True, destination=destination.hash.hex())
    assert sys.stdin.readline().strip() == "announce"
    server = publish_python()
    data, packet = wait_for(time.monotonic() + TRAFFIC, "control exact recipient bytes",
                            lambda: received[0] if received else None)
    assert data == b"qualified-control\x00native-bytes"
    assert packet.receiving_interface.parent_interface is server
    trace(control_received=data.hex(), receiving_interface=str(packet.receiving_interface))
    sys.stdin.readline()


def positive_control(args, deadline, consumer_udp):
    command = [sys.executable, "-u", __file__, "--case", "control", "--root", str(args.root),
               "--consumer-udp", str(consumer_udp), "--backbone-port", str(reserve_port())]
    output = queue.Queue()
    log = (args.root / "control.log").open("w")
    child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=log, text=True)
    CHILDREN.append(child)

    def collect():
        for line in child.stdout:
            log.write(line)
            log.flush()
            if line.startswith("TRACE "):
                output.put(json.loads(line[6:]))

    threading.Thread(target=collect, daemon=True).start()
    ready = wait_for(deadline, "positive control readiness", lambda:
                     output.get_nowait() if not output.empty() else None)
    while not ready.get("control_ready"):
        ready = wait_for(deadline, "positive control destination", lambda:
                         output.get_nowait() if not output.empty() else None)
    child.stdin.write("announce\n")
    child.stdin.flush()
    peer = wait_for(deadline, "qualified positive native autoconnect", lambda:
                    next((i for i in native_autoconnects() if i.name == "qualified-python" and i.online), None))
    destination = RNS.Destination(None, RNS.Destination.OUT, RNS.Destination.PLAIN,
                                  "discovery-test", "control")
    assert destination.hash.hex() == ready["destination"]
    RNS.Packet(destination, b"qualified-control\x00native-bytes", attached_interface=peer).send()
    received = wait_for(deadline, "positive control recipient", lambda:
                        output.get_nowait() if not output.empty() else None)
    assert received.get("control_received") == b"qualified-control\x00native-bytes".hex(), received
    trace(positive_control=received, native_interface=str(peer))
    return peer


def main(args):
    for module, key in [(RNS, "DISCOVERY_RNS_ROOT"), (LXMF, "DISCOVERY_LXMF_ROOT")]:
        assert Path(module.__file__).resolve().is_relative_to(Path(os.environ[key]).resolve())
    if args.case == "control":
        control_publisher(args)
        return
    ready_deadline = time.monotonic() + READINESS
    rust_udp, python_udp = reserve_port(socket.SOCK_DGRAM), reserve_port(socket.SOCK_DGRAM)
    backbone_port = reserve_port()
    publisher = args.case == "rust-default"
    instance = start_python(args.root / "python", not publisher,
                            args.case == "python-override", python_udp, rust_udp,
                            backbone_port if publisher else None)
    rpc_port = start_rust(args, publisher, rust_udp, python_udp,
                          None if publisher else backbone_port)
    wait_for(ready_deadline, "daemon readiness", lambda: ready_rpc(rpc_port))
    if not publisher:
        wait_for(ready_deadline, "advertised Rust native listener readiness", lambda:
                 rust_listener(rpc_port)["listener_state"] == "listening")
    deadline = time.monotonic() + TRAFFIC
    if publisher:
        server = publish_python()
        peer = wait_for(deadline, "Rust discovered native connection", lambda:
                        next((i for i in server.spawned_interfaces if i.online), None))
        info = wait_for(deadline, "Rust persisted authenticated Python advertisement", lambda:
                        next((i for i in rpc(rpc_port, "discovered_interfaces")
                              if i["name"] == "qualified-python"), None))
        assert info["impl_name"] == "RNS" and info["version"] == "1.5.5", info
        exact_lxmf_delivery(args, instance, rpc_port, peer, deadline, server)
    else:
        info = wait_for(deadline, "authenticated honest Rust discovery", discovered_rust)
        assert_persisted(info)
        if args.case == "python-override":
            peer = wait_for(deadline, "Python override native autoconnect", lambda:
                            next((i for i in native_autoconnects()
                                  if i.target_port == backbone_port and i.online), None))
            exact_lxmf_delivery(args, instance, rpc_port, peer, deadline)
        else:
            control = positive_control(args, min(deadline, ready_deadline), python_udp)
            rust_rows = rpc(rpc_port, "list_interfaces")["interfaces"]
            rust_address = next(row for row in rust_rows if row["name"] == "honest-rust")
            rust_address = rust_address["settings"]["_runtime"]["iface"].strip("/")
            observation_start = time.monotonic()
            observations = 0
            rust_observations = 0
            next_rust_check = observation_start
            while time.monotonic() - observation_start < 10:
                assert time.monotonic() < deadline
                native = native_autoconnects()
                assert control in native and control.online
                assert len(native) < RNS.Reticulum.max_autoconnected_interfaces()
                assert not any(getattr(i, "target_port", None) == backbone_port
                               for i in RNS.Transport.interfaces)
                if time.monotonic() >= next_rust_check:
                    # Native Python sampling stays frequent; RPC respects the unchanged rate limit.
                    status = rust_listener(rpc_port)
                    assert status["listener_state"] == "listening", status
                    assert status["accepted_connections"] == 0, status
                    rust_observations += 1
                    next_rust_check = time.monotonic() + 0.5
                observations += 1
                time.sleep(0.025)
            assert_persisted(discovered_rust())
            native_rust = rpc(rpc_port, "daemon_status_ex")["reticulum"]["transport"]["interfaces"]
            assert not any(row["parent"] == rust_address for row in native_rust), native_rust
            trace(rejection_observed_seconds=time.monotonic() - observation_start,
                  observations=observations, free_slots=4-len(native_autoconnects()),
                  rust_cumulative_listener_samples=rust_observations,
                  rejected_endpoint=f"127.0.0.2:{backbone_port}")
    trace(case=args.case, result="passed")


def watchdog():
    print("topology exceeded 15s readiness + 30s traffic + bounded cleanup", file=sys.stderr, flush=True)
    for child in CHILDREN:
        if child.poll() is None:
            child.kill()
            child.wait(timeout=2)
    os._exit(124)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--daemon")
    parser.add_argument("--consumer-udp", type=int)
    parser.add_argument("--backbone-port", type=int)
    arguments = parser.parse_args()
    timer = threading.Timer(70, watchdog)
    timer.daemon = True
    timer.start()
    try:
        main(arguments)
    finally:
        for process in reversed(CHILDREN):
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=2)
        timer.cancel()
