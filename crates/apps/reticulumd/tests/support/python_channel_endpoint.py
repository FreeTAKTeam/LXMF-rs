#!/usr/bin/env python3

import argparse
import hashlib
import json
import os
import random
import tempfile
import sys
import threading
import time

import RNS
import RNS.Buffer
from RNS.Channel import MessageBase
from RNS.vendor import umsgpack


class FaultingReader:
    def __init__(self, data, fail_at):
        self.data = data
        self.offset = 0
        self.fail_at = fail_at
        backing_file = tempfile.NamedTemporaryFile(mode="wb", delete=False)
        backing_file.write(data)
        backing_file.close()
        self.name = backing_file.name

    def read(self, size=-1):
        if self.offset >= self.fail_at:
            raise OSError("synthetic Python file-reader failure")
        if size is None or size < 0:
            size = self.fail_at - self.offset
        end = min(self.fail_at, self.offset + size)
        chunk = self.data[self.offset:end]
        self.offset = end
        return chunk

    def seek(self, offset, whence=0):
        if whence == 0:
            self.offset = offset
        elif whence == 1:
            self.offset += offset
        elif whence == 2:
            self.offset = len(self.data) + offset
        else:
            raise ValueError(f"unsupported seek mode: {whence}")
        return self.offset

    def close(self):
        try:
            os.unlink(self.name)
        except FileNotFoundError:
            pass


def process_peak_rss_kib():
    try:
        import resource
    except ImportError:
        return None
    value = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if sys.platform == "darwin":
        value /= 1024
    return int(value)


class MessageTest(MessageBase):
    MSGTYPE = 0xABCD

    def __init__(self, message_id=None, data=None):
        self.id = message_id
        self.data = data

    def pack(self) -> bytes:
        return umsgpack.packb((self.id, self.data))

    def unpack(self, raw):
        self.id, self.data = umsgpack.unpackb(raw)


class ChannelEndpoint:
    def __init__(self, payload_kind: str):
        self.payload_kind = payload_kind
        self.lock = threading.Lock()
        self.links = []
        self.received = []
        self.buffers = []

    def start(self, config_dir: str) -> RNS.Destination:
        print("python_channel_endpoint: starting Reticulum", file=sys.stderr, flush=True)
        RNS.Reticulum(configdir=config_dir, loglevel=7)

        identity = RNS.Identity()
        destination = RNS.Destination(
            identity,
            RNS.Destination.IN,
            RNS.Destination.SINGLE,
            "test",
            "channel",
        )
        if self.payload_kind in ("request", "large-request", "file-response"):
            destination.register_request_handler(
                "/test/request",
                response_generator=self._on_request,
                allow=RNS.Destination.ALLOW_ALL,
            )
        destination.set_link_established_callback(self._on_link_established)
        print("python_channel_endpoint: ready", file=sys.stderr, flush=True)
        return destination

    def _on_request(self, path, data, request_id, link_id, remote_identity, requested_at):
        if self.payload_kind == "file-response":
            with tempfile.NamedTemporaryFile(delete=False) as temp_file:
                temp_file.write(b"python-file-response")
                temp_file.flush()
                file_path = temp_file.name
            file_handle = open(file_path, "rb")
            return (file_handle, "python-file-meta")
        return f"reply:{data}"

    def _on_link_established(self, link) -> None:
        print("python_channel_endpoint: link established", file=sys.stderr, flush=True)
        channel = link.get_channel()
        if self.payload_kind == "identify":
            channel.register_message_type(MessageTest)

            def on_remote_identified(_link, identity) -> None:
                with self.lock:
                    self.received.append({"identity": identity.hash.hex()})
                channel.send(MessageTest("rust-identify", f"identified:{identity.hash.hex()}"))

            link.set_remote_identified_callback(on_remote_identified)
            with self.lock:
                self.links.append(link)
            return

        if self.payload_kind == "link-data":
            def on_packet(message, _packet) -> None:
                text = message.decode("utf-8")
                with self.lock:
                    self.received.append({"data": text})
                RNS.Packet(link, f"reply:{text}".encode("utf-8")).send()

            link.set_packet_callback(on_packet)
            with self.lock:
                self.links.append(link)
            return

        if self.payload_kind in (
            "resource",
            "resource-compression",
            "resource-multi-hop",
            "cancel-resource",
            "resource-shutdown",
            "resource-reader-failure",
        ):
            link.set_resource_strategy(RNS.Link.ACCEPT_ALL)

            if self.payload_kind == "cancel-resource":
                def on_resource_started(resource) -> None:
                    print(
                        "python_channel_endpoint: cancelling incoming resource",
                        file=sys.stderr,
                        flush=True,
                    )
                    resource.cancel()

                link.set_resource_started_callback(on_resource_started)

            if self.payload_kind in ("resource-shutdown", "resource-reader-failure"):
                channel.register_message_type(MessageTest)

                def on_resource_started(_resource) -> None:
                    channel.send(MessageTest("resource-started", "ready"))

                link.set_resource_started_callback(on_resource_started)

            def on_resource_concluded(resource) -> None:
                # RNS 1.5.2 invokes this callback from the assembler, but a
                # late duplicate part can race with that callback and restore
                # the status to TRANSFERRING. The assembled part count is the
                # stable completion signal in that narrow reference race.
                assembled = resource.status == RNS.Resource.COMPLETE or (
                    resource.status == RNS.Resource.TRANSFERRING
                    and resource.received_count == resource.total_parts
                )
                if not assembled:
                    return
                data = resource.data.read()
                digest = hashlib.sha256(data).hexdigest()
                metadata = resource.metadata
                with self.lock:
                    self.received.append(
                        {
                            "data_size": len(data),
                            "sha256": digest,
                            "metadata": metadata,
                            "compressed": resource.compressed,
                        }
                    )
                if metadata is not None and len(data) < 1024 * 1024:
                    reply_data = f"resource:{data.decode('utf-8')}:{metadata}"
                elif metadata is not None:
                    metadata_wire_size = len(umsgpack.packb(metadata)) + 3
                    expected_total_size = len(data) + metadata_wire_size
                    if resource.total_size != expected_total_size:
                        raise AssertionError(
                            f"Resource size accounting mismatch: advertised={resource.total_size} "
                            f"data={len(data)} metadata_wire={metadata_wire_size}"
                        )
                    reply_data = (
                        f"resource-sha256-metadata:{len(data)}:{digest}:"
                        f"{resource.total_size}:{metadata}"
                    )
                else:
                    reply_data = f"resource-sha256:{len(data)}:{digest}"
                if self.payload_kind == "resource-compression":
                    reply_data += (
                        f":total_size={resource.total_size}"
                        f":compressed={str(resource.compressed).lower()}"
                    )
                link.get_channel().send(
                    MessageTest(
                        "rust-resource",
                        reply_data,
                    )
                )

            link.set_resource_concluded_callback(on_resource_concluded)
            with self.lock:
                self.links.append(link)
            return

        if self.payload_kind == "buffer":
            buffer_ref = {}

            def on_buffer_ready(ready_bytes: int) -> None:
                data = buffer_ref["buffer"].read(ready_bytes)
                if data is None:
                    return
                with self.lock:
                    self.received.append({"data": data.decode("utf-8")})
                reply = data + b" back at you"
                buffer_ref["buffer"].write(reply)
                buffer_ref["buffer"].flush()

            buffer_ref["buffer"] = RNS.Buffer.create_bidirectional_buffer(
                0,
                0,
                channel,
                on_buffer_ready,
            )
            with self.lock:
                self.links.append(link)
                self.buffers.append(buffer_ref["buffer"])
            return

        channel.register_message_type(MessageTest)
        channel.add_message_handler(self._on_message)
        with self.lock:
            self.links.append(link)

    def _on_message(self, message) -> bool:
        if not isinstance(message, MessageTest):
            return False
        with self.lock:
            self.received.append({"id": message.id, "data": message.data})
            links = list(self.links)
        print(
            f"python_channel_endpoint: received channel message {message.id} {message.data}",
            file=sys.stderr,
            flush=True,
        )
        if links:
            reply = MessageTest(message.id, f"reply:{message.data}")
            try:
                links[-1].get_channel().send(reply)
                print(
                    f"python_channel_endpoint: sent channel reply {reply.id} {reply.data}",
                    file=sys.stderr,
                    flush=True,
                )
            except Exception as exc:
                print(
                    f"python_channel_endpoint: failed to send channel reply: {exc}",
                    file=sys.stderr,
                    flush=True,
                )
                raise
        return True


class ChannelClient:
    def __init__(self, payload_kind: str):
        self.payload_kind = payload_kind
        self.lock = threading.Lock()
        self.link = None
        self.received = []
        self.buffer = None

    def run(
        self,
        config_dir: str,
        destination_hash_hex: str,
        message_id: str,
        message_data: str,
        resource_size,
        send_delay: float,
        timeout: float,
    ) -> int:
        print("python_channel_client: starting Reticulum", file=sys.stderr, flush=True)
        RNS.Reticulum(configdir=config_dir, loglevel=7)
        destination_hash = bytes.fromhex(destination_hash_hex)
        deadline = time.time() + timeout

        if not RNS.Transport.has_path(destination_hash):
            RNS.Transport.request_path(destination_hash)
        while not RNS.Transport.has_path(destination_hash):
            if time.time() > deadline:
                print("python_channel_client: timed out waiting for path", file=sys.stderr, flush=True)
                return 1
            time.sleep(0.1)

        identity = RNS.Identity.recall(destination_hash)
        if identity is None:
            print("python_channel_client: destination identity not recalled", file=sys.stderr, flush=True)
            return 1

        destination = RNS.Destination(
            identity,
            RNS.Destination.OUT,
            RNS.Destination.SINGLE,
            "test",
            "channel",
        )
        link = RNS.Link(destination)
        link.set_link_established_callback(self._on_link_established)
        link.set_link_closed_callback(self._on_link_closed)

        while True:
            with self.lock:
                active_link = self.link
            if active_link is not None:
                break
            if time.time() > deadline:
                print("python_channel_client: timed out waiting for link", file=sys.stderr, flush=True)
                return 1
            time.sleep(0.05)

        time.sleep(send_delay)
        if self.payload_kind == "identify":
            self.identity = RNS.Identity()
            active_link.set_packet_callback(self._on_link_data)
            active_link.identify(self.identity)
            while True:
                with self.lock:
                    replies = list(self.received)
                for reply in replies:
                    if reply["data"] == "reply:identified":
                        print(json.dumps({"identified": self.identity.hash.hex()}), flush=True)
                        return 0
                if time.time() > deadline:
                    print("python_channel_client: timed out waiting for identify acknowledgement", file=sys.stderr, flush=True)
                    return 1
                time.sleep(0.05)

        if self.payload_kind in ("request", "large-request"):
            done = threading.Event()
            result = {}
            request_data = message_data
            if self.payload_kind == "large-request":
                # Keep this request resource-backed after negotiated-MTU
                # support: a fixed 900-byte payload is a normal packet on
                # TCP/Backbone links whose MDU is several kilobytes.
                request_data = "large:" + ("x" * (active_link.mdu + 1024))
            print(
                f"python_channel_client: sending {self.payload_kind} request len={len(request_data)} mdu={active_link.mdu}",
                file=sys.stderr,
                flush=True,
            )

            def on_response(receipt) -> None:
                result["response"] = receipt.response
                done.set()

            def on_failed(receipt) -> None:
                result["failed"] = True
                done.set()

            receipt = active_link.request(
                "/test/request",
                data=request_data,
                response_callback=on_response,
                failed_callback=on_failed,
                timeout=timeout,
            )
            if receipt is False:
                print("python_channel_client: failed to send request", file=sys.stderr, flush=True)
                return 1
            while not done.is_set():
                if time.time() > deadline:
                    print("python_channel_client: timed out waiting for request response", file=sys.stderr, flush=True)
                    return 1
                time.sleep(0.05)
            expected_response = f"reply:{request_data}"
            if result.get("response") == expected_response:
                response_bytes = expected_response.encode("utf-8")
                print(
                    json.dumps(
                        {
                            "response": result["response"],
                            "response_size": len(response_bytes),
                            "response_sha256": hashlib.sha256(response_bytes).hexdigest(),
                        }
                    ),
                    flush=True,
                )
                return 0
            print(f"python_channel_client: request failed: {result}", file=sys.stderr, flush=True)
            return 1

        if self.payload_kind == "link-data":
            active_link.set_packet_callback(self._on_link_data)
            RNS.Packet(active_link, message_data.encode("utf-8")).send()
        elif self.payload_kind in (
            "resource",
            "resource-compression-compressible",
            "resource-compression-threshold",
            "resource-compression-incompressible",
            "resource-compression-disabled",
            "resource-multi-hop",
            "cancel-resource",
            "resource-file-reader-failure",
        ):
            done = threading.Event()
            result = {}

            if resource_size is None:
                resource_data = message_data.encode("utf-8")
                resource_file = None
            elif resource_size == 0:
                resource_data = b""
                resource_file = None
            elif self.payload_kind in (
                "resource-compression-compressible",
                "resource-compression-threshold",
                "resource-compression-disabled",
            ):
                resource_data = (b"pinned Python Resource compression fixture " * (resource_size // 43 + 1))[:resource_size]
                resource_file = None
            else:
                resource_data = random.Random(605).randbytes(resource_size)
                if self.payload_kind == "resource-file-reader-failure":
                    resource_file = FaultingReader(resource_data, max(1, resource_size // 2))
                else:
                    resource_file = tempfile.TemporaryFile(mode="w+b")
                    resource_file.write(resource_data)
                    resource_file.flush()
                    resource_file.seek(0)

            resource_metadata = (
                None
                if self.payload_kind == "resource-compression-threshold"
                else "python-meta"
            )

            def resource_concluded(resource) -> None:
                result["status"] = resource.status
                result["total_size"] = resource.get_data_size()
                result["segments"] = resource.get_segments()
                result["metadata"] = resource_metadata
                done.set()

            resource = RNS.Resource(
                resource_file if resource_file is not None else resource_data,
                active_link,
                metadata=resource_metadata,
                auto_compress=self.payload_kind != "resource-compression-disabled",
                callback=resource_concluded,
                timeout=timeout,
            )
            if self.payload_kind == "cancel-resource":
                # Wait for the Rust receiver to request the advertised
                # resource, then cancel while the reference transfer is
                # still active. This avoids a scheduling-dependent sleep.
                while resource.status < RNS.Resource.ADVERTISED:
                    if time.time() > deadline:
                        print(
                            "python_channel_client: timed out waiting for resource request",
                            file=sys.stderr,
                            flush=True,
                        )
                        return 1
                    time.sleep(0.01)
                while resource.status == RNS.Resource.ADVERTISED:
                    if time.time() > deadline:
                        print(
                            "python_channel_client: timed out waiting for resource transfer",
                            file=sys.stderr,
                            flush=True,
                        )
                        return 1
                    time.sleep(0.01)
                if resource.status < RNS.Resource.COMPLETE:
                    resource.cancel()
            while not done.is_set():
                if time.time() > deadline:
                    print("python_channel_client: timed out waiting for resource", file=sys.stderr, flush=True)
                    return 1
                time.sleep(0.05)
            if self.payload_kind == "cancel-resource":
                if result.get("status") == RNS.Resource.FAILED:
                    print(json.dumps({"resource": "cancelled"}), flush=True)
                    # Let the asynchronous cancel packet reach the peer
                    # before this short-lived reference client exits.
                    time.sleep(0.5)
                    return 0
                print(f"python_channel_client: resource cancellation failed: {result}", file=sys.stderr, flush=True)
                return 1
            if result.get("status") == RNS.Resource.COMPLETE:
                if self.payload_kind == "resource-multi-hop":
                    expected = f"resource-sha256:{len(resource_data)}:{hashlib.sha256(resource_data).hexdigest()}"
                    while True:
                        with self.lock:
                            acknowledged = any(
                                reply.get("id") == "rust-resource" and reply.get("data") == expected
                                for reply in self.received
                            )
                        if acknowledged:
                            break
                        if time.time() > deadline:
                            print(
                                "python_channel_client: timed out waiting for endpoint Resource callback",
                                file=sys.stderr,
                                flush=True,
                            )
                            return 1
                        time.sleep(0.05)
                print(
                    json.dumps(
                        {
                            "resource": "complete",
                            "size": len(resource_data),
                            "sha256": hashlib.sha256(resource_data).hexdigest(),
                            "compressed": resource.compressed,
                            "total_size": result.get("total_size"),
                            "segments": result.get("segments"),
                            "metadata": result.get("metadata"),
                            "peak_rss_kib": process_peak_rss_kib(),
                        }
                    ),
                    flush=True,
                )
                return 0
            print(f"python_channel_client: resource failed: {result}", file=sys.stderr, flush=True)
            return 1

        if self.payload_kind == "buffer":
            self.buffer.write(message_data.encode("utf-8"))
            self.buffer.flush()
        elif self.payload_kind == "channel-sequence":
            time.sleep(1.0)
            channel = active_link.get_channel()
            for index in range(3):
                channel.send(MessageTest(f"python-seq-{index}", f"hello-rust-{index}"))
                time.sleep(0.25)
        elif self.payload_kind in ("channel", "channel-reconnect"):
            active_link.get_channel().send(MessageTest(message_id, message_data))
        reconnect_started = False
        while True:
            with self.lock:
                replies = list(self.received)
            for reply in replies:
                if self.payload_kind == "link-data" and reply["data"] == f"reply:{message_data}":
                    print(json.dumps({"received": reply}), flush=True)
                    return 0
                if self.payload_kind == "buffer" and reply["data"] == f"{message_data} back at you":
                    print(json.dumps({"received": reply}), flush=True)
                    return 0
                if self.payload_kind == "channel-reconnect":
                    if (
                        not reconnect_started
                        and reply["id"] == message_id
                        and reply["data"] == f"reply:{message_data}"
                    ):
                        reconnect_started = True
                        with self.lock:
                            self.link = None
                        active_link.teardown()
                        reconnect_link = RNS.Link(destination)
                        reconnect_link.set_link_established_callback(self._on_link_established)
                        reconnect_link.set_link_closed_callback(self._on_link_closed)
                        reconnect_deadline = time.time() + timeout
                        while True:
                            with self.lock:
                                reconnected_link = self.link
                            if reconnected_link is not None:
                                active_link = reconnected_link
                                break
                            if time.time() > reconnect_deadline:
                                print(
                                    "python_channel_client: timed out waiting for reconnected link",
                                    file=sys.stderr,
                                    flush=True,
                                )
                                return 1
                            time.sleep(0.05)
                        active_link.get_channel().send(MessageTest("python-reconnect", "hello-reconnect"))
                        break
                    if reply["id"] == "python-reconnect" and reply["data"] == "reply:hello-reconnect":
                        print(json.dumps({"received": reply}), flush=True)
                        return 0
                if (
                    self.payload_kind == "channel"
                    and reply["id"] == message_id
                    and reply["data"] == f"reply:{message_data}"
                ):
                    print(json.dumps({"received": reply}), flush=True)
                    return 0
                if (
                    self.payload_kind == "channel-sequence"
                    and reply["id"] == "sequence-ack"
                    and reply["data"] == "reply:sequence-ok"
                ):
                    print(json.dumps({"received": reply}), flush=True)
                    return 0
            if time.time() > deadline:
                print("python_channel_client: timed out waiting for reply", file=sys.stderr, flush=True)
                return 1
            time.sleep(0.05)

    def _on_link_established(self, link) -> None:
        print("python_channel_client: link established", file=sys.stderr, flush=True)
        channel = link.get_channel()
        if self.payload_kind == "buffer":
            buffer_ref = {}

            def on_buffer_ready(ready_bytes: int) -> None:
                data = buffer_ref["buffer"].read(ready_bytes)
                if data is None:
                    return
                with self.lock:
                    self.received.append({"data": data.decode("utf-8")})

            buffer_ref["buffer"] = RNS.Buffer.create_bidirectional_buffer(
                0,
                0,
                channel,
                on_buffer_ready,
            )
            with self.lock:
                self.link = link
                self.buffer = buffer_ref["buffer"]
            return

        if self.payload_kind == "identify":
            with self.lock:
                self.link = link
            return

        channel.register_message_type(MessageTest)
        channel.add_message_handler(self._on_message)
        with self.lock:
            self.link = link

    def _on_link_closed(self, _link) -> None:
        print("python_channel_client: link closed", file=sys.stderr, flush=True)

    def _on_link_data(self, message, _packet) -> None:
        with self.lock:
            self.received.append({"data": message.decode("utf-8")})

    def _on_message(self, message) -> bool:
        if not isinstance(message, MessageTest):
            return False
        with self.lock:
            self.received.append({"id": message.id, "data": message.data})
        print(
            f"python_channel_client: received channel message {message.id} {message.data}",
            file=sys.stderr,
            flush=True,
        )
        return True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("server", "client"), default="server")
    parser.add_argument(
        "--payload-kind",
        choices=(
            "channel",
            "channel-reconnect",
            "buffer",
            "resource",
            "resource-compression",
            "resource-compression-compressible",
            "resource-compression-threshold",
            "resource-compression-incompressible",
            "resource-compression-disabled",
            "resource-multi-hop",
            "cancel-resource",
            "resource-shutdown",
            "resource-reader-failure",
            "resource-file-reader-failure",
            "link-data",
            "request",
            "large-request",
            "file-response",
            "identify",
            "channel-sequence",
        ),
        default="channel",
    )
    parser.add_argument("--config-dir", required=True)
    parser.add_argument("--announce-interval", type=float, default=0.25)
    parser.add_argument("--destination-hash")
    parser.add_argument("--message-id", default="python-1")
    parser.add_argument("--message-data", default="hello-rust")
    parser.add_argument("--resource-size", type=int)
    parser.add_argument("--send-delay", type=float, default=0.3)
    parser.add_argument("--timeout", type=float, default=8.0)
    args = parser.parse_args()

    if args.mode == "client":
        if args.destination_hash is None:
            parser.error("--destination-hash is required in client mode")
        return ChannelClient(args.payload_kind).run(
            args.config_dir,
            args.destination_hash,
            args.message_id,
            args.message_data,
            args.resource_size,
            args.send_delay,
            args.timeout,
        )

    endpoint = ChannelEndpoint(args.payload_kind)
    destination = endpoint.start(args.config_dir)
    print(json.dumps({"ready": True, "destination_hash": destination.hash.hex()}), flush=True)

    while True:
        destination.announce()
        time.sleep(args.announce_interval)


if __name__ == "__main__":
    raise SystemExit(main())
