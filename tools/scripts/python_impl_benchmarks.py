#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

ANNOUNCE_BATCH_SIZE = 64
FIXTURES = json.loads(
    (Path(__file__).resolve().parents[2] / "tools/benchmarks/fixtures.json").read_text(
        encoding="utf-8"
    )
)


def fixture_private_key(name: str) -> bytes:
    return bytes.fromhex(FIXTURES["identities"][name])


def fixture_repeated_payload(length_key: str) -> bytes:
    payloads = FIXTURES["payloads"]
    return bytes.fromhex(payloads["resource_pattern_hex"]) * int(payloads[length_key])


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
    print(f"benchmarking {name} ({iterations} iterations)", flush=True)
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

    result = {
        "name": name,
        "iterations": iterations,
        "mean_ns": mean,
        "p50_ns": p50,
        "p95_ns": p95,
        "p99_ns": p99,
        "throughput_ops_per_sec": throughput,
    }
    print(f"completed {name}", flush=True)
    return result


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
            content=FIXTURES["payloads"]["message_content"],
            title=FIXTURES["payloads"]["message_title"],
            desired_method=LXMessage.DIRECT,
        )
        message.pack()
        return message

    packed_message = pack_message().packed
    large_content = "x" * int(FIXTURES["payloads"]["large_content_length"])

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
    resource_content = "x" * int(FIXTURES["payloads"]["resource_content_length"])

    def pack_resource_message():
        message = LXMessage(
            destination,
            source,
            content=resource_content,
            title="bench-resource-title",
            desired_method=LXMessage.DIRECT,
        )
        message.pack()
        return message

    packed_resource_message = pack_resource_message().packed
    return (
        pack_message,
        packed_message,
        pack_large_message,
        packed_large_message,
        pack_resource_message,
        packed_resource_message,
    )


def build_rns_fixtures():
    import RNS
    from RNS import Resource

    print("building Reticulum announce fixtures", flush=True)

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

    print("building Reticulum identity crypto fixtures", flush=True)
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

    print("building Reticulum resource fixtures", flush=True)
    resource_link = BenchLink()
    resource_byte = bytes.fromhex(FIXTURES["payloads"]["resource_pattern_hex"])
    if len(resource_byte) != 1:
        raise ValueError("resource_pattern_hex must encode exactly one byte")
    # Reticulum remaps resources with duplicate part hashes. Give each part a
    # distinct prefix so this deterministic fixture cannot remap forever.
    resource_payload = b"".join(
        bytes([part_index]) + resource_byte * (RNS.Packet.MDU - 1)
        for part_index in range(6)
    )

    resource = Resource(
        bytes(resource_payload),
        resource_link,
        advertise=False,
        auto_compress=False,
    )
    print("Reticulum resource fixtures ready", flush=True)

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

    packet_destination = RNS.Destination(
        None, RNS.Destination.OUT, RNS.Destination.PLAIN, "lxmf", "packet-benchmark"
    )

    def packet_pack():
        packet = RNS.Packet(
            packet_destination,
            fixture_repeated_payload("large_content_length")[:128],
            create_receipt=False,
        )
        packet.pack()
        return packet.raw

    packed_packet = packet_pack()

    def packet_unpack():
        packet = RNS.Packet(None, packed_packet)
        packet.unpack()
        return packet

    segmentation_payload = fixture_repeated_payload("resource_content_length")
    segment_size = RNS.Packet.ENCRYPTED_MDU
    segmentation_parts = tuple(
        segmentation_payload[index : index + segment_size]
        for index in range(0, len(segmentation_payload), segment_size)
    )

    def resource_segment():
        return tuple(
            segmentation_payload[index : index + segment_size]
            for index in range(0, len(segmentation_payload), segment_size)
        )

    def resource_reassemble():
        return b"".join(segmentation_parts)

    return (
        announce_create,
        announce_packet,
        announce_validate_batch,
        resource_request_window,
        identity_sign,
        identity_verify,
        identity_encrypt,
        identity_decrypt,
        packet_pack,
        packet_unpack,
        resource_segment,
        resource_reassemble,
    )


def verify_module_revision(name: str, module_path: Path, expected: str | None) -> None:
    if expected is None:
        return
    try:
        actual = subprocess.run(
            ["git", "-C", str(module_path.parent), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f"cannot verify pinned {name} revision for {module_path}: {exc}") from exc
    if actual != expected:
        raise SystemExit(f"pinned {name} revision mismatch: expected {expected}, imported {actual}")


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
    parser.add_argument("--expected-rns-ref", help="Required git revision for the imported RNS")
    parser.add_argument("--expected-lxmf-ref", help="Required git revision for the imported LXMF")
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output JSON report path.",
    )
    parser.add_argument(
        "--benchmark",
        help="Optional single benchmark name to run.",
    )
    args = parser.parse_args()

    try:
        import RNS
        import LXMF
        from LXMF import LXMessage
    except ImportError as exc:
        raise SystemExit(f"python benchmark prerequisites missing: {exc}") from exc

    verify_module_revision("RNS", Path(RNS.__file__).resolve(), args.expected_rns_ref)
    verify_module_revision("LXMF", Path(LXMF.__file__).resolve(), args.expected_lxmf_ref)

    config_dir = tempfile.mkdtemp(prefix="lxmf-python-bench-")
    reticulum = RNS.Reticulum(configdir=config_dir, loglevel=0)
    try:
        import atexit

        atexit.unregister(RNS.Reticulum.exit_handler)
    except Exception:
        pass

    if args.benchmark:
        if args.benchmark.startswith("python_lxmf/"):
            (
                pack_message,
                packed_message,
                pack_large_message,
                packed_large_message,
                pack_resource_message,
                packed_resource_message,
            ) = build_lxmf_fixtures()
            benchmark_factories = {
                "python_lxmf/message_to_wire": lambda: run_benchmark(
                    "python_lxmf/message_to_wire", args.iterations, pack_message
                ),
                "python_lxmf/message_from_wire": lambda: run_benchmark(
                    "python_lxmf/message_from_wire",
                    args.iterations,
                    lambda: LXMessage.unpack_from_bytes(packed_message),
                ),
                "python_lxmf/large_message_to_wire": lambda: run_benchmark(
                    "python_lxmf/large_message_to_wire", args.iterations, pack_large_message
                ),
                "python_lxmf/large_message_from_wire": lambda: run_benchmark(
                    "python_lxmf/large_message_from_wire",
                    args.iterations,
                    lambda: LXMessage.unpack_from_bytes(packed_large_message),
                ),
                "python_lxmf/resource_message_to_wire": lambda: run_benchmark(
                    "python_lxmf/resource_message_to_wire",
                    args.iterations,
                    pack_resource_message,
                ),
                "python_lxmf/resource_message_from_wire": lambda: run_benchmark(
                    "python_lxmf/resource_message_from_wire",
                    args.iterations,
                    lambda: LXMessage.unpack_from_bytes(packed_resource_message),
                ),
            }
        elif args.benchmark.startswith("python_rns/"):
            (
                announce_create,
                announce_packet,
                announce_validate_batch,
                resource_request_window,
                identity_sign,
                identity_verify,
                identity_encrypt,
                identity_decrypt,
                packet_pack,
                packet_unpack,
                resource_segment,
                resource_reassemble,
            ) = build_rns_fixtures()
            benchmark_factories = {
                "python_rns/announce_create": lambda: run_benchmark(
                    "python_rns/announce_create", args.iterations, announce_create
                ),
                "python_rns/announce_validate": lambda: run_benchmark(
                    "python_rns/announce_validate",
                    args.iterations,
                    lambda: RNS.Identity.validate_announce(announce_packet),
                ),
                "python_rns/announce_validate_batch_64": lambda: run_benchmark(
                    "python_rns/announce_validate_batch_64",
                    args.iterations,
                    announce_validate_batch,
                ),
                "python_rns/resource_request_window": lambda: run_benchmark(
                    "python_rns/resource_request_window",
                    args.iterations,
                    resource_request_window,
                ),
                "python_rns/identity_sign": lambda: run_benchmark(
                    "python_rns/identity_sign", args.iterations, identity_sign
                ),
                "python_rns/identity_verify": lambda: run_benchmark(
                    "python_rns/identity_verify", args.iterations, identity_verify
                ),
                "python_rns/identity_encrypt": lambda: run_benchmark(
                    "python_rns/identity_encrypt", args.iterations, identity_encrypt
                ),
                "python_rns/identity_decrypt": lambda: run_benchmark(
                    "python_rns/identity_decrypt", args.iterations, identity_decrypt
                ),
                "python_rns/packet_pack": lambda: run_benchmark(
                    "python_rns/packet_pack", args.iterations, packet_pack
                ),
                "python_rns/packet_unpack": lambda: run_benchmark(
                    "python_rns/packet_unpack", args.iterations, packet_unpack
                ),
                "python_rns/resource_segment_16k": lambda: run_benchmark(
                    "python_rns/resource_segment_16k", args.iterations, resource_segment
                ),
                "python_rns/resource_reassemble_16k": lambda: run_benchmark(
                    "python_rns/resource_reassemble_16k", args.iterations, resource_reassemble
                ),
            }
        else:
            raise SystemExit(f"unsupported benchmark: {args.benchmark}")
        try:
            results = [benchmark_factories[args.benchmark]()]
        except KeyError as exc:
            raise SystemExit(f"unsupported benchmark: {args.benchmark}") from exc
    else:
        print("building LXMF benchmark fixtures", flush=True)
        (
            pack_message,
            packed_message,
            pack_large_message,
            packed_large_message,
            pack_resource_message,
            packed_resource_message,
        ) = build_lxmf_fixtures()
        print("building Reticulum benchmark fixtures", flush=True)
        (
            announce_create,
            announce_packet,
            announce_validate_batch,
            resource_request_window,
            identity_sign,
            identity_verify,
            identity_encrypt,
            identity_decrypt,
            packet_pack,
            packet_unpack,
            resource_segment,
            resource_reassemble,
        ) = build_rns_fixtures()
        print("benchmark fixtures ready", flush=True)
        benchmark_factories = {
            "python_lxmf/message_to_wire": lambda: run_benchmark(
                "python_lxmf/message_to_wire", args.iterations, pack_message
            ),
            "python_lxmf/message_from_wire": lambda: run_benchmark(
                "python_lxmf/message_from_wire",
                args.iterations,
                lambda: LXMessage.unpack_from_bytes(packed_message),
            ),
            "python_lxmf/large_message_to_wire": lambda: run_benchmark(
                "python_lxmf/large_message_to_wire", args.iterations, pack_large_message
            ),
            "python_lxmf/large_message_from_wire": lambda: run_benchmark(
                "python_lxmf/large_message_from_wire",
                args.iterations,
                lambda: LXMessage.unpack_from_bytes(packed_large_message),
            ),
            "python_lxmf/resource_message_to_wire": lambda: run_benchmark(
                "python_lxmf/resource_message_to_wire", args.iterations, pack_resource_message
            ),
            "python_lxmf/resource_message_from_wire": lambda: run_benchmark(
                "python_lxmf/resource_message_from_wire",
                args.iterations,
                lambda: LXMessage.unpack_from_bytes(packed_resource_message),
            ),
            "python_rns/announce_create": lambda: run_benchmark(
                "python_rns/announce_create", args.iterations, announce_create
            ),
            "python_rns/announce_validate": lambda: run_benchmark(
                "python_rns/announce_validate",
                args.iterations,
                lambda: RNS.Identity.validate_announce(announce_packet),
            ),
            "python_rns/announce_validate_batch_64": lambda: run_benchmark(
                "python_rns/announce_validate_batch_64",
                args.iterations,
                announce_validate_batch,
            ),
            "python_rns/resource_request_window": lambda: run_benchmark(
                "python_rns/resource_request_window",
                args.iterations,
                resource_request_window,
            ),
            "python_rns/identity_sign": lambda: run_benchmark(
                "python_rns/identity_sign", args.iterations, identity_sign
            ),
            "python_rns/identity_verify": lambda: run_benchmark(
                "python_rns/identity_verify", args.iterations, identity_verify
            ),
            "python_rns/identity_encrypt": lambda: run_benchmark(
                "python_rns/identity_encrypt", args.iterations, identity_encrypt
            ),
            "python_rns/identity_decrypt": lambda: run_benchmark(
                "python_rns/identity_decrypt", args.iterations, identity_decrypt
            ),
            "python_rns/packet_pack": lambda: run_benchmark(
                "python_rns/packet_pack", args.iterations, packet_pack
            ),
            "python_rns/packet_unpack": lambda: run_benchmark(
                "python_rns/packet_unpack", args.iterations, packet_unpack
            ),
            "python_rns/resource_segment_16k": lambda: run_benchmark(
                "python_rns/resource_segment_16k", args.iterations, resource_segment
            ),
            "python_rns/resource_reassemble_16k": lambda: run_benchmark(
                "python_rns/resource_reassemble_16k", args.iterations, resource_reassemble
            ),
        }
        results = [factory() for factory in benchmark_factories.values()]
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
