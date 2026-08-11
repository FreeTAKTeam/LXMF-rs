#!/usr/bin/env python3
"""Build pinned independent Reticulum peers and emit network interop evidence."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Any

from independent_interop_rns_rs import run_two_node
from independent_interop_failures import run_restart_scenarios
from independent_interop_five_node import run_five_node_scenarios
from independent_interop_chaos import run_chaos_scenarios
from independent_interop_reticulum_go import (
    run_multi_hop as run_reticulum_go_multi_hop,
    run_two_node as run_reticulum_go_two_node,
)
from independent_interop_routing import run_routing_scenarios
from independent_interop_shared import run_shared_instance_scenarios
from independent_interop_topology import run_multi_hop
from independent_interop_support import Evidence, command_output, environment, render_markdown


ROOT = Path(__file__).resolve().parents[2]
PINS = ROOT / "tools/interop/independent-implementations.toml"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--peer", choices=("rns-rs", "reticulum-go"), default="rns-rs")
    parser.add_argument("--level", choices=("pr", "nightly", "release"), default="pr")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--peer-root", type=Path)
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--skip-build", action="store_true")
    return parser.parse_args()


def load_pins() -> dict[str, Any]:
    return tomllib.loads(PINS.read_text(encoding="utf-8"))


def peer_key(name: str) -> str:
    return name.replace("-", "_")


def prepare_peer(
    peer: dict[str, Any], external_root: Path, supplied_root: Path | None
) -> Path:
    path = supplied_root.resolve() if supplied_root else external_root / peer["implementation"]
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["git", "clone", "--filter=blob:none", peer["repository"], str(path)],
            check=True,
        )
    if command_output(["git", "status", "--porcelain"], path):
        raise RuntimeError(f"peer checkout is dirty: {path}")
    revision = peer["revision"]
    try:
        subprocess.run(["git", "cat-file", "-e", f"{revision}^{{commit}}"], cwd=path, check=True)
    except subprocess.CalledProcessError:
        subprocess.run(["git", "fetch", "origin", revision], cwd=path, check=True)
    subprocess.run(["git", "checkout", "--detach", revision], cwd=path, check=True)
    actual = command_output(["git", "rev-parse", "HEAD"], path)
    if actual != revision:
        raise RuntimeError(f"peer revision mismatch: expected {revision}, got {actual}")
    return path


def build_peer(peer: dict[str, Any], peer_root: Path) -> Path:
    command = [str(value) for value in peer["build"]]
    subprocess.run(command, cwd=peer_root, check=True)
    binary = peer_root / peer["binary"]
    if not binary.is_file():
        raise RuntimeError(f"peer build did not produce {binary}")
    return binary


def build_lxmf_probe() -> Path:
    subprocess.run(
        [
            "cargo",
            "build",
            "--locked",
            "--release",
            "-p",
            "rns-tools",
            "--bin",
            "independent-interop-node",
        ],
        cwd=ROOT,
        check=True,
    )
    binary = ROOT / "target/release/independent-interop-node"
    if not binary.is_file():
        raise RuntimeError(f"LXMF-rs probe build did not produce {binary}")
    subprocess.run(
        ["cargo", "build", "--locked", "--release", "-p", "reticulumd", "--bin", "reticulumd"],
        cwd=ROOT,
        check=True,
    )
    return binary


def build_rns_rs_control() -> Path:
    manifest = ROOT / "tools/interop/adapters/rns-rs-control/Cargo.toml"
    subprocess.run(
        ["cargo", "build", "--locked", "--release", "--manifest-path", str(manifest)],
        cwd=ROOT,
        check=True,
    )
    binary = ROOT / "target/release/lxmf-rs-rns-rs-control"
    if not binary.is_file():
        raise RuntimeError(f"rns-rs control adapter build did not produce {binary}")
    shared = ROOT / "target/release/lxmf-rs-rns-rs-shared-client"
    if not shared.is_file():
        raise RuntimeError(f"rns-rs shared client adapter build did not produce {shared}")
    return binary


def output_root(args: argparse.Namespace) -> Path:
    if args.output:
        return args.output.resolve()
    stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    return ROOT / "target/interop/independent" / stamp


def main() -> int:
    args = parse_args()
    output = output_root(args)
    output.mkdir(parents=True, exist_ok=True)
    pins = load_pins()
    peer = pins["peers"][peer_key(args.peer)]
    try:
        external_root = ROOT / "target/interop/independent/external"
        peer_root = prepare_peer(peer, external_root, args.peer_root)
        peer_binary = peer_root / peer["binary"]
        rns_rs_control_binary = ROOT / "target/release/lxmf-rs-rns-rs-control"
        if not args.skip_build:
            peer_binary = build_peer(peer, peer_root)
            build_lxmf_probe()
            if args.peer == "rns-rs":
                rns_rs_control_binary = build_rns_rs_control()
        elif not peer_binary.is_file() or not (
            ROOT / "target/release/independent-interop-node"
        ).is_file() or not (ROOT / "target/release/reticulumd").is_file():
            raise RuntimeError("--skip-build requires existing peer and LXMF-rs binaries")
        elif args.peer == "rns-rs" and not rns_rs_control_binary.is_file():
            raise RuntimeError("--skip-build requires the existing rns-rs control adapter")
        elif args.peer == "rns-rs" and not (
            ROOT / "target/release/lxmf-rs-rns-rs-shared-client"
        ).is_file():
            raise RuntimeError("--skip-build requires the existing rns-rs shared client adapter")

        metadata = {
            "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "level": args.level,
            "lxmf_rs": {
                "version": (ROOT / "VERSION").read_text(encoding="utf-8").strip(),
                "revision": command_output(["git", "rev-parse", "HEAD"], ROOT),
                "build": "cargo build --locked --release -p rns-tools --bin independent-interop-node",
            },
            "rns_reference": {
                "version": pins["rns_reference_version"],
                "revision": pins["rns_reference_revision"],
            },
            "peer": peer,
            "environment": environment(),
            "test_topology": "isolated loopback TCP; independent processes and control planes",
            "artifact_root": str(output),
            "control_adapter": (
                {
                    "implementation": "tools/interop/adapters/rns-rs-control",
                    "peer_api": "pinned rns-rs public RnsNode API",
                    "peer_revision": peer["revision"],
                }
                if args.peer == "rns-rs"
                else {
                    "implementation": "pinned Reticulum-Go stdlib control client",
                    "peer_api": "Reticulum-Go authenticated HTTP/WebSocket control API",
                    "peer_revision": peer["revision"],
                }
            ),
        }
        evidence = Evidence(metadata)
        if args.peer == "rns-rs":
            run_two_node(
                output,
                ROOT,
                peer_root,
                peer_binary,
                rns_rs_control_binary,
                evidence,
                args.level,
            )
            run_multi_hop(output, ROOT, peer_root, peer_binary, evidence, args.level)
            run_five_node_scenarios(
                output, ROOT, peer_root, peer_binary, evidence, args.level
            )
            run_routing_scenarios(
                output,
                ROOT,
                peer_root,
                peer_binary,
                evidence,
                args.level,
            )
            run_restart_scenarios(
                output,
                ROOT,
                peer_root,
                peer_binary,
                evidence,
                args.level,
            )
            run_shared_instance_scenarios(
                output,
                ROOT,
                peer_root,
                ROOT / "target/release/lxmf-rs-rns-rs-shared-client",
                evidence,
                args.level,
            )
            run_chaos_scenarios(
                output,
                ROOT,
                peer_root,
                peer_binary,
                rns_rs_control_binary,
                evidence,
                args.level,
            )
        else:
            run_reticulum_go_two_node(
                output,
                ROOT,
                peer_root,
                peer_binary,
                evidence,
                args.level,
            )
            run_reticulum_go_multi_hop(
                output,
                ROOT,
                peer_root,
                peer_binary,
                evidence,
                args.level,
            )
        report = evidence.report()
        json_path = output / "independent-interop.json"
        markdown_path = output / "independent-interop.md"
        json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        markdown_path.write_text(render_markdown(report), encoding="utf-8")
        print(f"Independent interop JSON: {json_path}")
        print(f"Independent interop report: {markdown_path}")
        if not args.keep:
            shutil.rmtree(output / "config", ignore_errors=True)
        return 0 if report["summary"]["status"] == "PASS" else 1
    except (OSError, KeyError, TypeError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        blocked = {
            "schema": "lxmf-rs-independent-interop-v1",
            "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "level": args.level,
            "peer": peer,
            "summary": {"status": "BLOCKED", "counts": {"BLOCKED": 1}},
            "scenarios": [
                {
                    "scenario": "peer build or harness startup",
                    "direction": "N/A",
                    "topology": "N/A",
                    "status": "BLOCKED",
                    "runtime_seconds": 0,
                    "bytes_transferred": None,
                    "content_sha256": None,
                    "failure_reason": str(error),
                }
            ],
        }
        (output / "independent-interop.json").write_text(
            json.dumps(blocked, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(f"independent_interop: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
