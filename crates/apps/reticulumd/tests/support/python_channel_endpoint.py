#!/usr/bin/env python3

import argparse
import json
import tempfile
import sys
import threading
import time

import RNS
import RNS.Buffer
from RNS.Channel import MessageBase
from RNS.vendor import umsgpack


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

        if self.payload_kind == "resource":
            link.set_resource_strategy(RNS.Link.ACCEPT_ALL)

            def on_resource_concluded(resource) -> None:
                if resource.status != RNS.Resource.COMPLETE:
                    return
                data = resource.data.read()
                with self.lock:
                    self.received.append(
                        {
                            "data": data.decode("utf-8"),
                            "metadata": resource.metadata,
                        }
                    )
                link.get_channel().send(
                    MessageTest(
                        "rust-resource",
                        f"resource:{data.decode('utf-8')}:{resource.metadata}",
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
                request_data = "large:" + ("x" * 900)
            print(
                f"python_channel_client: sending {self.payload_kind} request len={len(request_data)}",
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
                print(json.dumps({"response": result["response"]}), flush=True)
                return 0
            print(f"python_channel_client: request failed: {result}", file=sys.stderr, flush=True)
            return 1

        if self.payload_kind == "link-data":
            active_link.set_packet_callback(self._on_link_data)
            RNS.Packet(active_link, message_data.encode("utf-8")).send()
        elif self.payload_kind == "resource":
            done = threading.Event()
            result = {}

            def resource_concluded(resource) -> None:
                result["status"] = resource.status
                done.set()

            RNS.Resource(
                message_data.encode("utf-8"),
                active_link,
                metadata="python-meta",
                callback=resource_concluded,
                timeout=timeout,
            )
            while not done.is_set():
                if time.time() > deadline:
                    print("python_channel_client: timed out waiting for resource", file=sys.stderr, flush=True)
                    return 1
                time.sleep(0.05)
            if result.get("status") == RNS.Resource.COMPLETE:
                print(json.dumps({"resource": "complete"}), flush=True)
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
        elif self.payload_kind == "channel":
            active_link.get_channel().send(MessageTest(message_id, message_data))
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
            "buffer",
            "resource",
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
