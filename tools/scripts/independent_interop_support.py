#!/usr/bin/env python3
"""Process, control-plane, and evidence support for independent interop."""

from __future__ import annotations

import base64
import hashlib
import json
import os
import platform
import socket
import subprocess
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Callable


def free_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def command_output(command: list[str], cwd: Path | None = None) -> str:
    return subprocess.check_output(command, cwd=cwd, text=True).strip()


def cpu_model() -> str:
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.is_file():
        for line in cpuinfo.read_text(encoding="utf-8").splitlines():
            if line.lower().startswith("model name") and ":" in line:
                return line.split(":", 1)[1].strip()
    return platform.processor() or platform.machine()


def wait_until(
    label: str,
    predicate: Callable[[], Any],
    timeout: float = 15.0,
    interval: float = 0.1,
) -> Any:
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except Exception as error:  # Retain the final probe error as evidence.
            last_error = error
        time.sleep(interval)
    detail = f": {last_error}" if last_error else ""
    raise TimeoutError(f"timed out waiting for {label}{detail}")


class ManagedProcess:
    def __init__(
        self,
        name: str,
        command: list[str],
        cwd: Path,
        log_path: Path,
        env: dict[str, str] | None = None,
    ) -> None:
        self.name = name
        self.log_path = log_path
        log_path.parent.mkdir(parents=True, exist_ok=True)
        self._log = log_path.open("wb")
        child_env = os.environ.copy()
        if env:
            child_env.update(env)
        self.process = subprocess.Popen(
            command,
            cwd=cwd,
            env=child_env,
            stdout=self._log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )

    def check(self) -> None:
        returncode = self.process.poll()
        if returncode is not None:
            raise RuntimeError(
                f"{self.name} exited with {returncode}; see {self.log_path}"
            )

    def stop(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        self._log.close()


class RustProbe:
    def __init__(self, port: int, timeout: float = 5.0) -> None:
        self.port = port
        self.timeout = timeout

    def call(self, method: str, params: dict[str, Any] | None = None) -> Any:
        request = json.dumps({"method": method, "params": params or {}}).encode() + b"\n"
        with socket.create_connection(
            ("127.0.0.1", self.port), timeout=self.timeout
        ) as connection:
            connection.sendall(request)
            connection.shutdown(socket.SHUT_WR)
            chunks = []
            while True:
                chunk = connection.recv(1024 * 1024)
                if not chunk:
                    break
                chunks.append(chunk)
        response = json.loads(b"".join(chunks))
        if not response.get("ok"):
            raise RuntimeError(str(response.get("error", "Rust probe request failed")))
        return response["result"]

    def events(self, clear: bool = False) -> list[dict[str, Any]]:
        return list(self.call("events", {"clear": clear})["events"])


class RnsRsControl(RustProbe):
    """JSON-line control extension over pinned rns-rs public APIs."""



class RnsRsNode:
    def __init__(self, port: int) -> None:
        self.base = f"http://127.0.0.1:{port}"

    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any:
        encoded = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(
            self.base + path,
            data=encoded,
            method=method,
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                return json.loads(response.read())
        except urllib.error.HTTPError as error:
            detail = error.read().decode(errors="replace")
            raise RuntimeError(f"{method} {path} returned {error.code}: {detail}") from error

    def get(self, path: str) -> Any:
        return self.request("GET", path)

    def post(self, path: str, body: dict[str, Any]) -> Any:
        return self.request("POST", path, body)


class Evidence:
    def __init__(self, metadata: dict[str, Any]) -> None:
        self.metadata = metadata
        self.scenarios: list[dict[str, Any]] = []

    def run(
        self,
        scenario: str,
        direction: str,
        action: Callable[[], dict[str, Any] | None],
        *,
        topology: str = "two-node",
        expected_bytes: int | None = None,
        content_hash: str | None = None,
        failure_owner: str | None = None,
        classification: str | None = None,
        normative_reference: str | None = None,
    ) -> dict[str, Any] | None:
        started = time.monotonic()
        record: dict[str, Any] = {
            "scenario": scenario,
            "direction": direction,
            "topology": topology,
            "status": "PASS",
            "runtime_seconds": None,
            "bytes_transferred": expected_bytes,
            "content_sha256": content_hash,
            "failure_reason": None,
            "failure_owner": None,
            "classification": None,
            "normative_reference": normative_reference,
        }
        try:
            details = action() or {}
            record.update(details)
            return details
        except Exception as error:
            record["status"] = "FAIL"
            record["failure_reason"] = str(error)
            record["failure_owner"] = failure_owner or "lxmf-rs-or-undetermined"
            record["classification"] = classification or "protocol_incompatibility"
            return None
        finally:
            record["runtime_seconds"] = round(time.monotonic() - started, 6)
            self.scenarios.append(record)

    def record(
        self,
        scenario: str,
        direction: str,
        status: str,
        reason: str,
        *,
        topology: str = "two-node",
        classification: str,
        failure_owner: str | None = None,
    ) -> None:
        if status not in {"BLOCKED", "UNSUPPORTED"}:
            raise ValueError(f"non-executed evidence status must be BLOCKED or UNSUPPORTED, got {status}")
        self.scenarios.append(
            {
                "scenario": scenario,
                "direction": direction,
                "topology": topology,
                "status": status,
                "runtime_seconds": 0.0,
                "bytes_transferred": None,
                "content_sha256": None,
                "failure_reason": reason,
                "failure_owner": failure_owner,
                "classification": classification,
                "normative_reference": None,
            }
        )

    def report(self) -> dict[str, Any]:
        counts: dict[str, int] = {}
        for scenario in self.scenarios:
            status = str(scenario["status"])
            counts[status] = counts.get(status, 0) + 1
        if counts.get("FAIL", 0):
            overall = "FAIL"
        elif counts.get("BLOCKED", 0):
            overall = "BLOCKED"
        elif counts.get("PASS", 0):
            overall = "PASS"
        else:
            overall = "UNSUPPORTED"
        return {
            "schema": "lxmf-rs-independent-interop-v1",
            **self.metadata,
            "summary": {
                "status": overall,
                "counts": counts,
            },
            "scenarios": self.scenarios,
        }


def environment() -> dict[str, Any]:
    return {
        "os": platform.platform(),
        "architecture": platform.machine(),
        "cpu": cpu_model(),
        "python": platform.python_version(),
        "rustc": command_output(["rustc", "--version"]),
        "cargo": command_output(["cargo", "--version"]),
    }


def render_markdown(report: dict[str, Any]) -> str:
    peer = report["peer"]
    lines = [
        "# Independent Reticulum interoperability",
        "",
        "<!-- GENERATED: tools/scripts/independent_interop.py -->",
        "",
        f"- LXMF-rs: `{report['lxmf_rs']['revision']}`",
        f"- RNS reference: `{report['rns_reference']['version']}` (`{report['rns_reference']['revision']}`)",
        f"- Peer: `{peer['implementation']}` `{peer['version']}` (`{peer['revision']}`)",
        f"- Result: **{report['summary']['status']}**",
        "",
        "| Topology | Scenario | Direction | Result | Owner / class | Runtime | Bytes | SHA-256 / failure |",
        "|---|---|---|---:|---|---:|---:|---|",
    ]
    for row in report["scenarios"]:
        evidence = row.get("content_sha256") or row.get("failure_reason") or "-"
        owner = row.get("failure_owner") or "-"
        classification = row.get("classification") or "-"
        lines.append(
            f"| {row['topology']} | {row['scenario']} | {row['direction']} | "
            f"{row['status']} | {owner} / {classification} | {row['runtime_seconds']:.3f}s | "
            f"{row.get('bytes_transferred') or '-'} | {evidence} |"
        )
    lines.append("")
    return "\n".join(lines)
