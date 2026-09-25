#!/usr/bin/env python3

import argparse
import hashlib
import json
import socketserver
import sys
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


RAW_LINK_TEST_PREFIX = b"\x00lxmf-rs-test:"

OUTBOUND_STATE_NAMES = {
    LXMF.LXMessage.OUTBOUND: "outbound",
    LXMF.LXMessage.SENDING: "sending",
    LXMF.LXMessage.SENT: "sent",
    LXMF.LXMessage.DELIVERED: "delivered",
    LXMF.LXMessage.REJECTED: "rejected",
    LXMF.LXMessage.CANCELLED: "cancelled",
    LXMF.LXMessage.FAILED: "failed",
}


class EndpointState:
    def __init__(self, display_name: str, storage: Path):
        self.display_name = display_name
        self.storage = storage
        self.lock = threading.Lock()
        self.messages = []
        self.outbound_messages = {}
        self.reticulum = None
        self.router = None
        self.delivery_destination = None
        self.raw_link = None
        self.raw_messages = []
        self.raw_resources = []
        self.link = None
        self.link_established_count = 0
        self.link_closed_count = 0
        self.last_teardown_reason = None
        self.suppress_keepalive_responses = False
        self.resource_gate_enabled = False
        self.resource_gate_reached = threading.Event()
        self.resource_gate_release = threading.Event()
        self.resource_gate_observations = []
        self.resource_gate_target = None
        self.resource_gate_installed = False
        self.preserve_router_resource_callback = False
        self.resource_request_original = RNS.Resource.request
        self.outbound_packet_drop_target = None
        self.outbound_packet_drop_count = 0
        self.outbound_packet_drop_event = threading.Event()
        self.outbound_retry_window_open = False
        self.outbound_retry_packet_count = 0
        self.outbound_retry_packet_event = threading.Event()

    def start(self, config_dir: str) -> None:
        print("python_lxmf_endpoint: starting Reticulum", file=sys.stderr, flush=True)
        self.reticulum = RNS.Reticulum(configdir=config_dir, loglevel=7)
        print("python_lxmf_endpoint: creating LXMRouter", file=sys.stderr, flush=True)
        self.router = LXMF.LXMRouter(storagepath=str(self.storage), enforce_stamps=False)
        print("python_lxmf_endpoint: registering delivery callback", file=sys.stderr, flush=True)
        self.router.register_delivery_callback(self._on_delivery)
        identity = RNS.Identity()
        print("python_lxmf_endpoint: registering delivery identity", file=sys.stderr, flush=True)
        self.delivery_destination = self.router.register_delivery_identity(
            identity,
            display_name=self.display_name,
        )
        print("python_lxmf_endpoint: installing link callback", file=sys.stderr, flush=True)
        router_link_established = self.delivery_destination.callbacks.link_established

        def observe_delivery_link(link):
            if router_link_established is not None:
                router_link_established(link)
            self._on_link_established(link)
            self._on_raw_link_established(link)

        self.delivery_destination.set_link_established_callback(observe_delivery_link)
        print("python_lxmf_endpoint: endpoint state ready", file=sys.stderr, flush=True)

    def _install_resource_gate(self) -> None:
        if self.resource_gate_installed:
            return
        original_request = self.resource_request_original

        def observe_request(resource, request_data):
            if not getattr(resource, "initiator", False):
                return original_request(resource, request_data)

            link_id = getattr(resource.link, "link_id", b"").hex()
            key = (resource.hash.hex(), link_id)
            is_first_gated_request = False
            is_gated_resource = False
            with self.lock:
                if self.resource_gate_enabled:
                    if self.resource_gate_target is None:
                        self.resource_gate_target = key
                        is_first_gated_request = True
                        is_gated_resource = True
                    elif key == self.resource_gate_target:
                        is_gated_resource = True

            if is_gated_resource and not is_first_gated_request:
                # Do not let a later receiver request advance the selected Resource
                # before Rust has stopped the relay at the first-batch barrier.
                if not self.resource_gate_reached.wait(120):
                    raise RuntimeError("first Resource request did not reach the gate")
                if not self.resource_gate_release.wait(180):
                    raise RuntimeError("timed out waiting for the Rust test to release the Resource gate")

            original_request(resource, request_data)
            observation = {
                "resource_id": resource.hash.hex(),
                "link_id": link_id,
                "status": resource.status,
                "sent_parts": resource.sent_parts,
                "total_parts": len(resource.parts),
            }
            should_pause = False
            with self.lock:
                if not self.resource_gate_enabled:
                    return None
                if key not in [
                    (item["resource_id"], item["link_id"])
                    for item in self.resource_gate_observations
                ]:
                    self.resource_gate_observations.append(observation)
                if is_first_gated_request:
                    should_pause = True
                    self.resource_gate_reached.set()

            if should_pause:
                self.resource_gate_release.wait(180)

        RNS.Resource.request = observe_request
        self.resource_gate_installed = True

    def arm_resource_gate(self) -> dict:
        with self.lock:
            self.resource_gate_observations = []
            self.resource_gate_target = None
            self.resource_gate_reached.clear()
            self.resource_gate_release.clear()
            self.resource_gate_enabled = True
        self._install_resource_gate()
        return {"armed": True}

    def wait_resource_gate(self, timeout: float = 120.0) -> dict:
        if not self.resource_gate_reached.wait(timeout):
            raise RuntimeError("timed out waiting for an in-flight LXMF Resource")
        with self.lock:
            observation = dict(self.resource_gate_observations[0])
        if observation["status"] != RNS.Resource.TRANSFERRING:
            raise RuntimeError(f"gated Resource was not transferring: {observation}")
        if not 0 < observation["sent_parts"] < observation["total_parts"]:
            raise RuntimeError(f"gated Resource was not incomplete: {observation}")
        return observation

    def release_resource_gate(self) -> dict:
        self.resource_gate_release.set()
        with self.lock:
            self.resource_gate_enabled = False
            if self.resource_gate_installed:
                RNS.Resource.request = self.resource_request_original
                self.resource_gate_installed = False
        return {"released": True}

    def resource_gate_snapshot(self) -> dict:
        with self.lock:
            return {
                "target": self.resource_gate_target,
                "reached": self.resource_gate_reached.is_set(),
                "observations": list(self.resource_gate_observations),
            }

    def set_router_resource_callback_chaining(self, enabled: bool) -> dict:
        with self.lock:
            self.preserve_router_resource_callback = enabled
        return {"enabled": enabled}

    def arm_outbound_packet_drop(self, destination: str) -> dict:
        destination_hash = bytes.fromhex(destination)
        if len(destination_hash) != RNS.Reticulum.TRUNCATED_HASHLENGTH // 8:
            raise ValueError("outbound packet-drop destination has an invalid hash length")
        original_outbound = RNS.Transport._outbound
        with self.lock:
            self.outbound_packet_drop_target = destination_hash
            self.outbound_packet_drop_count = 0
            self.outbound_packet_drop_event.clear()
            self.outbound_retry_window_open = False
            self.outbound_retry_packet_count = 0
            self.outbound_retry_packet_event.clear()

        def drop_first_lxmf_packet(packet):
            should_drop = False
            with self.lock:
                if (
                    self.outbound_packet_drop_target == packet.destination_hash
                    and packet.packet_type == RNS.Packet.DATA
                    and packet.context == RNS.Packet.NONE
                    and self.outbound_packet_drop_count == 0
                ):
                    self.outbound_packet_drop_count = 1
                    should_drop = True
                    self.outbound_packet_drop_event.set()
            if should_drop:
                return False
            sent = original_outbound(packet)
            if (
                sent
                and packet.destination_hash == self.outbound_packet_drop_target
                and packet.packet_type == RNS.Packet.DATA
                and packet.context == RNS.Packet.NONE
            ):
                with self.lock:
                    if self.outbound_retry_window_open:
                        self.outbound_retry_packet_count += 1
                        self.outbound_retry_packet_event.set()
            return sent

        RNS.Transport._outbound = staticmethod(drop_first_lxmf_packet)
        return {"armed": True, "destination": destination_hash.hex()}

    def wait_outbound_packet_drop(self, timeout: float = 10.0) -> dict:
        if not self.outbound_packet_drop_event.wait(timeout):
            raise RuntimeError("timed out waiting for the selected outbound LXMF packet drop")
        with self.lock:
            return {
                "dropped_packets": self.outbound_packet_drop_count,
                "destination": self.outbound_packet_drop_target.hex(),
            }

    def mark_outbound_retry_window(self) -> dict:
        with self.lock:
            self.outbound_retry_window_open = True
        return {"opened": True}

    def wait_outbound_retry_packet(self, timeout: float = 30.0) -> dict:
        if not self.outbound_retry_packet_event.wait(timeout):
            raise RuntimeError("timed out waiting for a post-replacement LXMF retry packet")
        with self.lock:
            return {
                "transmitted_packets": self.outbound_retry_packet_count,
                "destination": self.outbound_packet_drop_target.hex(),
            }

    def _on_raw_link_established(self, link) -> None:
        link.set_resource_strategy(RNS.Link.ACCEPT_ALL)
        router_resource_concluded = link.callbacks.resource_concluded

        def on_resource_concluded(resource) -> None:
            if resource.status == RNS.Resource.COMPLETE:
                resource.data.seek(0)
                data = resource.data.read()
                with self.lock:
                    self.raw_resources.append({
                        "size": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(),
                        "metadata": resource.metadata,
                    })
                resource.data.seek(0)
            if self.preserve_router_resource_callback and router_resource_concluded is not None:
                router_resource_concluded(resource)

        link.set_resource_concluded_callback(on_resource_concluded)
        if not getattr(link, "_codex_raw_packet_capture_installed", False):
            original_packet_callback = link.callbacks.packet

            def observe_packet(data, packet):
                payload = bytes(data)
                if payload.startswith(RAW_LINK_TEST_PREFIX):
                    self._on_raw_packet(payload[len(RAW_LINK_TEST_PREFIX) :], packet)
                    return
                try:
                    if original_packet_callback is not None:
                        original_packet_callback(data, packet)
                finally:
                    self._on_raw_packet(data, packet)

            link.set_packet_callback(observe_packet)
            link._codex_raw_packet_capture_installed = True
        with self.lock:
            self.raw_link = link

    def _on_raw_packet(self, data, _packet) -> None:
        with self.lock:
            self.raw_messages.append(bytes(data).hex())

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
            "reticulum": {
                "is_shared_instance": bool(getattr(self.reticulum, "is_shared_instance", False)),
                "is_connected_to_shared_instance": bool(
                    getattr(self.reticulum, "is_connected_to_shared_instance", False)
                ),
                "shared_instance_type": getattr(self.reticulum, "shared_instance_type", None),
            },
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

    def wait_message_prefix(self, prefix: str, timeout: float = 60.0) -> dict:
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                matching = [
                    message for message in self.messages
                    if message.get("content", "").startswith(prefix)
                ]
                if matching:
                    return {"count": len(matching)}
            time.sleep(0.05)
        raise RuntimeError(f"timed out waiting for inbound message prefix {prefix!r}")

    def send_message(
        self,
        destination_hex: str,
        title: str,
        content: str,
        wait_for_path: bool = True,
        method: str = "direct",
        content_bytes: int = 0,
    ) -> dict:
        if content_bytes:
            marker = f"LXMF-RS-INFLIGHT-RESOURCE-{self.display_name}:"
            payload_bytes = content_bytes - len(marker)
            if payload_bytes < 0:
                raise ValueError("content_bytes is smaller than the in-flight marker")
            digest = hashlib.shake_256(
                f"lxmf-rs-inflight-resource:{self.display_name}".encode("utf-8")
            ).hexdigest((payload_bytes + 1) // 2)[:payload_bytes]
            content = marker + digest
            if len(content.encode("utf-8")) != content_bytes:
                raise RuntimeError("generated test message content has an unexpected UTF-8 byte length")
        destination_hash = bytes.fromhex(destination_hex)

        if wait_for_path:
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
        else:
            recipient_identity = RNS.Identity.recall(destination_hash)

        if recipient_identity is None:
            raise RuntimeError(
                f"no cached identity for {destination_hex} while sending without a path wait"
            )

        desired_method = {
            "direct": LXMF.LXMessage.DIRECT,
            "opportunistic": LXMF.LXMessage.OPPORTUNISTIC,
        }.get(method)
        if desired_method is None:
            raise RuntimeError(f"unsupported LXMF delivery method {method!r}")

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
            desired_method=desired_method,
            include_ticket=True,
        )
        self.router.handle_outbound(message)
        message_hash = message.hash.hex()
        with self.lock:
            self.outbound_messages[message_hash] = message
        return {
            "accepted": True,
            "destination": destination_hex,
            "message_hash": message_hash,
        }

    def outbound_status(self, message_hash: str) -> dict:
        with self.lock:
            message = self.outbound_messages.get(message_hash)
        if message is None:
            raise RuntimeError(f"unknown outbound message {message_hash}")

        state = message.state
        return {
            "message_hash": message_hash,
            "destination": message.destination_hash.hex(),
            "state": state,
            "state_name": OUTBOUND_STATE_NAMES.get(state, f"unknown_{state}"),
            "delivery_attempts": message.delivery_attempts,
            "progress": message.progress,
            "resource_id": getattr(message.resource_representation, "hash", b"").hex()
            if message.resource_representation is not None
            else None,
            "link_id": getattr(
                getattr(message.resource_representation, "link", None), "link_id", b""
            ).hex()
            if message.resource_representation is not None
            else None,
        }

    def wait_outbound_state(
        self,
        message_hash: str,
        expected_state: str,
        timeout: float = 60.0,
    ) -> dict:
        deadline = time.time() + timeout
        while time.time() < deadline:
            status = self.outbound_status(message_hash)
            if status["state_name"] == expected_state:
                return status
            if status["state_name"] in {"rejected", "cancelled", "failed"}:
                raise RuntimeError(f"outbound message reached terminal state: {status}")
            time.sleep(0.1)

        raise RuntimeError(
            f"outbound message did not reach {expected_state!r} within {timeout}s: "
            f"{self.outbound_status(message_hash)}"
        )

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

    def wait_path(self, destination_hex: str, timeout: float = 60.0) -> dict:
        destination_hash = bytes.fromhex(destination_hex)
        deadline = time.time() + timeout
        requested = False
        while time.time() < deadline:
            if RNS.Transport.has_path(destination_hash):
                recipient_identity = RNS.Identity.recall(destination_hash)
                if recipient_identity is not None:
                    return {
                        "path_found": True,
                        "destination": destination_hex,
                        "identity_hash": recipient_identity.hash.hex(),
                    }
            if not requested:
                RNS.Transport.request_path(destination_hash)
                requested = True
            time.sleep(0.1)

        path_found = RNS.Transport.has_path(destination_hash)
        identity_found = RNS.Identity.recall(destination_hash) is not None
        raise RuntimeError(
            f"timed out waiting for path/identity to {destination_hex} "
            f"(path_found={path_found}, identity_found={identity_found})"
        )

    def path_snapshot(self, destination_hex: str) -> dict:
        destination_hash = bytes.fromhex(destination_hex)
        with RNS.Transport.path_table_lock:
            entry = RNS.Transport.path_table.get(destination_hash)
            if entry is None:
                return {"known": False, "destination": destination_hex}

            receiving_interface = entry[5]
            return {
                "known": True,
                "destination": destination_hex,
                "timestamp": entry[0],
                "next_hop": bytes(entry[1]).hex() if entry[1] is not None else None,
                "hops": int(entry[2]),
                "interface": getattr(receiving_interface, "name", str(receiving_interface)),
                "interface_type": type(receiving_interface).__name__,
            }

    def request_path(self, destination_hex: str) -> dict:
        destination_hash = bytes.fromhex(destination_hex)
        RNS.Transport.request_path(destination_hash)
        return {"requested": True, "destination": destination_hex}

    def open_raw_link(self, destination_hex: str, timeout: float = 60.0) -> dict:
        destination_hash = bytes.fromhex(destination_hex)
        path_deadline = time.time() + timeout
        identity = None
        while time.time() < path_deadline:
            if not RNS.Transport.has_path(destination_hash):
                RNS.Transport.request_path(destination_hash)
            if RNS.Transport.has_path(destination_hash):
                identity = RNS.Identity.recall(destination_hash)
                if identity is not None:
                    break
            time.sleep(0.1)

        if identity is None:
            raise RuntimeError(f"timed out waiting for raw link destination {destination_hex}")

        destination = RNS.Destination(
            identity,
            RNS.Destination.OUT,
            RNS.Destination.SINGLE,
            "lxmf",
            "delivery",
        )
        link = RNS.Link(destination, established_callback=self._on_raw_link_established)
        with self.lock:
            self.raw_link = link

        link_deadline = time.time() + timeout
        while time.time() < link_deadline:
            if link.status == RNS.Link.ACTIVE:
                return {"status": "active", "link_id": link.link_id.hex()}
            if link.status == RNS.Link.CLOSED:
                raise RuntimeError(f"raw link to {destination_hex} closed before activation")
            time.sleep(0.1)

        raise RuntimeError(f"timed out establishing raw link to {destination_hex}")

    def send_raw(self, content: str) -> dict:
        with self.lock:
            link = self.raw_link
        if link is None or link.status != RNS.Link.ACTIVE:
            raise RuntimeError("no active raw link")

        receipt = RNS.Packet(link, RAW_LINK_TEST_PREFIX + content.encode("utf-8")).send()
        if receipt is False:
            raise RuntimeError("raw link packet was not dispatched")
        return {"sent": True, "content": content}

    def raw_link_status(self) -> dict:
        with self.lock:
            link = self.raw_link
        if link is None:
            return {"status_name": None}
        return {
            "status_name": LINK_STATUS_NAMES.get(link.status, str(link.status)),
            "link_id": link.link_id.hex(),
        }

    def wait_raw_message(self, content: str, timeout: float = 60.0) -> dict:
        expected = content.encode("utf-8").hex()
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                if expected in self.raw_messages:
                    return {"received": True, "content": content}
            time.sleep(0.1)

        raise RuntimeError(f"timed out waiting for raw link packet {content!r}")

    def send_raw_resource(self, size: int, metadata: str, timeout: float = 60.0) -> dict:
        with self.lock:
            link = self.raw_link
        if link is None or link.status != RNS.Link.ACTIVE:
            raise RuntimeError("no active raw link")
        data = bytes((index * 31 + 7) % 256 for index in range(size))
        completed = threading.Event()
        result = {}

        def on_concluded(resource) -> None:
            result["status"] = resource.status
            completed.set()

        resource = RNS.Resource(data, link, metadata=metadata, callback=on_concluded, timeout=timeout)
        if not completed.wait(timeout):
            raise RuntimeError(f"Resource did not complete; status={resource.status}")
        if result.get("status") != RNS.Resource.COMPLETE:
            raise RuntimeError(f"Resource caller completion was not successful: {result}")
        return {"completed": True, "size": size, "sha256": hashlib.sha256(data).hexdigest()}

    def wait_raw_resource(self, size: int, digest: str, metadata: str, timeout: float = 60.0) -> dict:
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                for resource in self.raw_resources:
                    if resource == {"size": size, "sha256": digest, "metadata": metadata}:
                        return {"received": True, **resource}
            time.sleep(0.05)
        raise RuntimeError(f"timed out waiting for Resource size={size} sha256={digest} metadata={metadata!r}")

    def link_status(self) -> dict:
        return self._link_snapshot()

    def wait_link_state(self, state: str, timeout: float = 60.0) -> dict:
        deadline = time.time() + timeout
        snapshot = self._link_snapshot()
        while time.time() < deadline:
            snapshot = self._link_snapshot()
            if snapshot.get("status_name") == state:
                return snapshot
            time.sleep(0.1)

        raise RuntimeError(f"timed out waiting for link state {state!r}; last snapshot: {snapshot}")

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
            elif method == "wait_message_prefix":
                result = self.server.state.wait_message_prefix(
                    params["prefix"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "send_message":
                result = self.server.state.send_message(
                    params["destination"],
                    params.get("title", ""),
                    params.get("content", ""),
                    bool(params.get("wait_for_path", True)),
                    params.get("method", "direct"),
                    int(params.get("content_bytes", 0)),
                )
            elif method == "arm_resource_gate":
                result = self.server.state.arm_resource_gate()
            elif method == "wait_resource_gate":
                result = self.server.state.wait_resource_gate(
                    float(params.get("timeout", 120.0)),
                )
            elif method == "release_resource_gate":
                result = self.server.state.release_resource_gate()
            elif method == "resource_gate_snapshot":
                result = self.server.state.resource_gate_snapshot()
            elif method == "set_router_resource_callback_chaining":
                result = self.server.state.set_router_resource_callback_chaining(
                    bool(params.get("enabled", False))
                )
            elif method == "arm_outbound_packet_drop":
                result = self.server.state.arm_outbound_packet_drop(params["destination"])
            elif method == "wait_outbound_packet_drop":
                result = self.server.state.wait_outbound_packet_drop(
                    float(params.get("timeout", 10.0)),
                )
            elif method == "mark_outbound_retry_window":
                result = self.server.state.mark_outbound_retry_window()
            elif method == "wait_outbound_retry_packet":
                result = self.server.state.wait_outbound_retry_packet(
                    float(params.get("timeout", 30.0)),
                )
            elif method == "outbound_status":
                result = self.server.state.outbound_status(params["message_hash"])
            elif method == "wait_outbound_state":
                result = self.server.state.wait_outbound_state(
                    params["message_hash"],
                    params["state"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "open_link":
                result = self.server.state.open_link(
                    params["destination"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "wait_path":
                result = self.server.state.wait_path(
                    params["destination"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "path_snapshot":
                result = self.server.state.path_snapshot(params["destination"])
            elif method == "request_path":
                result = self.server.state.request_path(params["destination"])
            elif method == "open_raw_link":
                result = self.server.state.open_raw_link(
                    params["destination"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "send_raw":
                result = self.server.state.send_raw(params["content"])
            elif method == "raw_link_status":
                result = self.server.state.raw_link_status()
            elif method == "wait_raw_message":
                result = self.server.state.wait_raw_message(
                    params["content"],
                    float(params.get("timeout", 60.0)),
                )
            elif method == "send_raw_resource":
                result = self.server.state.send_raw_resource(
                    int(params["size"]), params["metadata"], float(params.get("timeout", 60.0))
                )
            elif method == "wait_raw_resource":
                result = self.server.state.wait_raw_resource(
                    int(params["size"]), params["sha256"], params["metadata"],
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
            print(
                f"python_lxmf_endpoint: control error method={method!r}: {exc}",
                file=sys.stderr,
                flush=True,
            )
            response = {"ok": False, "error": str(exc)}

        self.wfile.write(json.dumps(response).encode("utf-8"))
        self.wfile.flush()


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

    print("python_lxmf_endpoint: starting control server", file=sys.stderr, flush=True)
    with ControlServer(("127.0.0.1", args.control_port), ControlHandler, state) as server:
        print("python_lxmf_endpoint: control server ready", file=sys.stderr, flush=True)
        server.serve_forever(poll_interval=0.1)


if __name__ == "__main__":
    main()
