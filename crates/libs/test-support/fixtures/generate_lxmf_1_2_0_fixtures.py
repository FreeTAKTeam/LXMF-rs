"""Regenerate LXMF 1.2.0 fixtures using explicitly pinned local Python references."""

import json
import os
from pathlib import Path
import subprocess
import struct
import sys


PINS = {
    "RETICULUM_PY_REPO": "99de23c040d507e3fefca19e87b182302902725d",
    "LXMF_PY_REPO": "c3ff2d6dc2f256daab896dadc044dd5a913ecbb7",
}
roots = {}
for variable, expected in PINS.items():
    root = Path(os.environ[variable]).resolve()
    actual = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    if actual != expected:
        raise SystemExit(f"{variable}: expected {expected}, got {actual}")
    dirty = subprocess.check_output(
        ["git", "-C", str(root), "status", "--porcelain", "--untracked-files=all"], text=True
    )
    if dirty:
        raise SystemExit(f"{variable}: reference checkout must be clean")
    roots[variable] = root
    sys.path.insert(0, str(root))

import RNS
import LXMF

for module, variable in [(RNS, "RETICULUM_PY_REPO"), (LXMF, "LXMF_PY_REPO")]:
    if not Path(module.__file__).resolve().is_relative_to(roots[variable]):
        raise SystemExit(f"{variable}: imported module is outside the verified checkout")


def destination(seed):
    identity = RNS.Identity.from_bytes(bytes([seed]) * 64)
    return RNS.Destination(identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")


source, recipient = destination(0x11), destination(0x22)
cases = []
profiles = [
    ("empty", b"", {}, None),
    ("title_fields", b"title", {1: b"field"}, None),
    ("stamped", b"title", {1: b"field"}, bytes([0x44]) * 32),
]
for name, title, fields, stamp in profiles:
    for wire_len in [398, 399, 400, 430, 431, 432]:
        for size in range(450):
            message = LXMF.LXMessage(
                recipient, source, content=b"x" * size, title=title, fields=fields,
                desired_method=LXMF.LXMessage.OPPORTUNISTIC,
            )
            message.timestamp = 1773999000.25
            message.defer_stamp = False
            message.get_stamp = lambda value=stamp: value
            message.pack()
            if len(message.packed) == wire_len:
                assert message.stamp == stamp
                cases.append({
                    "name": f"{name}_{wire_len}", "content_bytes": size,
                    "stamp_bytes": len(stamp) if stamp is not None else 0,
                    "wire_bytes": wire_len, "wire_hex": message.packed.hex(),
                    "method": message.method, "representation": message.representation,
                })
                break
        else:
            raise RuntimeError(f"No serialized boundary for {name}/{wire_len}")

output = {
    "lxmf_commit": PINS["LXMF_PY_REPO"], "rns_commit": PINS["RETICULUM_PY_REPO"],
    "note": "Fixed timestamp/identities. Injected stamp bytes test serialization, not proof-of-work.",
    "cases": cases,
}
Path(__file__).with_name("lxmf_1_2_0_sizes.json").write_text(json.dumps(output, indent=2) + "\n")

# Exercise the reference production encoder without starting its network workers.
from unittest.mock import patch

router = object.__new__(LXMF.LXMRouter)
router.name = "Peer Node"
router.propagation_node = True
router.from_static_only = False
router.propagation_stamp_cost = 21
router.propagation_stamp_cost_flexibility = 5
router.peering_cost = 13
router.propagation_per_transfer_limit = 256
router.propagation_per_sync_limit = 10240
with patch("time.time", return_value=1773999000):
    announcement = RNS.vendor.umsgpack.unpackb(router.get_propagation_node_app_data())

metadata_cases = []
for name, metadata in [
    ("current", announcement[6]),
    ("legacy_name_only", {1: b"Peer Node"}),
    ("legacy_empty", {}),
    ("unknown_field", {**announcement[6], 0x7f: [None, {"future": b"value"}]}),
    ("malformed_optional_values", {1: b"Peer Node", 0: [1], 0xfe: None}),
    ("malformed_name", {**announcement[6], 1: b"\xff"}),
    ("malformed_metadata_array", []),
    ("malformed_metadata_nil", None),
]:
    data = RNS.vendor.umsgpack.packb([*announcement[:6], metadata])
    metadata_cases.append({
        "name": name, "wire_hex": data.hex(),
        "valid": LXMF.pn_announce_data_is_valid(data),
        "display_name": LXMF.pn_name_from_app_data(data),
        "stamp_cost": LXMF.pn_stamp_cost_from_app_data(data),
    })
Path(__file__).with_name("lxmf_1_2_0_metadata.json").write_text(json.dumps({
    "lxmf_commit": PINS["LXMF_PY_REPO"], "rns_commit": PINS["RETICULUM_PY_REPO"],
    "note": "Current production encoder plus legacy and malformed variants, evaluated by target reference parsers.",
    "cases": metadata_cases,
}, indent=2) + "\n")

# Evaluate timing decisions through the pinned router, with only clock and
# transport measurements replaced. No worker or real path request is started.
from types import SimpleNamespace
import threading

timing_cases = []
for name, medium, bitrate, first_hop in [
    ("unknown", 0, None, None),
    ("normal_pathless", 8, 2000, None),
    ("slow_boundary", 8, 1999, None),
    ("slow_medium", 86, 100, None),
    ("normal_known", 8, 2000, 6),
    ("known_below_default", 8, 2000, 4),
    ("known_retry_boundary", 8, 2000, 8),
    ("known_above_retry_boundary", 8, 2000, 8.000001),
    ("known_four_attempts", 8, 2000, 9),
    ("known_three_attempts", 86, 100, 46),
    ("slow_unknown_bitrate", 86, None, None),
    ("path_even_rounding_four", 50 / 4.5, 1000, None),
    ("path_even_rounding_three", 50 / 3.5, 1000, None),
    ("known_even_rounding_four", 8, 2000, (50 / 4.5 - 6) / 2 + 6),
    ("known_even_rounding_three", 8, 2000, (50 / 3.5 - 6) / 2 + 6),
]:
    instance = SimpleNamespace(
        get_medium_path_timeout=lambda: medium,
        get_lowest_interface_bitrate=lambda: bitrate,
        get_first_hop_timeout=lambda destination: first_hop,
    )
    with patch.object(RNS.Reticulum, "get_instance", return_value=instance), \
         patch.object(RNS.Transport, "has_path", return_value=first_hop is not None) as has_path, \
         patch.object(RNS.Transport, "request_path") as request, \
         patch("time.time", return_value=1000) as clock:
        schedule_cases = []
        for path_request in [False, True]:
            message = SimpleNamespace(awaiting_path=False, max_delivery_attempts=None, delivery_attempts=2)
            router.schedule_attempt(message, b"peer", path_request=path_request)
            deadline = message.next_delivery_attempt
            checks = []
            for now, path_known in [(1000, False), (1000, True), (deadline, False), (deadline, True), (deadline + 0.001, False)]:
                clock.return_value = now
                has_path.return_value = path_known
                checks.append({"now": now, "path_known": path_known,
                               "due": router.attempt_due(message, b"peer")})
            schedule_cases.append({
                "path_request": path_request, "deadline": deadline,
                "max_attempts": router.attempt_limit(message),
                "attempts": message.delivery_attempts, "awaiting_path": message.awaiting_path,
                "checks": checks,
            })
            clock.return_value = 1000
            has_path.return_value = first_hop is not None
        router.path_request_lock = threading.Lock()
        router.path_request_times = {}
        debounce = 60 if router.slow_interface_online() else router.path_request_wait()
        request_checks = []
        for elapsed in sorted(set([0, router.path_request_wait(), debounce - 0.001, debounce])):
            # Reset the same previous request, so each edge is independent.
            router.path_request_times = {b"peer": 1000}
            clock.return_value = 1000 + elapsed
            pending = router.path_request_pending(b"peer")
            allowed = router.request_path(b"peer")
            request_checks.append({"elapsed": elapsed, "pending": pending, "allowed": allowed})
        timing_cases.append({
            "name": name, "medium_timeout": medium, "lowest_bitrate": bitrate,
            "first_hop_timeout": first_hop, "default_per_hop_timeout": RNS.Reticulum.DEFAULT_PER_HOP_TIMEOUT,
            "path_wait": router.path_request_wait(), "request_debounce": debounce,
            "destination_round_trip": router.destination_round_trip(b"peer"),
            "schedules": schedule_cases, "requests": request_checks,
        })
Path(__file__).with_name("lxmf_1_2_0_timing.json").write_text(json.dumps({
    "lxmf_commit": PINS["LXMF_PY_REPO"], "rns_commit": PINS["RETICULUM_PY_REPO"],
    "note": "Pinned production router timing methods with injected clock and transport measurements; no network delivery proof.",
    "cases": timing_cases,
}, indent=2) + "\n")

# Exercise actual RNS measurement arithmetic before LXMF scheduling. Only raw
# interface bitrates/path presence are injected, not calculated timeout values.
raw_cases = []
instance = object.__new__(RNS.Reticulum)
instance.is_connected_to_shared_instance = False
for name, lowest, known, next_bitrate in [
    ("no_interfaces", None, False, None),
    ("normal_pathless", 2000, False, None),
    ("slow_pathless", 100, False, None),
    ("minimum_bitrate", 1, False, None),
    ("slowest_differs_from_next_hop", 100, True, 100000),
    ("known_without_interface", 100, True, None),
    ("known_zero_bitrate", 100, True, 0),
    ("known_zero_bitrate_only", None, True, 0),
    ("known_four_attempts", 1500, True, 1500),
    ("known_rounding_below", 1565, True, 1565),
    ("known_rounding_above", 1566, True, 1566),
    ("known_three_attempts", 100, True, 100),
]:
    with patch.object(RNS.Reticulum, "get_instance", return_value=instance), \
         patch.object(RNS.Transport, "has_path", return_value=known), \
         patch.object(RNS.Transport, "lowest_interface_bitrate", lowest), \
         patch.object(RNS.Transport, "next_hop_interface_bitrate", return_value=next_bitrate), \
         patch("time.time", return_value=1000):
        limits = []
        for path_request in [False, True]:
            message = SimpleNamespace(awaiting_path=False, max_delivery_attempts=None)
            router.schedule_attempt(message, b"peer", path_request=path_request)
            limits.append(router.attempt_limit(message))
        raw_cases.append({
            "name": name, "lowest_bitrate": lowest, "path_known": known,
            "next_hop_bitrate": next_bitrate,
            "medium_timeout": router.medium_path_timeout(),
            "destination_round_trip": router.destination_round_trip(b"peer"),
            "path_wait": router.path_request_wait(),
            "delivery_attempt_limit": limits[0], "path_attempt_limit": limits[1],
        })
# JSON parsers may round decimal floats differently. Preserve the exact binary
# reference values too, so arithmetic tests do not test the JSON decoder.
for case in raw_cases:
    for field in ["medium_timeout", "destination_round_trip", "path_wait"]:
        case[field + "_bits"] = int.from_bytes(struct.pack(">d", float(case[field])), "big")
Path(__file__).with_name("lxmf_1_2_0_timing_measurements.json").write_text(json.dumps({
    "lxmf_commit": PINS["LXMF_PY_REPO"], "rns_commit": PINS["RETICULUM_PY_REPO"],
    "note": "Pinned RNS measurements and LXMF scheduling with raw bitrates/path presence injected; no network delivery proof.",
    "cases": raw_cases,
}, indent=2) + "\n")

from generate_lxmf_1_2_0_retry_traces import generate_retry_traces

Path(__file__).with_name("lxmf_1_2_0_retry_traces.json").write_text(json.dumps({
    "lxmf_commit": PINS["LXMF_PY_REPO"], "rns_commit": PINS["RETICULUM_PY_REPO"],
    "note": "Production handle_outbound/process_outbound/LXMessage.send with controlled clock, transport measurements, Packet.send effect and thread execution. No network proof. The reference delayed rediscovery request survives delivery/cancellation; Rust owner cancellation is intentionally stronger.",
    "cases": generate_retry_traces(RNS, LXMF, source, recipient),
}, indent=2) + "\n")

from generate_lxmf_1_2_0_link_traces import generate_link_traces

Path(__file__).with_name("lxmf_1_2_0_link_traces.json").write_text(json.dumps({
    "lxmf_commit": PINS["LXMF_PY_REPO"], "rns_commit": PINS["RETICULUM_PY_REPO"],
    "note": "Production router admission/retry and LXMessage packet/resource callbacks. Link/Resource transport effects, clock, measurements and thread scheduling are controlled. No real link, packet proof or Resource delivery is claimed. Propagation encryption is real but only stable lengths and within-run byte preservation are recorded. Python callbacks after cancellation may change state; Rust durable terminal monotonicity remains an intentional stronger boundary. Cancellation inputs assign message state rather than invoke router.cancel_outbound; no Resource cancellation or native RNS watchdog timing is proved. Shared-link cases exercise sequential production router processing, not concurrent Rust acquisition. Rust explicit fallback and local propagation custody have no matching router policy here.",
    "cases": generate_link_traces(RNS, LXMF, source, recipient),
}, indent=2) + "\n")
