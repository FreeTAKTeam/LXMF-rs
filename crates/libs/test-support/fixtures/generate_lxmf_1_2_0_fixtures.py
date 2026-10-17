"""Regenerate LXMF 1.2.0 fixtures using explicitly pinned local Python references."""

import json
import os
from pathlib import Path
import subprocess
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
