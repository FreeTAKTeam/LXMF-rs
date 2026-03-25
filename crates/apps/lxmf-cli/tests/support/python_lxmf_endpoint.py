#!/usr/bin/env python3

import argparse
import json
import socketserver
import threading
import time
from pathlib import Path

import LXMF
import RNS


LINK_STATUS_NAMES = {
    RNS.Link.PENDING: "pending",
    RNS.Link.HANDSHAKE: "handshake",
    RNS.Link.ACTIVE: "active",
    RNS.Link.STALE: "stale",
    RNS.Link.CLOSED: "closed",
}

LINK_REASON_NAMES = {
    RNS.Link.TIMEOUT: "timeout",
    RNS.Link.INITIATOR_CLOSED: "initiator_closed",
    RNS.Link.DESTINATION_CLOSED: "destination_closed",
}


class EndpointState:
    def __init__(self, display_name: str, storage: Path):
        self.display_name = display_name
        self.storage = storage
        self.lock = threading.Lock()
        self.messages = []
        self.reticulum = None
        self.router = None
        self.delivery_destination = None
        self.link = None
        self.link_established_count = 0
        self.link_closed_count = 0
        self.last_teardown_reason = None
        self.suppress_keepalive_responses = False

    def start(self, config_dir: str) -> None:
        self.reticulum = RNS.Reticulum(configdir=config_dir, loglevel=7)
        self.router = LXMF.LXMRouter(storagepath=str(self.storage), enforce_stamps=False)
        self.router.register_delivery_callback(self._on_delivery)
        identity = RNS.Identity()
        self.delivery_destination = self.router.register_delivery_identity(
            identity,
            display_name=self.display_name,
        )
        self.delivery_destination.set_link_established_callback(self._on_link_established)

    def _on_delivery(self, message) -> None:
        with self.lock:
            self.messages.append(
                {
                    "source_hash": message.source_hash.hex(),
                    "destination_hash": message.destination_hash.hex(),
                    "title": message.title_as_string(),
                    "content": message.content_as_string(),
                    "timestamp": message.timestamp,
                }
            )

    def _attach_link(self, link) -> None:
        link.set_link_closed_callback(self._on_link_closed)
        if not getattr(link, "_codex_keepalive_wrapper_installed", False):
            original_receive = link.receive

            def wrapped_receive(packet):
                if (
                    self.suppress_keepalive_responses
                    and not link.initiator
                    and packet.context == RNS.Packet.KEEPALIVE
                    and packet.data == bytes([0xFF])
                ):
                    link.watchdog_lock = True
                    try:
                        if packet.receiving_interface != link.attached_interface:
                            RNS.log(
                                (
                                    "Link-associated packet received on unexpected interface "
                                    f"{packet.receiving_interface} instead of {link.attached_interface}!"
                                ),
                                RNS.LOG_ERROR,
                            )
                            return

                        link.last_inbound = time.time()
                        link.rx += 1
                        link.rxbytes += len(packet.data)
                        if link.status == RNS.Link.STALE:
                            link.status = RNS.Link.ACTIVE
                        link._Link__update_phy_stats(packet)
                        return
                    finally:
                        link.watchdog_lock = False

                return original_receive(packet)

            link.receive = wrapped_receive
            link._codex_keepalive_wrapper_installed = True

        with self.lock:
            self.link = link

    def _link_snapshot(self) -> dict:
        with self.lock:
            link = self.link
            established_count = self.link_established_count
            closed_count = self.link_closed_count
            last_teardown_reason = self.last_teardown_reason
            suppress_keepalive_responses = self.suppress_keepalive_responses

        if link is None:
            return {
                "present": False,
                "status": None,
                "status_name": None,
                "initiator": None,
                "established_count": established_count,
                "closed_count": closed_count,
                "teardown_reason": last_teardown_reason,
                "suppress_keepalive_responses": suppress_keepalive_responses,
            }

        reason_code = getattr(link, "teardown_reason", None)
        reason = None
        if reason_code is not None:
            reason = {
                "code": int(reason_code),
                "name": LINK_REASON_NAMES.get(reason_code, str(reason_code)),
            }

        snapshot = {
            "present": True,
            "status": int(link.status),
            "status_name": LINK_STATUS_NAMES.get(link.status, str(link.status)),
            "initiator": bool(link.initiator),
            "established_count": established_count,
            "closed_count": closed_count,
            "teardown_reason": reason or last_teardown_reason,
            "suppress_keepalive_responses": suppress_keepalive_responses,
            "rtt_seconds": float(link.rtt) if link.rtt is not None else None,
            "keepalive_seconds": float(link.keepalive),
            "stale_time_seconds": float(link.stale_time),
            "no_inbound_for_seconds": float(link.no_inbound_for()),
            "no_outbound_for_seconds": float(link.no_outbound_for()),
            "no_data_for_seconds": float(link.no_data_for()),
            "inactive_for_seconds": float(link.inactive_for()),
        }

        link_id = getattr(link, "link_id", None)
        if isinstance(link_id, (bytes, bytearray)):
            snapshot["link_id"] = bytes(link_id).hex()

        activated_at = getattr(link, "activated_at", None)
        if activated_at is not None:
            snapshot["activated_at"] = float(activated_at)

        return snapshot

    def _on_link_established(self, link) -> None:
        self._attach_link(link)
        with self.lock:
            self.link_established_count += 1

    def _on_link_closed(self, link) -> None:
        reason_code = getattr(link, "teardown_reason", None)
        reason = None
        if reason_code is not None:
            reason = {
                "code": int(reason_code),
                "name": LINK_REASON_NAMES.get(reason_code, str(reason_code)),
            }
        with self.lock:
            self.link_closed_count += 1
            self.last_teardown_reason = reason
            self.link = link

    def status(self) -> dict:
        return {
            "delivery_destination_hash": self.delivery_destination.hash.hex(),
            "identity_hash": self.delivery_destination.identity.hash.hex(),
            "inbox_count": len(self.messages),
            "link": self._link_snapshot(),
        }

    def announce(self) -> dict:
        self.router.announce(self.delivery_destination.hash)
        return {"announced": True}

    def list_messages(self) -> dict:
        with self.lock:
            return {"messages": list(self.messages)}

    def wait_message(self, content: str, timeout: float = 60.0) -> dict:
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                for message in self.messages:
                    if message.get("content") == content:
                        return {"message": message}
            time.sleep(0.1)

        raise RuntimeError(f"timed out waiting for inbound message content {content!r}")

    def send_message(self, destination_hex: str, title: str, content: str) -> dict:
        destination_hash = bytes.fromhex(destination_hex)

        if not RNS.Transport.has_path(destination_hash):
            RNS.Transport.request_path(destination_hash)

        deadline = time.time() + 60
        recipient_identity = None
        while time.time() < deadline:
            if RNS.Transport.has_path(destination_hash):
                recipient_identity = RNS.Identity.recall(destination_hash)
                if recipient_identity is not None:
                    break
            time.sleep(0.1)

        if recipient_identity is None:
            raise RuntimeError(f"timed out waiting for path/identity to {destination_hex}")

        destination = RNS.Destination(
            recipient_identity,
            RNS.Destination.OUT,
            RNS.Destination.SINGLE,
            "lxmf",
            "delivery",
        )
        message = LXMF.LXMessage(
            destination,
            self.delivery_destination,
            content,
            title,
            desired_method=LXMF.LXMessage.DIRECT,
            include_ticket=True,
        )
        self.router.handle_outbound(message)
        return {"accepted": True, "destination": destination_hex}

    def open_link(self, destination_hex: str, timeout: float = 60.0) -> dict:
        destination_hash = bytes.fromhex(destination_hex)

        deadline = time.time() + timeout
        recipient_identity = None
        while time.time() < deadline:
            if not RNS.Transport.has_path(destination_hash):
                RNS.Transport.request_path(destination_hash)
            if RNS.Transport.has_path(destination_hash):
                recipient_identity = RNS.Identity.recall(destination_hash)
                if recipient_identity is not None:
                    break
            time.sleep(0.1)

        if recipient_identity is None:
            raise RuntimeError(f"timed out waiting for path/identity to {destination_hex}")

        destination = RNS.Destination(
            recipient_identity,
            RNS.Destination.OUT,
            RNS.Destination.SINGLE,
            "lxmf",
            "delivery",
        )
        link = RNS.Link(
            destination,
            established_callback=self._on_link_established,
            closed_callback=self._on_link_closed,
        )
        self._attach_link(link)
        return self.wait_link_state("active", timeout)

    def link_status(self) -> dict:
        return self._link_snapshot()

    def wait_link_state(self, state: str, timeout: float = 60.0) -> dict:
        deadline = time.time() + timeout
        while time.time() < deadline:
            snapshot = self._link_snapshot()
            if snapshot.get("status_name") == state:
                return snapshot
            time.sleep(0.1)

        raise RuntimeError(f"timed out waiting for link state {state!r}")

    def teardown_link(self) -> dict:
        with self.lock:
            link = self.link
        if link is None:
            raise RuntimeError("no link available to tear down")
        link.teardown()
        return self._link_snapshot()

    def set_keepalive_responses(self, enabled: bool) -> dict:
        with self.lock:
            self.suppress_keepalive_responses = not enabled
        return self._link_snapshot()


class ControlServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True

    def __init__(self, server_address, handler_class, state: EndpointState):
        super().__init__(server_address, handler_class)
        self.state = state


class ControlHandler(socketserver.StreamRequestHandler):
    def handle(self) -> None:
        line = self.rfile.readline()
        if not line:
            return

        try:
            request = json.loads(line.decode("utf-8"))
            method = request.get("method")
            params = request.get("params")
            if params is None:
                params = {}

            if method == "status":
                result = self.server.state.status()
            elif method == "announce":
                result = self.server.state.announce()
            elif method == "list_messages":
                result = self.server.state.list_messages()
            elif method == "wait_message":
                result = self.server.state.wait_message(
                    params["content"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "send_message":
                result = self.server.state.send_message(
                    params["destination"],
                    params.get("title", ""),
                    params.get("content", ""),
                )
            elif method == "open_link":
                result = self.server.state.open_link(
                    params["destination"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "link_status":
                result = self.server.state.link_status()
            elif method == "wait_link_state":
                result = self.server.state.wait_link_state(
                    params["state"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "teardown_link":
                result = self.server.state.teardown_link()
            elif method == "set_keepalive_responses":
                result = self.server.state.set_keepalive_responses(
                    bool(params.get("enabled", True))
                )
            else:
                raise RuntimeError(f"unknown method: {method}")

            response = {"ok": True, "result": result}
        except Exception as exc:
            response = {"ok": False, "error": str(exc)}

        self.wfile.write(json.dumps(response).encode("utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--name", required=True)
    parser.add_argument("--display-name", required=True)
    parser.add_argument("--rnsconfig", required=True)
    parser.add_argument("--storage", required=True)
    parser.add_argument("--control-port", type=int, required=True)
    args = parser.parse_args()

    storage = Path(args.storage)
    storage.mkdir(parents=True, exist_ok=True)

    state = EndpointState(args.display_name, storage)
    state.start(args.rnsconfig)

    with ControlServer(("127.0.0.1", args.control_port), ControlHandler, state) as server:
        server.serve_forever(poll_interval=0.1)


if __name__ == "__main__":
    main()
