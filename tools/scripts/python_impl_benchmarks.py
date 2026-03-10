#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import statistics
import tempfile
import time
from pathlib import Path

ANNOUNCE_BATCH_SIZE = 64


def percentile(values: list[int], p: float) -> float:
    if not values:
        raise ValueError("percentile requires at least one value")
    index = round((len(values) - 1) * p)
    return float(values[index])


def trimmed_tail_sample(values: list[int]) -> list[int]:
    if len(values) < 8:
        return values
    trim = max(len(values) // 20, 1)
    if len(values) <= trim * 2:
        return values
    return values[trim:-trim]


def run_benchmark(name: str, iterations: int, func):
    samples: list[int] = []
    for _ in range(iterations):
        start = time.perf_counter_ns()
        func()
        samples.append(time.perf_counter_ns() - start)

    samples.sort()
    tail_samples = trimmed_tail_sample(samples)
    p50 = percentile(samples, 0.50)
    p95 = percentile(tail_samples, 0.95)
    p99 = percentile(tail_samples, 0.99)
    mean = statistics.fmean(samples)
    throughput = 1_000_000_000.0 / max(p50, 1.0)

    return {
        "name": name,
        "iterations": iterations,
        "mean_ns": mean,
        "p50_ns": p50,
        "p95_ns": p95,
        "p99_ns": p99,
        "throughput_ops_per_sec": throughput,
    }


def build_lxmf_fixtures():
    import RNS
    from LXMF import LXMessage

    source_identity = RNS.Identity()
    destination_identity = RNS.Identity()
    source = RNS.Destination(
        source_identity, RNS.Destination.IN, RNS.Destination.SINGLE, "lxmf", "delivery"
    )
    destination = RNS.Destination(
        destination_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery"
    )

    def pack_message():
        message = LXMessage(
            destination,
            source,
            content="bench-content-payload",
            title="bench-title",
            desired_method=LXMessage.DIRECT,
        )
        message.pack()
        return message

    packed_message = pack_message().packed
    large_content = "x" * 2048

    def pack_large_message():
        message = LXMessage(
            destination,
            source,
            content=large_content,
            title="bench-large-title",
            desired_method=LXMessage.DIRECT,
        )
        message.pack()
        return message

    packed_large_message = pack_large_message().packed
    return pack_message, packed_message, pack_large_message, packed_large_message


def build_rns_fixtures():
    import RNS
    from RNS import Resource

    class BenchLink:
        ACTIVE = 0x02
        MDU = RNS.Packet.MDU

        def __init__(self):
            self.type = RNS.Destination.LINK
            self.hash = b"\x02" * (RNS.Identity.TRUNCATED_HASHLENGTH // 8)
            self.mtu = RNS.Reticulum.MTU
            self.rtt = 0.01
            self.establishment_cost = 1
            self.expected_rate = 1
            self.traffic_timeout_factor = 1
            self.status = self.ACTIVE
            self.link_id = b"\x01" * 16

        def ready_for_new_resource(self):
            return True

        def register_outgoing_resource(self, resource):
            return None

        def cancel_outgoing_resource(self, resource):
            return None

        def cancel_incoming_resource(self, resource):
            return None

        def resource_concluded(self, resource):
            return None

        def encrypt(self, data):
            return data

        def decrypt(self, data):
            return data

    identity = RNS.Identity()
    destination = RNS.Destination(
        identity,
        RNS.Destination.IN,
        RNS.Destination.SINGLE,
        "example_utilities",
        "announcesample",
        "fruits",
    )

    def announce_create():
        packet = destination.announce(app_data=b"rust-announce-app-data", send=False)
        packet.pack()
        return packet

    announce_packet = announce_create()
    announce_batch_packets = []
    for index in range(ANNOUNCE_BATCH_SIZE):
        batch_packet = destination.announce(
            app_data=f"rust-announce-app-data-{index}".encode("utf-8"), send=False
        )
        batch_packet.pack()
        announce_batch_packets.append(batch_packet)

    sign_message = b"x" * 2048
    signature = identity.sign(sign_message)

    def identity_sign():
        return identity.sign(sign_message)

    def identity_verify():
        return identity.validate(signature, sign_message)

    ciphertext = identity.encrypt(sign_message)

    def identity_encrypt():
        return identity.encrypt(sign_message)

    def identity_decrypt():
        return identity.decrypt(ciphertext)

    def announce_validate_batch():
        total = 0
        for packet in announce_batch_packets:
            total += 1 if RNS.Identity.validate_announce(packet) else 0
        return total

    resource_link = BenchLink()
    resource_payload = bytearray()
    for index in range(6):
        resource_payload.extend(bytes([index % 251]) * RNS.Packet.MDU)

    resource = Resource(
        bytes(resource_payload),
        resource_link,
        advertise=False,
        auto_compress=False,
    )

    def packet_send_stub(self):
        self.sent = True
        return None

    def packet_resend_stub(self):
        return None

    RNS.Packet.send = packet_send_stub
    if hasattr(RNS.Packet, "resend"):
        RNS.Packet.resend = packet_resend_stub

    requested_hashes = []
    for index in range(min(Resource.WINDOW, len(resource.parts))):
        requested_hashes.append(resource.parts[index].map_hash)
    request_data = bytes([Resource.HASHMAP_IS_NOT_EXHAUSTED]) + resource.hash + b"".join(requested_hashes)

    def resource_request_window():
        now = time.time()
        resource.adv_sent = now - resource_link.rtt
        resource.last_activity = now
        resource.last_part_sent = now
        resource.sent_parts = 0
        resource.receiver_min_consecutive_height = 0
        resource.status = Resource.TRANSFERRING
        resource.retries_left = resource.max_retries
        for part in resource.parts:
            part.sent = False
        resource.request(request_data)
        return resource.sent_parts

    return (
        announce_create,
        announce_packet,
        announce_validate_batch,
        resource_request_window,
        identity_sign,
        identity_verify,
        identity_encrypt,
        identity_decrypt,
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Benchmark canonical Python Reticulum and LXMF hot paths."
    )
    parser.add_argument(
        "--iterations",
        type=int,
        default=2000,
        help="Number of iterations per benchmark (default: 2000).",
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output JSON report path.",
    )
    args = parser.parse_args()

    try:
        import RNS  # noqa: F401
        from LXMF import LXMessage
    except ImportError as exc:
        raise SystemExit(f"python benchmark prerequisites missing: {exc}") from exc

    config_dir = tempfile.mkdtemp(prefix="lxmf-python-bench-")
    reticulum = RNS.Reticulum(configdir=config_dir, loglevel=0)
    try:
        import atexit

        atexit.unregister(RNS.Reticulum.exit_handler)
    except Exception:
        pass

    (
        pack_message,
        packed_message,
        pack_large_message,
        packed_large_message,
    ) = build_lxmf_fixtures()
    (
        announce_create,
        announce_packet,
        announce_validate_batch,
        resource_request_window,
        identity_sign,
        identity_verify,
        identity_encrypt,
        identity_decrypt,
    ) = build_rns_fixtures()

    results = [
        run_benchmark("python_lxmf/message_to_wire", args.iterations, pack_message),
        run_benchmark(
            "python_lxmf/message_from_wire",
            args.iterations,
            lambda: LXMessage.unpack_from_bytes(packed_message),
        ),
        run_benchmark("python_lxmf/large_message_to_wire", args.iterations, pack_large_message),
        run_benchmark(
            "python_lxmf/large_message_from_wire",
            args.iterations,
            lambda: LXMessage.unpack_from_bytes(packed_large_message),
        ),
        run_benchmark("python_rns/announce_create", args.iterations, announce_create),
        run_benchmark(
            "python_rns/announce_validate",
            args.iterations,
            lambda: RNS.Identity.validate_announce(announce_packet),
        ),
        run_benchmark(
            "python_rns/announce_validate_batch_64",
            args.iterations,
            announce_validate_batch,
        ),
        run_benchmark(
            "python_rns/resource_request_window",
            args.iterations,
            resource_request_window,
        ),
        run_benchmark("python_rns/identity_sign", args.iterations, identity_sign),
        run_benchmark("python_rns/identity_verify", args.iterations, identity_verify),
        run_benchmark("python_rns/identity_encrypt", args.iterations, identity_encrypt),
        run_benchmark("python_rns/identity_decrypt", args.iterations, identity_decrypt),
    ]
    payload = {
        "generated_at_epoch_s": time.time(),
        "iterations": args.iterations,
        "benchmarks": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"python benchmark report written to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
