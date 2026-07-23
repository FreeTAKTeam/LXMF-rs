#!/usr/bin/env python3
"""Generate and check the pinned-Python SDK-access and daemon operation inventory."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
PYTHON_PARITY = ROOT / "docs/status/python-surface-parity.json"
JSON_OUT = ROOT / "docs/status/sdk-zmq-parity.json"
MARKDOWN_OUT = ROOT / "docs/status/sdk-zmq-parity-matrix.md"
SPEC_DIR = ROOT / "crates/libs/rns-rpc/src/rpc/daemon/sdk_operations_parts"
ZMQ_DIR = ROOT / "crates/libs/lxmf-sdk/src/backend/zmq_pipeline"
SCHEMA_DIR = ROOT / "docs/schemas/sdk/v2/rpc"
DAEMON_SURFACE_TOKENS = ("lxmf-sdk", "reticulum-rs-rpc", "reticulumd")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def repo_relative(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def classify_python_item(item: dict[str, Any]) -> dict[str, Any]:
    if item["implementation"] == "not-applicable":
        access = "not-applicable"
    else:
        surfaces = " ".join(item.get("rust_surface", [])).lower()
        has_daemon = any(token in surfaces for token in DAEMON_SURFACE_TOKENS)
        access = "daemon-sdk" if has_daemon else "local-library" if surfaces else "unclassified"
    return {
        "id": item["id"],
        "access": access,
        "implementation": item["implementation"],
        "rust_surface": item.get("rust_surface", []),
        "evidence": item.get("evidence", []),
        **({"notes": item["notes"]} if item.get("notes") else {}),
    }


def string_field(block: str, field: str) -> str:
    match = re.search(rf'{field}:\s*"([^"]+)"', block)
    if not match:
        raise ValueError(f"operation spec missing {field}: {block[:120]!r}")
    return match.group(1)


def list_field(block: str, field: str) -> list[str]:
    match = re.search(rf"{field}:\s*&\[(.*?)\]", block, re.DOTALL)
    if not match:
        raise ValueError(f"operation spec missing {field}: {block[:120]!r}")
    return re.findall(r'"([^"]+)"', match.group(1))


def operation_specs() -> list[dict[str, Any]]:
    specs: list[dict[str, Any]] = []
    for path in sorted(SPEC_DIR.glob("*operation_specs.rs")):
        source = read(path)
        for block in re.findall(r"SdkOperationSpec\s*\{(.*?)\n\s*\}", source, re.DOTALL):
            if "$id" in block:
                continue
            operation_id = string_field(block, "id")
            method = string_field(block, "rpc_method")
            kind = string_field(block, "kind")
            schema = SCHEMA_DIR / f"{method}.schema.json"
            specs.append(
                {
                    "id": operation_id,
                    "rpc_method": method,
                    "kind": kind,
                    "authorization": "read" if kind == "query" else "mutate",
                    "required_capabilities": list_field(block, "required_capabilities"),
                    "typed_contract": (
                        repo_relative(schema) if schema.is_file() else "Rust SDK serde types"
                    ),
                    "http_implementation": "RpcBackendClient/RpcDaemon framed RPC",
                    "zmq_implementation": "ZmqPipelineBackendClient sdk_envelope_execute_v2",
                    "automated_evidence": [
                        "rns-rpc daemon operation registry tests",
                        "lxmf-sdk ZeroMQ envelope transport contract tests",
                    ],
                    "source": repo_relative(path),
                }
            )
        for operation_id, group, kind, capability, method in re.findall(
            r'rns_operation!\("([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)"\)',
            source,
        ):
            specs.append(
                {
                    "id": operation_id,
                    "rpc_method": method,
                    "kind": kind,
                    "authorization": "read" if kind == "query" else "mutate",
                    "required_capabilities": [capability],
                    "typed_contract": "Rust SDK serde types",
                    "http_implementation": "RpcBackendClient/RpcDaemon framed RPC",
                    "zmq_implementation": "ZmqPipelineBackendClient sdk_envelope_execute_v2",
                    "automated_evidence": [
                        "rns-rpc daemon operation registry tests",
                        "lxmf-sdk ZeroMQ envelope transport contract tests",
                    ],
                    "source": repo_relative(path),
                }
            )
    specs.sort(key=lambda value: value["id"])
    duplicate_ids = sorted({spec["id"] for spec in specs if sum(s["id"] == spec["id"] for s in specs) > 1})
    if duplicate_ids:
        raise ValueError(f"duplicate daemon operation ids: {duplicate_ids}")
    return specs


def build() -> dict[str, Any]:
    python = json.loads(read(PYTHON_PARITY))
    entries = [classify_python_item(item) for item in python["items"]]
    counts: dict[str, int] = {}
    for entry in entries:
        counts[entry["access"]] = counts.get(entry["access"], 0) + 1
    if counts.get("unclassified"):
        missing = [entry["id"] for entry in entries if entry["access"] == "unclassified"]
        raise ValueError(f"unclassified pinned-Python entries: {missing[:20]}")
    operations = operation_specs()
    if not operations:
        raise ValueError("daemon operation inventory is empty")
    return {
        "schema_version": 1,
        "sdk_contract_release": "v2.6",
        "schema_namespace": "v2",
        "protocol_version": 2,
        "compatible_request_contracts": ["v2.5", "v2.6"],
        "python_references": python["references"],
        "summary": {"python_entries": len(entries), **dict(sorted(counts.items())), "daemon_operations": len(operations)},
        "python_access": entries,
        "daemon_operations": operations,
    }


def markdown(payload: dict[str, Any]) -> str:
    summary = payload["summary"]
    rows = [
        "# ZeroMQ SDK access parity",
        "",
        "<!-- GENERATED: tools/scripts/sdk_zmq_parity.py -->",
        "",
        f"SDK contract: `{payload['sdk_contract_release']}` in schema namespace `v2` and protocol version `2`.",
        "",
        f"Pinned-Python entries: **{summary['python_entries']}** — daemon SDK: **{summary.get('daemon-sdk', 0)}**, local library: **{summary.get('local-library', 0)}**, provenance-backed not applicable: **{summary.get('not-applicable', 0)}**.",
        "",
        f"Daemon operations inventoried: **{summary['daemon_operations']}**. Every operation uses the shared framed-RPC codec over HTTP/Unix and ZeroMQ; authorization is derived from query (`read`) versus command (`mutate`) semantics.",
        "",
        "The complete row-level Python classification and daemon capability inventory are in [`sdk-zmq-parity.json`](sdk-zmq-parity.json). This file is generated and checked for drift in CI.",
        "",
        "| Operation | RPC method | Auth | Capabilities | Typed contract |",
        "|---|---|---|---|---|",
    ]
    for operation in payload["daemon_operations"]:
        capabilities = ", ".join(f"`{item}`" for item in operation["required_capabilities"]) or "none"
        rows.append(
            f"| `{operation['id']}` | `{operation['rpc_method']}` | {operation['authorization']} | {capabilities} | `{operation['typed_contract']}` |"
        )
    rows.append("")
    return "\n".join(rows)


def serialized(payload: dict[str, Any]) -> str:
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def main() -> int:
    args = parse_args()
    try:
        payload = build()
        expected = {JSON_OUT: serialized(payload), MARKDOWN_OUT: markdown(payload)}
        if args.check:
            drift = [
                repo_relative(path)
                for path, content in expected.items()
                if not path.is_file() or read(path) != content
            ]
            if drift:
                raise ValueError(f"ZeroMQ SDK parity drift: regenerate {', '.join(drift)}")
        else:
            for path, content in expected.items():
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8", newline="\n")
    except (OSError, KeyError, ValueError) as error:
        print(f"sdk_zmq_parity: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
