#!/usr/bin/env python3
"""Run the pinned Python-to-Rust wire conformance lane.

This lane is intentionally outside the protocol crates. It consumes the same
encoded bytes that the Rust test-support fixture consumes, asks the pinned
Python reference to decode Rust-produced bytes, and records every scenario in
one machine-readable report.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
FIXTURE_PATH = ROOT / "tools/interop/python-rust-wire-conformance-v1.json"
RUST_VECTOR_SOURCE = ROOT / "crates/libs/test-support/tests/reticulum_spec_vectors.rs"
RUST_LXMF_FIXTURE = ROOT / "crates/libs/lxmf-core/tests/fixtures/lxmf_interop/fixtures.json"
EXPECTED_RETICULUM_REVISION = "99de23c040d507e3fefca19e87b182302902725d"
EXPECTED_LXMF_REVISION = "727830cefda83d9c6e3982b48675425f3f988f9c"


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def repo_path(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT))
    except ValueError:
        return str(path.resolve())


def command_output(command: list[str], cwd: Path = ROOT) -> str | None:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
    except OSError:
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout.strip()


def git_metadata(path: Path) -> dict[str, Any]:
    revision = command_output(["git", "-C", str(path), "rev-parse", "HEAD"])
    status = command_output(
        ["git", "-C", str(path), "status", "--porcelain", "--untracked-files=all"]
    )
    status_lines = status.splitlines() if status is not None and status else []
    return {
        "path": repo_path(path),
        "revision": revision,
        "clean": status is not None and not status_lines,
        "status": status_lines,
    }


def resolve_reference_path(configured: str | None, variable: str, candidates: tuple[Path, ...]) -> Path:
    if configured:
        return Path(configured).expanduser().resolve()
    from_environment = os.environ.get(variable)
    if from_environment:
        return Path(from_environment).expanduser().resolve()
    for candidate in candidates:
        if candidate.is_dir():
            return candidate.resolve()
    return candidates[0].resolve()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def decode_hex(value: str, label: str) -> bytes:
    try:
        return bytes.fromhex(value)
    except ValueError as error:
        raise ValueError(f"{label} is not valid hexadecimal: {error}") from error


def vector_map(manifest: dict[str, Any]) -> dict[str, dict[str, Any]]:
    vectors = manifest.get("vectors")
    if not isinstance(vectors, list):
        raise ValueError("wire conformance manifest vectors must be an array")
    result: dict[str, dict[str, Any]] = {}
    for vector in vectors:
        if not isinstance(vector, dict) or not isinstance(vector.get("id"), str):
            raise ValueError("every wire vector must be an object with a string id")
        identifier = vector["id"]
        if identifier in result:
            raise ValueError(f"duplicate wire vector id: {identifier}")
        if not isinstance(vector.get("wire_hex"), str):
            raise ValueError(f"wire vector {identifier} is missing wire_hex")
        result[identifier] = vector
    return result


def result_record(
    identifier: str,
    direction: str,
    kind: str,
    wire: bytes,
    status: str,
    reason: str,
) -> dict[str, Any]:
    return {
        "id": identifier,
        "direction": direction,
        "kind": kind,
        "status": status,
        "reason": reason,
        "wire_length": len(wire),
        "wire_sha256": hashlib.sha256(wire).hexdigest(),
    }


def field_values(source: str, field: str) -> list[str]:
    return re.findall(rf"{re.escape(field)}:\s*\"([0-9a-f]+)\"", source)


def import_reference_modules(reticulum_path: Path, lxmf_path: Path) -> tuple[Any, Any]:
    sys.path.insert(0, str(reticulum_path))
    sys.path.insert(0, str(lxmf_path))
    import RNS  # type: ignore[import-not-found]
    from LXMF.LXMessage import LXMessage  # type: ignore[import-not-found]

    return RNS, LXMessage


def validate_manifest_provenance(manifest: dict[str, Any]) -> None:
    if manifest.get("schema_version") != 1:
        raise ValueError("wire conformance manifest schema_version must be 1")
    generator = manifest.get("generator")
    if not isinstance(generator, dict):
        raise ValueError("wire conformance manifest is missing generator provenance")
    if generator.get("script") != "tools/scripts/python_wire_conformance.py":
        raise ValueError("wire conformance generator script provenance is incorrect")
    if generator.get("reticulum_revision") != EXPECTED_RETICULUM_REVISION:
        raise ValueError("wire conformance Reticulum pin does not match the acceptance target")
    if generator.get("lxmf_revision") != EXPECTED_LXMF_REVISION:
        raise ValueError("wire conformance LXMF pin does not match the acceptance target")
    if generator.get("rust_fixture_source") != repo_path(RUST_LXMF_FIXTURE):
        raise ValueError("wire conformance Rust fixture provenance is incorrect")
    vector_map(manifest)


def validate_python_generated_vectors(
    manifest: dict[str, Any], RNS: Any, LXMessage: Any, records: list[dict[str, Any]]
) -> None:
    vectors = vector_map(manifest)

    packet_vector = vectors["python_plain_packet"]
    packet_wire = decode_hex(packet_vector["wire_hex"], packet_vector["id"])
    packet = RNS.Packet(None, packet_wire)
    if not packet.unpack():
        raise ValueError("python_plain_packet: Python reference rejected its own packet vector")
    expected = packet_vector["expected"]
    if packet.header_type != 0:
        raise ValueError("python_plain_packet: header_type diverged")
    if packet.packet_type != RNS.Packet.DATA:
        raise ValueError("python_plain_packet: packet_type diverged")
    if packet.destination_type != RNS.Destination.PLAIN:
        raise ValueError("python_plain_packet: destination_type diverged")
    if packet.context != RNS.Packet.NONE:
        raise ValueError("python_plain_packet: context diverged")
    if packet.data != expected["payload_utf8"].encode("utf-8"):
        raise ValueError("python_plain_packet: payload bytes diverged")
    records.append(
        result_record(
            packet_vector["id"],
            packet_vector["direction"],
            packet_vector["kind"],
            packet_wire,
            "pass",
            "pinned Python Reticulum decoded the encoded packet bytes",
        )
    )

    for identifier in ("python_lxmf_wire", "rust_lxmf_wire"):
        vector = vectors[identifier]
        wire = decode_hex(vector["wire_hex"], identifier)
        message = LXMessage.unpack_from_bytes(wire)
        expected = vector["expected"]
        if message.packed != wire:
            raise ValueError(f"{identifier}: Python did not preserve the encoded bytes")
        if message.destination_hash.hex() != expected["destination_hex"]:
            raise ValueError(f"{identifier}: destination hash diverged")
        if message.source_hash.hex() != expected["source_hex"]:
            raise ValueError(f"{identifier}: source hash diverged")
        if message.title != expected["title_utf8"].encode("utf-8"):
            raise ValueError(f"{identifier}: title bytes diverged")
        if message.content != expected["content_utf8"].encode("utf-8"):
            raise ValueError(f"{identifier}: content bytes diverged")
        if message.timestamp != expected["timestamp"]:
            raise ValueError(f"{identifier}: timestamp diverged")
        records.append(
            result_record(
                identifier,
                vector["direction"],
                vector["kind"],
                wire,
                "pass",
                "pinned Python LXMF decoded the encoded wire bytes",
            )
        )


def validate_rust_fixture_provenance(manifest: dict[str, Any]) -> None:
    fixture = load_json(RUST_LXMF_FIXTURE)
    direct = fixture.get("fixtures", {}).get("direct_no_stamp", {}).get("hex")
    expected = vector_map(manifest)["rust_lxmf_wire"]["wire_hex"]
    if direct != expected:
        raise ValueError("rust_lxmf_wire differs from its declared Rust fixture source")


def validate_rust_spec_vectors(RNS: Any, LXMessage: Any, records: list[dict[str, Any]]) -> None:
    source = RUST_VECTOR_SOURCE.read_text(encoding="utf-8")
    private_keys = field_values(source, "private_key_hex")
    public_keys = field_values(source, "public_key_hex")
    destination_hashes = field_values(source, "destination_hash_hex")
    if len(private_keys) != 2 or len(public_keys) != 2 or len(destination_hashes) < 2:
        raise ValueError("reticulum_spec_vectors.rs identity corpus shape changed")

    for index, private_key_hex in enumerate(private_keys):
        identity = RNS.Identity.from_bytes(decode_hex(private_key_hex, f"identity[{index}]"))
        if identity is None:
            raise ValueError(f"identity[{index}]: Python reference rejected the private key")
        if identity.get_public_key().hex() != public_keys[index]:
            raise ValueError(f"identity[{index}]: public key bytes diverged")
        destination_hash = RNS.Destination.hash(identity, "lxmf", "delivery")
        if destination_hash.hex() != destination_hashes[index]:
            raise ValueError(f"identity[{index}]: destination hash bytes diverged")
        records.append(
            result_record(
                f"identity_{index}",
                "both",
                "identity_derivation",
                identity.get_public_key(),
                "pass",
                "Python Reticulum reproduced the Rust identity vector",
            )
        )

    announces = field_values(source, "wire_bytes_hex")
    if len(announces) != 2:
        raise ValueError("reticulum_spec_vectors.rs announce corpus shape changed")
    for index, wire_hex in enumerate(announces):
        wire = decode_hex(wire_hex, f"announce_{index}")
        packet = RNS.Packet(None, wire)
        if not packet.unpack() or packet.packet_type != RNS.Packet.ANNOUNCE:
            raise ValueError(f"announce_{index}: Python rejected the encoded announce")
        records.append(
            result_record(
                f"announce_{index}",
                "both",
                "announce_packet",
                wire,
                "pass",
                "Python Reticulum decoded the Rust announce vector bytes",
            )
        )

    lxmf_vectors = field_values(source, "lxmf_packed_hex")
    if len(lxmf_vectors) != 2:
        raise ValueError("reticulum_spec_vectors.rs LXMF corpus shape changed")
    for index, wire_hex in enumerate(lxmf_vectors):
        wire = decode_hex(wire_hex, f"lxmf_vector_{index}")
        message = LXMessage.unpack_from_bytes(wire)
        if message.packed != wire:
            raise ValueError(f"lxmf_vector_{index}: Python did not preserve wire bytes")
        records.append(
            result_record(
                f"lxmf_vector_{index}",
                "both",
                "lxmf_wire",
                wire,
                "pass",
                "Python Reticulum/LXMF decoded the Rust LXMF vector bytes",
            )
        )

    for field, packet_kind in (
        ("linkrequest_raw_hex", RNS.Packet.LINKREQUEST),
        ("lrproof_raw_hex", RNS.Packet.PROOF),
        ("lrrtt_raw_hex", RNS.Packet.DATA),
    ):
        values = field_values(source, field)
        if len(values) != 1:
            raise ValueError(f"reticulum_spec_vectors.rs {field} corpus shape changed")
        wire = decode_hex(values[0], field)
        packet = RNS.Packet(None, wire)
        if not packet.unpack() or packet.packet_type != packet_kind:
            raise ValueError(f"{field}: Python rejected the encoded link/proof vector")
        records.append(
            result_record(
                field,
                "both",
                "link_packet",
                wire,
                "pass",
                "Python Reticulum decoded the Rust link/proof vector bytes",
            )
        )


def validate_negative_vectors(manifest: dict[str, Any], RNS: Any, LXMessage: Any, records: list[dict[str, Any]]) -> None:
    vectors = vector_map(manifest)
    negatives = manifest.get("negative_vectors")
    if not isinstance(negatives, list) or not negatives:
        raise ValueError("wire conformance manifest must contain negative vectors")

    for negative in negatives:
        identifier = negative["id"]
        if "wire_hex" in negative:
            wire = decode_hex(negative["wire_hex"], identifier)
        else:
            base = bytearray(decode_hex(vectors[negative["base_vector"]]["wire_hex"], identifier))
            if "truncate_bytes" in negative:
                count = int(negative["truncate_bytes"])
                if count <= 0 or count >= len(base):
                    raise ValueError(f"{identifier}: invalid truncation operation")
                wire = bytes(base[:-count])
            elif "replace_last_byte_hex" in negative:
                replacement = decode_hex(negative["replace_last_byte_hex"], identifier)
                if len(replacement) != 1:
                    raise ValueError(f"{identifier}: replacement must be one byte")
                base[-1] = replacement[0]
                wire = bytes(base)
            else:
                raise ValueError(f"{identifier}: no corruption operation")

        rejected = False
        if negative["kind"] == "reticulum_packet":
            rejected = not RNS.Packet(None, wire).unpack()
        elif negative["kind"] == "lxmf_wire":
            try:
                LXMessage.unpack_from_bytes(wire)
            except Exception:
                rejected = True
        else:
            raise ValueError(f"{identifier}: unsupported negative kind")
        if not rejected:
            raise ValueError(f"{identifier}: pinned Python reference accepted corrupted bytes")
        records.append(
            result_record(
                identifier,
                negative["direction"],
                negative["kind"],
                wire,
                "pass",
                negative["failure_boundary"],
            )
        )


def default_output() -> Path:
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return ROOT / "target" / "interop" / "python-rust-wire-conformance" / stamp / "report.json"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=default_output())
    parser.add_argument("--python-rns-path")
    parser.add_argument("--python-lxmf-path")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    args.output = args.output if args.output.is_absolute() else ROOT / args.output
    args.output = args.output.resolve()
    return args


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if args.self_test:
        manifest = load_json(FIXTURE_PATH)
        validate_manifest_provenance(manifest)
        print("python-wire-conformance: self-test ok")
        return 0

    started_at = utc_now()
    reticulum_path = resolve_reference_path(
        args.python_rns_path,
        "RETICULUM_PY_REPO",
        (ROOT / ".tmp/python-refs/Reticulum", ROOT.parent / "reticulum"),
    )
    lxmf_path = resolve_reference_path(
        args.python_lxmf_path,
        "LXMF_PY_REPO",
        (ROOT / ".tmp/python-refs/LXMF", ROOT.parent / "lxmf"),
    )
    candidate = git_metadata(ROOT)
    reticulum = git_metadata(reticulum_path)
    lxmf = git_metadata(lxmf_path)
    records: list[dict[str, Any]] = []
    failures: list[str] = []
    RNS = None
    LXMessage = None

    try:
        manifest = load_json(FIXTURE_PATH)
        validate_manifest_provenance(manifest)
        validate_rust_fixture_provenance(manifest)
    except Exception as error:
        failures.append(f"fixture provenance: {error}")
        manifest = {}

    if reticulum["revision"] != EXPECTED_RETICULUM_REVISION:
        failures.append(
            f"Python Reticulum revision mismatch: {reticulum['revision']} != {EXPECTED_RETICULUM_REVISION}"
        )
    if lxmf["revision"] != EXPECTED_LXMF_REVISION:
        failures.append(f"Python LXMF revision mismatch: {lxmf['revision']} != {EXPECTED_LXMF_REVISION}")
    if not reticulum["clean"]:
        failures.append("Python Reticulum checkout is dirty or unavailable")
    if not lxmf["clean"]:
        failures.append("Python LXMF checkout is dirty or unavailable")

    if not failures:
        try:
            RNS, LXMessage = import_reference_modules(reticulum_path, lxmf_path)
            validate_python_generated_vectors(manifest, RNS, LXMessage, records)
            validate_rust_spec_vectors(RNS, LXMessage, records)
            validate_negative_vectors(manifest, RNS, LXMessage, records)
        except Exception as error:
            failures.append(str(error))

    report = {
        "schema_version": 1,
        "status": "pass" if not failures else "fail",
        "started_at": started_at,
        "finished_at": utc_now(),
        "candidate": candidate,
        "reference": {"reticulum": reticulum, "lxmf": lxmf},
        "toolchain": {
            "python": sys.version.split()[0],
            "python_implementation": platform.python_implementation(),
            "rustc": command_output(["rustc", "-Vv"]),
            "cargo": command_output(["cargo", "-V"]),
        },
        "fixture": repo_path(FIXTURE_PATH),
        "failures": failures,
        "passed": sum(record["status"] == "pass" for record in records),
        "failed": len(failures),
        "skipped": 0,
        "vectors": records,
        "command": "python3 tools/scripts/python_wire_conformance.py --output target/interop/python-rust-wire-conformance/report.json",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"python-wire-conformance: status={report['status']} report={repo_path(args.output)}")
    for failure in failures:
        print(f"python-wire-conformance: {failure}", file=sys.stderr)
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
