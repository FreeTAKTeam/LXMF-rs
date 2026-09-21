#!/usr/bin/env python3
"""Generate HDLC compatibility vectors from an immutable Reticulum revision."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

REFERENCE = "99de23c040d507e3fefca19e87b182302902725d"
SOURCE = "RNS/Interfaces/util/HDLC.py"
DEFAULT_OUTPUT = Path("crates/libs/test-support/fixtures/rns_1_5_4_hdlc.json")


def generate(reference: Path) -> dict:
    head = subprocess.check_output(["git", "-C", str(reference), "rev-parse", "HEAD"], text=True).strip()
    if head != REFERENCE:
        raise ValueError(f"expected Reticulum {REFERENCE}, got {head}")
    source = reference / SOURCE
    committed = subprocess.check_output(["git", "-C", str(reference), "show", f"{REFERENCE}:{SOURCE}"])
    if source.read_bytes() != committed:
        raise ValueError("reference HDLC source differs from the pinned commit")
    spec = importlib.util.spec_from_file_location("rns_reference_hdlc", source)
    if spec is None or spec.loader is None:
        raise ValueError("cannot load reference HDLC source")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    cases = [
        ("empty", b""), ("flags", bytes([0x7e, 0x7d])),
        ("escape_lookalikes", bytes([0x7d, 0x5e, 0x7d, 0x5d, 0x00, 0xff])),
        ("all_octets", bytes(range(256))),
        ("reticulum_mtu", bytes(i % 256 for i in range(500))),
        ("rnode_mtu", bytes(i % 256 for i in range(508))),
        ("worst_case_escaping", bytes([0x7e, 0x7d]) * 250),
        ("backbone_payload", bytes(i % 256 for i in range(4096))),
    ]
    return {
        "reference": REFERENCE, "reference_version": "1.5.4-development",
        "source": SOURCE, "source_sha256": hashlib.sha256(committed).hexdigest(),
        "vectors": [{"name": name, "payload": payload.hex(), "frame": bytes(module.HDLC.frame(payload)).hex()}
                    for name, payload in cases],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rendered = json.dumps(generate(args.reference), indent=2) + "\n"
    if args.check:
        if args.output.read_text() != rendered:
            raise SystemExit("HDLC reference vectors differ; regenerate and review")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    print("HDLC: 8 pinned Python reference vectors verified")


if __name__ == "__main__":
    main()
