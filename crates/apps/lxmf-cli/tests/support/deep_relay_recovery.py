"""Three fresh five-process topologies; frozen bounds and cells are in TRANSPORT.md."""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time

import LXMF
import RNS
from RNS.vendor import umsgpack as msgpack

READY_SECONDS = 60
PATH_SECONDS = 30
CELL_SECONDS = 60
CHILDREN = []
RESERVED = set()


def trace(**values):
    print("TRACE " + json.dumps(values, sort_keys=True), flush=True)


def port():
    while True:
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            value = sock.getsockname()[1]
        if value not in RESERVED:
            RESERVED.add(value)
            return value


def call(target, method, params, deadline, python=False):
    remaining = deadline - time.monotonic()
    assert remaining > 0, f"deadline before {method}"
    if python:
        with socket.create_connection(("127.0.0.1", target), timeout=min(1, remaining)) as stream:
            stream.settimeout(max(0.001, deadline-time.monotonic()))
            stream.sendall(json.dumps({"method": method, "params": params}).encode()+b"\n")
            data = bytearray()
            while True:
                remaining = deadline-time.monotonic()
                assert remaining > 0, method
                stream.settimeout(remaining)
                chunk = stream.recv(65536)
                if not chunk:
                    break
                data.extend(chunk)
        response = json.loads(data)
        assert response["ok"], (method, response)
        assert time.monotonic() < deadline, f"late Python response: {method}"
        return response["result"]
    conn = http.client.HTTPConnection("127.0.0.1", target, timeout=min(5, remaining))
    try:
        data = msgpack.packb([1, method, params])
        conn.request("POST", "/rpc", len(data).to_bytes(4, "big")+data,
                     {"Content-Type": "application/msgpack"})
        assert conn.sock is not None
        remaining = deadline-time.monotonic()
        assert remaining > 0, method
        conn.sock.settimeout(remaining)
        response = conn.getresponse()
        assert response.status == 200
        payload = response.read()
        decoded = msgpack.unpackb(payload[4:])
        assert not decoded[2], (method, decoded)
        assert time.monotonic() < deadline, f"late daemon response: {method}"
        return decoded[1]
    finally:
        conn.close()


def wait_for(deadline, label, probe):
    while time.monotonic() < deadline:
        result = probe()
        if result:
            assert time.monotonic() < deadline, f"late probe: {label}"
            return result
        for process in CHILDREN:
            assert process.poll() is None, f"process {process.pid} exited {process.returncode}: {label}"
        time.sleep(0.25)
    raise AssertionError(f"deadline waiting for {label}")


def status(target, deadline, python=False):
    try:
        return call(target, "status" if python else "daemon_status_ex", None, deadline, python)
    except (OSError, http.client.HTTPException):
        return None


def spawn(command, directory):
    directory.mkdir(parents=True, exist_ok=True)
    with (directory/"stderr.log").open("wb") as log:
        process = subprocess.Popen(command, stdout=log, stderr=log, env={
            **os.environ, "PYTHONUNBUFFERED": "1", "RUST_LOG": "reticulumd=debug,rns_transport=debug",
        })
    CHILDREN.append(process)
    return process


def python_endpoint(root, name, config, control):
    root.mkdir()
    rns = root/"rns"
    rns.mkdir()
    (rns/"config").write_text(config)
    return spawn([sys.executable, "-u", str(Path(__file__).with_name("python_lxmf_endpoint.py")),
                  "--name", name, "--display-name", name, "--rnsconfig", str(rns),
                  "--storage", str(root/"lxmf"), "--control-port", str(control), "--native-lxmf"], root)


def rust_relay(binary, root, rpc, listener, upstream, shared=False):
    root.mkdir()
    assert not list(root.iterdir()), "replacement starts with no persisted route/hash state"
    kind = "local_client" if shared else "tcp_client"
    config = ("[reticulum]\nshare_instance=false\nenable_transport=true\n"
              "[[interfaces]]\nenabled=true\nname='uplink'\n"
              f"type='{kind}'\nhost='127.0.0.1'\nport={upstream}\n")
    if shared:
        config += "shared_instance_type='tcp'\nfixed_mtu=262144\nforce_shared_instance_bitrate=1000000\n"
    (root/"config.toml").write_text(config)
    process = spawn([binary, "--db", str(root/"state.db"), "--identity", str(root/"identity"),
                     "--config", str(root/"config.toml"), "--rpc", f"127.0.0.1:{rpc}",
                     "--transport", f"127.0.0.1:{listener}", "--rpc-unix", str(root/"rpc.sock")], root)
    return process


def connected(target, deadline, attached=False):
    rows = call(target, "list_interfaces", None, deadline)["interfaces"]
    row = next(i for i in rows if i["name"] == "uplink")
    runtime = row["settings"]["_runtime"]
    return runtime["startup_status"] == "attached" if attached else (
        runtime.get("tcp", {}).get("stream_status", {}).get("stream_state") == "connected")


def paths(controls, hashes, relays, middle_identity):
    deadline = time.monotonic()+PATH_SECONDS
    for target in controls:
        call(target, "announce", None, deadline, True)
    for target, destination in zip(controls, reversed(hashes)):
        call(target, "request_path", {"destination": destination}, deadline, True)

    def learned():
        for target, destination in zip(controls, reversed(hashes)):
            entry = call(target, "path_snapshot", {"destination": destination}, deadline, True)
            if not entry["known"]:
                return False
            assert entry["hops"] >= 3, entry
        for index, target in enumerate(relays):
            rows = call(target, "get_path_table", None, deadline)
            if not all(any(row["hash"] == destination for row in rows) for destination in hashes):
                return False
            if index in (0, 2):
                remote = hashes[1] if index == 0 else hashes[0]
                if not any(row["hash"] == remote and row["via"] == middle_identity for row in rows):
                    return False
        return True

    wait_for(deadline, "fresh bidirectional paths through all three Rust relays", learned)
    trace(paths=[call(target, "get_path_table", None, deadline) for target in relays],
          endpoint_paths=[call(target, "path_snapshot", {"destination": destination}, deadline, True)
                          for target, destination in zip(controls, reversed(hashes))])


def content(run, phase, direction, large):
    marker = f"deep:{run}:{phase}:{direction}:"
    if not large:
        return marker+"packet\x00é"
    seed = hashlib.shake_256(marker.encode()).hexdigest(65551)
    return (marker+seed)[:131101]


def deliver(run, phase, controls, hashes):
    verified = []
    for large in (False, True):
        for direction in (0, 1):
            deadline = time.monotonic()+CELL_SECONDS
            sender, receiver = controls[direction], controls[1-direction]
            text = content(run, phase, direction, large)
            title = f"run-{run}:{phase}:{'resource' if large else 'packet'}:{direction}"
            sent = call(sender, "send_message", {
                "destination": hashes[1-direction], "title": title, "content": text,
                "method": "direct", "wait_for_path": False,
            }, deadline, True)
            message_hash = sent["message_hash"]

            def recipient():
                messages = call(receiver, "list_messages", None, deadline, True)["messages"]
                matching = [m for m in messages if m["message_hash"] == message_hash]
                assert len(matching) <= 1, ("duplicate recipient", message_hash)
                return matching[0] if matching else None

            received = wait_for(deadline, "exact LXMF recipient", recipient)
            assert received["content"] == text and received["title"] == title
            assert received["source_hash"] == hashes[direction]
            assert received["destination_hash"] == hashes[1-direction]
            assert received["method"] == LXMF.LXMessage.DIRECT
            assert received["signature_validated"]
            assert received["content_sha256"] == hashlib.sha256(text.encode()).hexdigest()

            def sender_complete():
                result = call(sender, "outbound_status", {"message_hash": message_hash}, deadline, True)
                assert result["state_name"] not in ("rejected", "cancelled", "failed"), result
                return result if result["state_name"] == "delivered" else None

            completed = wait_for(deadline, "separate sender completion", sender_complete)
            assert completed["method"] == LXMF.LXMessage.DIRECT
            assert completed["representation"] == (LXMF.LXMessage.RESOURCE if large else LXMF.LXMessage.PACKET)
            assert len(completed["handoff_link_id"]) == 32, completed
            if large:
                assert len(completed["resource_id"]) == 64 and len(completed["link_id"]) == 32, completed
                assert completed["link_id"] == completed["handoff_link_id"], completed
            else:
                assert completed["resource_id"] is None, completed
            verified.append(message_hash)
            trace(run=run, phase=phase, direction=direction, content_bytes=len(text.encode()),
                  content_sha256=received["content_sha256"], message_hash=message_hash,
                  sender_completion=completed, signature_validated=True,
                  elapsed_seconds=CELL_SECONDS-(deadline-time.monotonic()))
    return verified


def topology(args, run):
    root = args.root/f"run-{run}"
    root.mkdir()
    shared, rns_control = port(), port()
    controls = [port(), port()]
    rpcs, listeners = [port() for _ in range(3)], [port() for _ in range(3)]
    ready = time.monotonic()+READY_SECONDS
    python_a = python_endpoint(root/"python-a", "deep-shared-owner", (
        "[reticulum]\nenable_transport=no\nshare_instance=yes\nshared_instance_type=tcp\n"
        f"shared_instance_port={shared}\ninstance_control_port={rns_control}\ndiscover_interfaces=no\n"
        "[logging]\nloglevel=7\n"), controls[0])
    a = wait_for(ready, "Python shared owner", lambda: status(controls[0], ready, True))
    assert a["reticulum"]["is_shared_instance"]
    edge_a = rust_relay(args.daemon, root/"edge-a", rpcs[0], listeners[0], shared, True)
    edge_a_identity = wait_for(ready, "edge A RPC", lambda: status(rpcs[0], ready))["identity_hash"]
    wait_for(ready, "edge A shared attachment", lambda: connected(rpcs[0], ready, True))
    middle = rust_relay(args.daemon, root/"middle-before", rpcs[1], listeners[1], listeners[0])
    first_middle = wait_for(ready, "middle RPC", lambda: status(rpcs[1], ready))["identity_hash"]
    wait_for(ready, "middle uplink", lambda: connected(rpcs[1], ready))
    edge_c = rust_relay(args.daemon, root/"edge-c", rpcs[2], listeners[2], listeners[1])
    edge_c_identity = wait_for(ready, "edge C RPC", lambda: status(rpcs[2], ready))["identity_hash"]
    wait_for(ready, "edge C uplink", lambda: connected(rpcs[2], ready))
    python_b = python_endpoint(root/"python-b", "deep-remote-client", (
        "[reticulum]\nenable_transport=no\nshare_instance=no\ndiscover_interfaces=no\n"
        "[logging]\nloglevel=7\n[interfaces]\n[[edge-c]]\ntype=TCPClientInterface\n"
        f"enabled=yes\ntarget_host=127.0.0.1\ntarget_port={listeners[2]}\n"), controls[1])
    b = wait_for(ready, "Python remote client", lambda: status(controls[1], ready, True))
    hashes = [a["delivery_destination_hash"], b["delivery_destination_hash"]]
    for endpoint in (a, b):
        assert Path(endpoint["references"]["rns"]).resolve().is_relative_to(Path(os.environ["DEEP_RNS_ROOT"]))
        assert Path(endpoint["references"]["lxmf"]).resolve().is_relative_to(Path(os.environ["DEEP_LXMF_ROOT"]))
    trace(run=run, topology="Python shared owner -> Rust A -> Rust B -> Rust C -> Python client",
          rpc_ports=rpcs, transport_ports=listeners, endpoints=hashes)
    paths(controls, hashes, rpcs, first_middle)
    verified = deliver(run, "before", controls, hashes)
    links_before = [call(target, "status", None, time.monotonic()+5, True)["native_links"]
                    for target in controls]
    assert all(links_before), links_before
    middle.terminate()
    middle.wait(timeout=10)
    assert middle.poll() is not None
    CHILDREN.remove(middle)
    reconnect_deadline = time.monotonic()+PATH_SECONDS
    replacement = rust_relay(args.daemon, root/"middle-after", rpcs[1], listeners[1], listeners[0])
    middle_after = wait_for(reconnect_deadline, "replacement middle RPC", lambda:
                           status(rpcs[1], reconnect_deadline))["identity_hash"]
    assert middle_after != first_middle
    for target in rpcs[1:]:
        wait_for(reconnect_deadline, "native reconnect", lambda: connected(target, reconnect_deadline))
    assert all(process.poll() is None for process in (python_a, python_b, edge_a, edge_c))
    assert call(rpcs[0], "daemon_status_ex", None, reconnect_deadline)["identity_hash"] == edge_a_identity
    assert call(rpcs[2], "daemon_status_ex", None, reconnect_deadline)["identity_hash"] == edge_c_identity
    for target, expected in zip(controls, hashes):
        assert call(target, "status", None, reconnect_deadline, True)["delivery_destination_hash"] == expected
    trace(run=run, replaced_pid=middle.pid, replacement_pid=replacement.pid,
          old_middle_identity=first_middle, new_middle_identity=middle_after,
          fresh_state_directory=str(root/"middle-after"), state_restored=False,
          retained_endpoint_pids=[python_a.pid, python_b.pid], retained_edge_pids=[edge_a.pid, edge_c.pid],
          native_links_before=links_before,
          native_links_after_replacement=[call(target, "status", None, reconnect_deadline, True)["native_links"]
                                          for target in controls])
    paths(controls, hashes, rpcs, middle_after)
    verified.extend(deliver(run, "after", controls, hashes))
    final_deadline = time.monotonic()+5
    for control in controls:
        messages = call(control, "list_messages", None, final_deadline, True)["messages"]
        assert len(messages) == 4, ("exactly four recipient messages per endpoint", len(messages))
        assert len({message["message_hash"] for message in messages}) == 4
    assert len(set(verified)) == 8
    trace(run=run, verified_messages=8)
    return verified


def stop_all():
    for process in reversed(CHILDREN):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)
    CHILDREN.clear()


def watchdog():
    print("recovery topology exceeded its declared total bound", file=sys.stderr, flush=True)
    for process in CHILDREN:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=2)
    os._exit(124)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--daemon", required=True)
    args = parser.parse_args()
    for module, key in [(RNS, "DEEP_RNS_ROOT"), (LXMF, "DEEP_LXMF_ROOT")]:
        assert Path(module.__file__).resolve().is_relative_to(Path(os.environ[key]).resolve())
    trace(rns_module=RNS.__file__, lxmf_module=LXMF.__file__, rns_version=RNS.__version__,
          lxmf_version=LXMF.__version__, readiness_seconds=READY_SECONDS,
          path_seconds=PATH_SECONDS, shared_message_seconds=CELL_SECONDS)
    with open(args.daemon, "rb") as binary:
        trace(immutable_daemon_sha256=hashlib.file_digest(binary, "sha256").hexdigest())
    verified = []
    for run in range(1, 4):
        timer = threading.Timer(710, watchdog)
        timer.daemon = True
        timer.start()
        try:
            verified.extend(topology(args, run))
        finally:
            stop_all()
            timer.cancel()
    assert len(set(verified)) == 24
    trace(verified_messages=24, fresh_topologies=3, result="passed")


if __name__ == "__main__":
    main()
