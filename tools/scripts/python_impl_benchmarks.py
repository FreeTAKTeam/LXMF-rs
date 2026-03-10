#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import statistics
import tempfile
import time
from pathlib import Path


def percentile(values: list[int], p: float) -> float:
    if not values:
        raise ValueError("percentile requires at least one value")
    index = round((len(values) - 1) * p)
    return float(values[index])


def run_benchmark(name: str, iterations: int, func):
    samples: list[int] = []
    for _ in range(iterations):
        start = time.perf_counter_ns()
        func()
        samples.append(time.perf_counter_ns() - start)

    samples.sort()
    p50 = percentile(samples, 0.50)
    p95 = percentile(samples, 0.95)
    p99 = percentile(samples, 0.99)
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

    return (
        announce_create,
        announce_packet,
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
            run_benchmark("python_rns/identity_sign", args.iterations, identity_sign),
            run_benchmark("python_rns/identity_verify", args.iterations, identity_verify),
            run_benchmark("python_rns/identity_encrypt", args.iterations, identity_encrypt),
            run_benchmark("python_rns/identity_decrypt", args.iterations, identity_decrypt),
        ]
    finally:
        try:
            reticulum.exit_handler()
        except Exception:
            pass

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
