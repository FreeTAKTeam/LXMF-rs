#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

PYTHON_BIN="${PYTHON_BIN:-python3}"
PYTHON_BIN="${LXMF_PYTHON_BIN:-${PYTHON_BIN}}"
RETICULUM_PY_REPO="${RETICULUM_PY_REPO:-${REPO_ROOT}/../reticulum}"
LXMF_PY_REPO="${LXMF_PY_REPO:-${REPO_ROOT}/../lxmf}"
LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/interop/python-lxmd-rust-lxmd}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
PERFORMANCE_MODE="${PERFORMANCE_MODE:-0}"
PERFORMANCE_PAYLOAD_BYTES="${PERFORMANCE_PAYLOAD_BYTES:-0}"
if [[ "${PERFORMANCE_MODE}" == "1" ]]; then
  PERFORMANCE_POLL_SECS="${PERFORMANCE_POLL_SECS:-0.02}"
  PERFORMANCE_POLL_ATTEMPTS="$((TIMEOUT_SECS * 50))"
else
  PERFORMANCE_POLL_SECS="${PERFORMANCE_POLL_SECS:-1}"
  PERFORMANCE_POLL_ATTEMPTS="${TIMEOUT_SECS}"
fi
PERFORMANCE_START_NS=""
PERFORMANCE_END_NS=""
PATH_DISCOVERY_TIMEOUT_SECS="${PATH_DISCOVERY_TIMEOUT_SECS:-20}"
PATH_DISCOVERY_QUIET_SECS="${PATH_DISCOVERY_QUIET_SECS:-2}"
REMOTE_STATUS_TIMEOUT_SECS="${REMOTE_STATUS_TIMEOUT_SECS:-300}"
REMOTE_STATUS_ATTEMPTS="${REMOTE_STATUS_ATTEMPTS:-2}"
REMOTE_CONTROL_PATH_TIMEOUT_SECS="${REMOTE_CONTROL_PATH_TIMEOUT_SECS:-120}"
REMOTE_CONTROL_SETTLE_SECS="${REMOTE_CONTROL_SETTLE_SECS:-2}"
REMOTE_STATUS_PREFLIGHT="${REMOTE_STATUS_PREFLIGHT:-0}"
COMPAT_CASE="${COMPAT_CASE:-direct_python_to_rust}"
PROPAGATION_PEERING_COST="${PROPAGATION_PEERING_COST:-8}"
RUST_ANNOUNCE_AT_START="yes"
RUST_ANNOUNCE_INTERVAL_LINE="announce_interval = 1"
PY_ANNOUNCE_AT_START="yes"
PY_ANNOUNCE_INTERVAL_LINE="announce_interval = 1"
if [[ "${COMPAT_CASE}" == "rns_path_request_python_to_rust" ]]; then
  RUST_ANNOUNCE_AT_START="no"
  RUST_ANNOUNCE_INTERVAL_LINE=""
fi
if [[ "${COMPAT_CASE}" == "rns_path_request_rust_to_python" || "${COMPAT_CASE}" == "rns_path_request_rust_to_python_scoped_refresh" ]]; then
  PY_ANNOUNCE_AT_START="no"
  PY_ANNOUNCE_INTERVAL_LINE=""
fi

RUST_RPC_ADDR="${RUST_RPC_ADDR:-127.0.0.1:$((42430 + ($$ % 1000)))}"
RUST_TRANSPORT_ADDR="${RUST_TRANSPORT_ADDR:-127.0.0.1:$((37430 + ($$ % 1000)))}"
RUST_TRANSPORT_HOST="${RUST_TRANSPORT_ADDR%:*}"
RUST_TRANSPORT_PORT="${RUST_TRANSPORT_ADDR##*:}"

PY_SHARED_INSTANCE_PORT="${PY_SHARED_INSTANCE_PORT:-$((39428 + ($$ % 200)))}"
PY_INSTANCE_CONTROL_PORT="${PY_INSTANCE_CONTROL_PORT:-$((PY_SHARED_INSTANCE_PORT + 1))}"
PY_ENDPOINT_CONTROL_PORT="${PY_ENDPOINT_CONTROL_PORT:-$((PY_INSTANCE_CONTROL_PORT + 10))}"
PY_ENDPOINT_HELPER="${PY_ENDPOINT_HELPER:-${REPO_ROOT}/crates/apps/lxmf-cli/tests/support/python_lxmf_endpoint.py}"

PYTHON_PATHSEP="$("${PYTHON_BIN}" - <<'PY'
import os
print(os.pathsep)
PY
)"
export PYTHONPATH="${RETICULUM_PY_REPO}${PYTHON_PATHSEP}${LXMF_PY_REPO}${PYTHONPATH:+${PYTHON_PATHSEP}${PYTHONPATH}}"

HOST_BASH="${BASH_BIN:-}"
if [[ -z "${HOST_BASH}" ]]; then
  HOST_BASH="$(command -v bash)"
  if command -v cygpath >/dev/null 2>&1; then
    HOST_BASH="$(cygpath -w "${HOST_BASH}")"
  fi
fi

require_python_modules() {
  "${PYTHON_BIN}" - <<'PY' >/dev/null
import importlib.util
for module in ("RNS", "LXMF"):
    if importlib.util.find_spec(module) is None:
        raise SystemExit(f"missing Python module: {module}")
PY
}

wait_for_file_pattern() {
  local file="$1"
  local pattern="$2"
  local timeout="$3"
  local start
  start="$(date +%s)"
  while true; do
    if [[ -f "${file}" ]] && grep -Eq "${pattern}" "${file}"; then
      return 0
    fi
    if (( "$(date +%s)" - start >= timeout )); then
      return 1
    fi
    sleep 1
  done
}

extract_hash() {
  local file="$1"
  local marker="$2"
  "${PYTHON_BIN}" - <<'PY' "${file}" "${marker}"
import re
import sys
from pathlib import Path

path = Path(sys.argv[1])
marker = sys.argv[2]
pattern = re.compile(r"([0-9a-f]{32})", re.IGNORECASE)

for line in path.read_text(encoding="utf-8", errors="ignore").splitlines():
    if marker in line:
        match = pattern.search(line)
        if match:
            print(match.group(1).lower())
            raise SystemExit(0)

raise SystemExit(1)
PY
}

destination_hash_from_identity() {
  local identity_path="$1"
  local aspect_one="$2"
  local aspect_two="$3"
  local aspect_three="${4:-}"
  "${PYTHON_BIN}" - <<'PY' "${identity_path}" "${aspect_one}" "${aspect_two}" "${aspect_three}"
import os
import sys
import tempfile

import RNS

identity_path, aspect_one, aspect_two, aspect_three = sys.argv[1:5]
cfg = tempfile.mkdtemp(prefix="rns-hash-")
with open(os.path.join(cfg, "config"), "w", encoding="utf-8") as handle:
    handle.write(
        "[reticulum]\n"
        "share_instance = no\n"
        "enable_transport = no\n"
        "discover_interfaces = false\n"
        "autoconnect_discovered_interfaces = 0\n"
    )

RNS.Reticulum(configdir=cfg, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load identity from {identity_path}")

aspects = [aspect_one, aspect_two]
if aspect_three:
    aspects.append(aspect_three)

destination = RNS.Destination(identity, RNS.Destination.IN, RNS.Destination.SINGLE, *aspects)
print(RNS.hexrep(destination.hash, delimit=False).lower())
PY
}

identity_hash_from_file() {
  local identity_path="$1"
  "${PYTHON_BIN}" - <<'PY' "${identity_path}"
import os
import sys
import tempfile

import RNS

identity_path = sys.argv[1]
cfg = tempfile.mkdtemp(prefix="rns-ident-")
with open(os.path.join(cfg, "config"), "w", encoding="utf-8") as handle:
    handle.write(
        "[reticulum]\n"
        "share_instance = no\n"
        "enable_transport = no\n"
        "discover_interfaces = false\n"
        "autoconnect_discovered_interfaces = 0\n"
    )

RNS.Reticulum(configdir=cfg, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load identity from {identity_path}")
print(RNS.hexrep(identity.hash, delimit=False).lower())
PY
}

assert_contains() {
  local file="$1"
  local pattern="$2"
  local description="$3"
  if ! grep -Eq "${pattern}" "${file}"; then
    echo "missing expected output: ${description}" >&2
    echo "looked for pattern '${pattern}' in ${file}" >&2
    return 1
  fi
}

trace_contains_status() {
  local trace_file="$1"
  local status="$2"
  [[ -f "${trace_file}" ]] && grep -Eq "\"status\": *\"${status}\"|${status}" "${trace_file}"
}

trace_lacks_status_prefix() {
  local trace_file="$1"
  local status_prefix="$2"
  ! { [[ -f "${trace_file}" ]] && grep -Eq "\"status\": *\"${status_prefix}|${status_prefix}" "${trace_file}"; }
}

rpc_call() {
  local rpc_addr="$1"
  local method="$2"
  local params_json="${3:-null}"
  "${PYTHON_BIN}" - <<'PY' "${rpc_addr}" "${method}" "${params_json}"
import json
import errno
import socket
import sys
import time

import RNS.vendor.umsgpack as msgpack

rpc_addr, method, params_json = sys.argv[1:4]
params = None if params_json == "null" else json.loads(params_json)
host, port = rpc_addr.split(":", 1)

def is_rate_limited(error):
    if error == "SDK_SECURITY_RATE_LIMITED":
        return True
    if isinstance(error, list) and error:
        return error[0] == "SDK_SECURITY_RATE_LIMITED"
    if isinstance(error, dict):
        return error.get("code") == "SDK_SECURITY_RATE_LIMITED"
    return False

def is_retryable_socket_error(exc):
    if isinstance(exc, (ConnectionRefusedError, TimeoutError, socket.timeout)):
        return True
    return getattr(exc, "errno", None) in {
        errno.ECONNREFUSED,
        errno.ECONNRESET,
        errno.EPIPE,
        errno.ETIMEDOUT,
    }

for attempt in range(60):
    payload = {"id": 1, "method": method, "params": params}
    packed = msgpack.packb(payload)
    frame = len(packed).to_bytes(4, "big") + packed
    request = (
        f"POST /rpc HTTP/1.1\r\n"
        f"Host: {rpc_addr}\r\n"
        f"Content-Length: {len(frame)}\r\n"
        f"Connection: close\r\n\r\n"
    ).encode("utf-8") + frame
    try:
        with socket.create_connection((host, int(port)), timeout=30) as sock:
            sock.sendall(request)
            response = bytearray()
            while True:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                response.extend(chunk)
    except OSError as exc:
        if is_retryable_socket_error(exc) and attempt + 1 < 60:
            time.sleep(1)
            continue
        raise
    header_end = response.find(b"\r\n\r\n")
    if header_end < 0:
        raise SystemExit("missing rpc response body")
    header = response[:header_end].decode("utf-8", errors="replace")
    body = response[header_end + 4 :]
    if not header.startswith("HTTP/1.1 200"):
        decoded_body = body.decode("utf-8", errors="replace").strip()
        raise SystemExit(decoded_body or f"rpc http error: {header!r}")
    if len(body) < 4:
        raise SystemExit(f"rpc response too short: header={header!r} body={body[:200]!r}")
    frame_len = int.from_bytes(body[:4], "big")
    if len(body) < 4 + frame_len:
        raise SystemExit(
            f"rpc response incomplete: header={header!r} frame_len={frame_len} "
            f"body_len={len(body)} body_prefix={body[:200]!r}"
        )
    value = msgpack.unpackb(body[4 : 4 + frame_len])
    if isinstance(value, list):
        result = value[1] if len(value) > 1 else None
        error = value[2] if len(value) > 2 else None
    elif isinstance(value, dict):
        result = value.get("result", value)
        error = value.get("error")
        if error is None and isinstance(result, dict):
            error = result.get("error")
    else:
        result = value
        error = None
    if error and is_rate_limited(error) and attempt + 1 < 60:
        time.sleep(5)
        continue
    if error:
        raise SystemExit(json.dumps(error))
    print(json.dumps(result))
    raise SystemExit(0)

raise SystemExit(f"rpc call {method} exhausted retry budget")
PY
}

capture_rust_message_evidence() {
  local message_id="$1"
  local out_dir="${RUST_EVIDENCE_DIR}/${message_id}"
  mkdir -p "${out_dir}"
  rpc_call "${RUST_RPC_ADDR}" "message_delivery_trace" "{\"message_id\":\"${message_id}\"}" >"${out_dir}/message_delivery_trace.json" || true
  rpc_call "${RUST_RPC_ADDR}" "sdk_status_v2" "{\"message_id\":\"${message_id}\"}" >"${out_dir}/sdk_status_v2.json" || true
  rpc_call "${RUST_RPC_ADDR}" "sdk_snapshot_v2" "{}" >"${out_dir}/sdk_snapshot_v2.json" || true
  rpc_call "${RUST_RPC_ADDR}" "sdk_poll_events_v2" "{\"max\":64}" >"${out_dir}/sdk_poll_events_v2.json" || true
}

wait_rust_trace_status() {
  local message_id="$1"
  local status="$2"
  local timeout="$3"
  local out_dir="${RUST_EVIDENCE_DIR}/${message_id}"
  mkdir -p "${out_dir}"
  local start
  start="$(date +%s)"
  while true; do
    capture_rust_message_evidence "${message_id}"
    if trace_contains_status "${out_dir}/message_delivery_trace.json" "${status}"; then
      return 0
    fi
    if (( "$(date +%s)" - start >= timeout )); then
      return 1
    fi
    sleep 1
  done
}

record_python_stored_message() {
  local messages_dir="$1"
  local content="$2"
  local output_json="$3"
  "${PYTHON_BIN}" - <<'PY' "${messages_dir}" "${content}" "${output_json}"
import json
import sys
from pathlib import Path

import LXMF

messages_dir = Path(sys.argv[1])
content = sys.argv[2]
output_json = Path(sys.argv[3])

for path in sorted(messages_dir.glob("*"), key=lambda item: item.stat().st_mtime, reverse=True):
    if not path.is_file():
        continue
    try:
        with path.open("rb") as handle:
            message = LXMF.LXMessage.unpack_from_file(handle)
    except Exception:
        continue
    if message is None:
        continue
    message_content = message.content_as_string()
    if message_content != content:
        continue
    payload = {
        "message_file": str(path),
        "source": message.source_hash.hex() if message.source_hash else "",
        "destination": message.destination_hash.hex() if message.destination_hash else "",
        "title": message.title_as_string() or "",
        "content_len": len(message_content),
        "content_prefix": message_content[:160],
        "exact_content_match": True,
    }
    output_json.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(str(path))
    raise SystemExit(0)

raise SystemExit(f"no stored Python LXMF message matched content in {messages_dir}")
PY
}

record_python_propagation_payload() {
  local messages_dir="$1"
  local destination_hash_hex="$2"
  local output_json="$3"
  "${PYTHON_BIN}" - <<'PY' "${messages_dir}" "${destination_hash_hex}" "${output_json}"
import json
import sys
from pathlib import Path

import LXMF
import RNS

messages_dir = Path(sys.argv[1])
destination_hash_hex = sys.argv[2].lower()
output_json = Path(sys.argv[3])
stamp_size = LXMF.LXStamper.STAMP_SIZE

for path in sorted(messages_dir.glob("*"), key=lambda item: item.stat().st_mtime, reverse=True):
    if not path.is_file():
        continue
    data = path.read_bytes()
    if len(data) <= stamp_size:
        continue
    payload = data[:-stamp_size]
    if len(payload) < LXMF.LXMessage.DESTINATION_LENGTH:
        continue
    destination = payload[:LXMF.LXMessage.DESTINATION_LENGTH].hex()
    if destination != destination_hash_hex:
        continue
    transient_id = RNS.Identity.full_hash(payload).hex()
    proof = {
        "message_file": str(path),
        "destination": destination,
        "transient_id": transient_id,
        "payload_hex": payload.hex(),
        "payload_bytes": len(payload),
        "stored_bytes": len(data),
    }
    output_json.write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(proof))
    raise SystemExit(0)

raise SystemExit(f"no Python propagation payload for {destination_hash_hex} in {messages_dir}")
PY
}

python_control_call() {
  local control_port="$1"
  local method="$2"
  local params_json="${3:-}"
  if [[ -z "${params_json}" ]]; then
    params_json="{}"
  fi
  "${PYTHON_BIN}" - <<'PY' "${control_port}" "${method}" "${params_json}"
import json
import socket
import sys

control_port = int(sys.argv[1])
method = sys.argv[2]
try:
    params = json.loads(sys.argv[3])
except json.JSONDecodeError as exc:
    raise SystemExit(f"invalid control params argv={sys.argv!r}: {exc}")

socket_timeout = 30.0
if isinstance(params, dict) and "timeout" in params:
    socket_timeout = max(socket_timeout, float(params["timeout"]) + 5.0)

request = json.dumps({"method": method, "params": params}).encode("utf-8") + b"\n"
with socket.create_connection(("127.0.0.1", control_port), timeout=socket_timeout) as sock:
    sock.settimeout(socket_timeout)
    sock.sendall(request)
    sock.shutdown(socket.SHUT_WR)
    response = bytearray()
    while True:
        chunk = sock.recv(65536)
        if not chunk:
            break
        response.extend(chunk)

if not response:
    raise SystemExit("empty response from python endpoint control server")

decoded = response.decode("utf-8", errors="replace")
try:
    payload = json.loads(decoded)
except json.JSONDecodeError as exc:
    raise SystemExit(f"invalid control response: {decoded!r}: {exc}")
if not payload.get("ok"):
    raise SystemExit(payload.get("error") or "python endpoint control call failed")

print(json.dumps(payload.get("result")))
PY
}

wait_for_python_control() {
  local control_port="$1"
  local last_error=""
  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if last_error="$(python_control_call "${control_port}" "status" "null" 2>&1 >/dev/null)"; then
      return 0
    fi
    sleep 1
  done
  if [[ -n "${last_error}" ]]; then
    echo "Python endpoint helper control probe failed on port ${control_port}: ${last_error}" >&2
  else
    echo "Python endpoint helper control probe failed on port ${control_port}" >&2
  fi
  return 1
}

wait_for_rust_peer() {
  local peer_hash="$1"
  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if rpc_call "${RUST_RPC_ADDR}" "list_peers" "null" | grep -Eq "\"peer\": *\"${peer_hash}\""; then
      return 0
    fi
    # Delivery announces are routing evidence, not propagation peers. Keep the
    # propagation-directory check above, but also accept a learned transport
    # path for delivery and link endpoints.
    if rpc_call "${RUST_RPC_ADDR}" "path_status" "{\"destination\":\"${peer_hash}\"}" \
      | grep -Eq '"known": *true'; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_rust_tcp_client_accept() {
  local status_json=""
  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if status_json="$(rpc_call "${RUST_RPC_ADDR}" "daemon_status_ex" "null" 2>/dev/null)"; then
      if "${PYTHON_BIN}" - <<'PY' "${status_json}" >/dev/null 2>&1
import json
import sys

payload = json.loads(sys.argv[1])
for iface in payload.get("interfaces") or []:
    settings = iface.get("settings") or {}
    runtime = settings.get("_runtime") or {}
    tcp = runtime.get("tcp") or {}
    listener = tcp.get("listener_status") or {}
    if int(listener.get("accepted_connections") or 0) > 0:
        raise SystemExit(0)
raise SystemExit(1)
PY
      then
        return 0
      fi
    fi
    sleep 1
  done
  return 1
}

wait_for_python_remote_control() {
  local destination_hash="$1"
  local timeout_secs="${2:-${TIMEOUT_SECS}}"
  "${PYTHON_BIN}" - <<'PY' "${PY_RNS_DIR}" "${destination_hash}" "${timeout_secs}"
import sys
import time

import LXMF
import RNS

rns_config, destination_hash_hex, timeout_secs = sys.argv[1:4]
timeout_secs = max(float(timeout_secs), 1.0)
destination_hash = bytes.fromhex(destination_hash_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
deadline = time.time() + timeout_secs
while time.time() < deadline:
    remote_identity = RNS.Identity.recall(destination_hash)
    if remote_identity is None:
        RNS.Transport.request_path(destination_hash)
    else:
        control_destination = RNS.Destination(
            remote_identity,
            RNS.Destination.OUT,
            RNS.Destination.SINGLE,
            LXMF.APP_NAME,
            "propagation",
            "control",
        )
        if RNS.Transport.has_path(control_destination.hash):
            raise SystemExit(0)
        RNS.Transport.request_path(control_destination.hash)
    time.sleep(0.5)

raise SystemExit(f"timed out waiting for Python remote control path to {destination_hash_hex}")
PY
}

wait_for_python_destination_path() {
  local destination_hash="$1"
  local timeout_secs="${2:-${TIMEOUT_SECS}}"
  "${PYTHON_BIN}" - <<'PY' "${PY_RNS_DIR}" "${destination_hash}" "${timeout_secs}"
import sys
import time

import RNS

rns_config, destination_hash_hex, timeout_secs = sys.argv[1:4]
timeout_secs = max(float(timeout_secs), 1.0)
destination_hash = bytes.fromhex(destination_hash_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
deadline = time.time() + timeout_secs
while time.time() < deadline:
    if RNS.Transport.has_path(destination_hash):
        raise SystemExit(0)
    RNS.Transport.request_path(destination_hash)
    time.sleep(0.5)

raise SystemExit(f"timed out waiting for Python path to {destination_hash_hex}")
PY
}

request_python_destination_path_json() {
  local destination_hash="$1"
  local timeout_secs="${2:-${PATH_DISCOVERY_TIMEOUT_SECS}}"
  local quiet_secs="${3:-${PATH_DISCOVERY_QUIET_SECS}}"
  "${PYTHON_BIN}" - <<'PY' "${PY_RNS_DIR}" "${destination_hash}" "${timeout_secs}" "${quiet_secs}"
import json
import sys
import time

import RNS

rns_config, destination_hash_hex, timeout_secs, quiet_secs = sys.argv[1:5]
timeout_secs = max(float(timeout_secs), 1.0)
quiet_secs = max(float(quiet_secs), 0.0)
destination_hash = bytes.fromhex(destination_hash_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
quiet_started = time.time()
quiet_deadline = quiet_started + quiet_secs
quiet_checks = 0
while time.time() < quiet_deadline:
    if RNS.Transport.has_path(destination_hash):
        print(json.dumps({
            "destination_hash": destination_hash_hex,
            "path_found_before": True,
            "path_found": True,
            "requested": False,
            "identity_recalled": RNS.Identity.recall(destination_hash) is not None,
            "quiet_seconds": round(time.time() - quiet_started, 3),
            "quiet_checks": quiet_checks,
            "elapsed_seconds": 0.0,
        }, sort_keys=True))
        raise SystemExit(0)
    quiet_checks += 1
    time.sleep(0.25)

started = time.time()
before = RNS.Transport.has_path(destination_hash)
requested = False
deadline = started + timeout_secs
while time.time() < deadline:
    if RNS.Transport.has_path(destination_hash):
        print(json.dumps({
            "destination_hash": destination_hash_hex,
            "path_found_before": before,
            "path_found": True,
            "requested": requested,
            "identity_recalled": RNS.Identity.recall(destination_hash) is not None,
            "quiet_seconds": round(started - quiet_started, 3),
            "quiet_checks": quiet_checks,
            "elapsed_seconds": round(time.time() - started, 3),
        }, sort_keys=True))
        raise SystemExit(0)
    RNS.Transport.request_path(destination_hash)
    requested = True
    time.sleep(0.5)

print(json.dumps({
    "destination_hash": destination_hash_hex,
    "path_found_before": before,
    "path_found": False,
    "requested": requested,
    "identity_recalled": RNS.Identity.recall(destination_hash) is not None,
    "quiet_seconds": round(started - quiet_started, 3),
    "quiet_checks": quiet_checks,
    "elapsed_seconds": round(time.time() - started, 3),
}, sort_keys=True))
raise SystemExit(1)
PY
}

start_python_lxmd() {
  local redirect="${1:->}"
  if [[ "${redirect}" == ">>" ]]; then
    "${PYTHON_BIN}" -m LXMF.Utilities.lxmd \
      --config "${PY_DIR}" \
      --rnsconfig "${PY_RNS_DIR}" \
      --propagation-node >>"${PY_LOG}" 2>&1 &
  else
    "${PYTHON_BIN}" -m LXMF.Utilities.lxmd \
      --config "${PY_DIR}" \
      --rnsconfig "${PY_RNS_DIR}" \
      --propagation-node >"${PY_LOG}" 2>&1 &
  fi
  PY_PID=$!

  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if [[ -f "${PY_DIR}/identity" ]] && kill -0 "${PY_PID}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  return 1
}

seed_python_sync_peer() {
  local expected_transient="$1"
  local out_json="$2"
  "${PYTHON_BIN}" - <<'PY' \
    "${PY_RNS_DIR}" \
    "${PY_DIR}/identity" \
    "${PY_DIR}/storage" \
    "${RUST_PROPAGATION_HASH}" \
    "${expected_transient}" \
    "${out_json}" \
    "${PROPAGATION_PEERING_COST}"
import json
import sys
import time
from pathlib import Path

import LXMF
import RNS
import RNS.vendor.umsgpack as msgpack

(
    rns_config,
    identity_path,
    storage_root,
    peer_hash_hex,
    transient_hex,
    out_path,
    peering_cost_text,
) = sys.argv[1:8]
peer_hash = bytes.fromhex(peer_hash_hex)
transient_id = bytes.fromhex(transient_hex)
out_path = Path(out_path)
peering_cost = int(peering_cost_text)

RNS.Reticulum(configdir=rns_config, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load Python lxmd identity from {identity_path}")

router = LXMF.LXMRouter(identity=identity, storagepath=storage_root, autopeer=True, autopeer_maxdepth=6, peering_cost=peering_cost, max_peering_cost=peering_cost)
router.enable_propagation()

if transient_id not in router.propagation_entries:
    raise SystemExit(f"Python propagation entry {transient_hex} was not indexed")

peer_identity = RNS.Identity.recall(peer_hash)
if peer_identity is None:
    RNS.Transport.request_path(peer_hash)
    deadline = time.time() + 8.0
    while time.time() < deadline and peer_identity is None:
        time.sleep(0.5)
        peer_identity = RNS.Identity.recall(peer_hash)
if peer_identity is None:
    raise SystemExit(f"Python could not recall Rust propagation identity {peer_hash_hex}")

router.peer(peer_hash, int(time.time()), 256, 1024 * 1024, 0, 0, peering_cost, {"source": "python-compat-harness"})
peer = router.peers.get(peer_hash)
if peer is None:
    raise SystemExit(f"Python did not create peer row for {peer_hash_hex}")

original_stamp_time = LXMF.LXStamper.time.time
LXMF.LXStamper.time.time = time.perf_counter
try:
    peering_key_ready = peer.generate_peering_key()
finally:
    LXMF.LXStamper.time.time = original_stamp_time

if not peering_key_ready:
    raise SystemExit(f"Python could not generate peering key for {peer_hash_hex}")

peer.add_unhandled_message(transient_id)
peer.next_sync_attempt = 0
peer.sync_backoff = 0
peer.alive = True
peer.last_heard = time.time()

serialised_peers = [item.to_bytes() for item in router.peers.values()]
peers_path = Path(router.storagepath) / "peers"
peers_path.write_bytes(msgpack.packb(serialised_peers))

proof = {
    "peer": peer_hash_hex,
    "transient_id": transient_hex,
    "peers_path": str(peers_path),
    "indexed_entries": len(router.propagation_entries),
    "unhandled_ids": [item.hex() for item in peer.unhandled_messages],
    "peering_key_value": peer.peering_key_value(),
}
out_path.write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
print(json.dumps(proof))
PY
}

mkdir -p "${LOG_DIR}"
TMP_ROOT="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"

RUST_DIR="${TMP_ROOT}/rust-lxmd"
PY_DIR="${TMP_ROOT}/python-lxmd"
PY_RNS_DIR="${TMP_ROOT}/python-rns"
PY_SENDER_DIR="${TMP_ROOT}/python-sender"
PY_SENDER_RNS_DIR="${TMP_ROOT}/python-sender-rns"
HOOK_STATE_DIR="${TMP_ROOT}/hook-state"

RUST_LXMD_LOG="${TMP_ROOT}/rust-lxmd.log"
# Delivery/resource traces are emitted at trace level; enable them for the rust
# node so the resource-transfer assertions below can find them (overridable).
RUST_LOG="${RUST_LOG:-reticulumd=trace,reticulum_rs_transport=trace}"
PY_LOG="${TMP_ROOT}/python-lxmd.log"
PY_REMOTE_STATUS_LOG="${TMP_ROOT}/python-remote-status.log"
RUST_REMOTE_STATUS_LOG="${TMP_ROOT}/rust-remote-status.log"
PY_SEND_LOG="${TMP_ROOT}/python-send.json"
RUST_HOOK_LOG="${HOOK_STATE_DIR}/rust-hook.log"
PY_HOOK_LOG="${HOOK_STATE_DIR}/python-hook.log"
PY_STORED_MESSAGE_JSON="${TMP_ROOT}/python-stored-message.json"
PY_PROPAGATION_PAYLOAD_JSON="${TMP_ROOT}/python-propagation-payload.json"
RUST_EVIDENCE_DIR="${TMP_ROOT}/rust-evidence"

kill_process_tree() {
  local pid="$1"
  local child=""
  while IFS= read -r child; do
    if [[ -n "${child}" ]]; then
      kill_process_tree "${child}"
    fi
  done < <(pgrep -P "${pid}" 2>/dev/null || true)
  kill "${pid}" >/dev/null 2>&1 || true
}

cleanup() {
  local status=$?
  if [[ -n "${PY_ENDPOINT_PID:-}" ]]; then
    kill_process_tree "${PY_ENDPOINT_PID}"
    wait "${PY_ENDPOINT_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${PY_PID:-}" ]]; then
    kill_process_tree "${PY_PID}"
    wait "${PY_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${RUST_PID:-}" ]]; then
    kill_process_tree "${RUST_PID}"
    wait "${RUST_PID}" >/dev/null 2>&1 || true
  fi
  if [[ ${status} -ne 0 ]]; then
    echo "[python-lxmd-rust-lxmd-smoke] failed" >&2
    echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}" >&2
  fi
}
trap cleanup EXIT

require_python_modules

if [[ "${COMPAT_CASE}" == "lxm_interchange" ]]; then
  mkdir -p "${TMP_ROOT}/lxm"
  cargo build -p reticulumd --bin lxm-interchange --quiet

  LXM_PATH="$("${PYTHON_BIN}" - <<'PY' "${TMP_ROOT}/lxm"
import sys
from pathlib import Path

import RNS
import LXMF

out_dir = Path(sys.argv[1])
out_dir.mkdir(parents=True, exist_ok=True)

sender_identity = RNS.Identity()
receiver_identity = RNS.Identity()
sender = RNS.Destination(sender_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")
receiver = RNS.Destination(receiver_identity, RNS.Destination.OUT, RNS.Destination.SINGLE, "lxmf", "delivery")
message = LXMF.LXMessage(
    destination=receiver,
    source=sender,
    title=b"\xfftitle",
    content=b"body\x00\xff",
    fields={"meta": {"kind": "python-storage", "count": 2}},
    desired_method=LXMF.LXMessage.DIRECT,
)
message.timestamp = 1770000000.25
message.pack()
written = message.write_to_directory(str(out_dir))
if written is None:
    raise SystemExit("failed to write Python .lxm container")
print(written)
PY
)"

  DECODED_JSON="$("${REPO_ROOT}/target/debug/lxm-interchange" --file "${LXM_PATH}")"
  "${PYTHON_BIN}" - <<'PY' "${DECODED_JSON}" "${REPORT_PATH}" "${COMPAT_CASE}"
import base64
import json
import sys
from pathlib import Path

decoded = json.loads(sys.argv[1])
report_path = Path(sys.argv[2])
case_id = sys.argv[3]

assert decoded["title_utf8"] is None, decoded
assert decoded["content_utf8"] is None, decoded
assert decoded["title_base64"] == base64.b64encode(b"\xfftitle").decode("ascii"), decoded
assert decoded["content_base64"] == base64.b64encode(b"body\x00\xff").decode("ascii"), decoded
assert decoded["fields"] == {"meta": {"kind": "python-storage", "count": 2}}, decoded
assert abs(decoded["timestamp_f64"] - 1770000000.25) < 1e-9, decoded
assert len(decoded["source"]) == 32, decoded
assert len(decoded["destination"]) == 32, decoded

report_path.write_text(json.dumps({
    "status": "pass",
    "case": case_id,
    "decoded": decoded,
}), encoding="utf-8")
PY
  exit 0
fi

mkdir -p "${RUST_DIR}" "${PY_DIR}" "${PY_RNS_DIR}" "${PY_SENDER_DIR}" "${PY_SENDER_RNS_DIR}" "${HOOK_STATE_DIR}"

PY_CONTROL_IDENTITY_HASH="$("${PYTHON_BIN}" - <<'PY' "${PY_DIR}/identity"
import sys
import RNS

path = sys.argv[1]
identity = RNS.Identity()
identity.to_file(path)
print(RNS.hexrep(identity.hash, delimit=False).lower())
PY
)"

RUST_ON_INBOUND_LINE="on_inbound = ${RUST_DIR}/on_inbound.sh"
if [[ "${COMPAT_CASE}" == *_rust_to_python || "${COMPAT_CASE}" == "rns_path_request_rust_to_python_scoped_refresh" ]]; then
  RUST_ON_INBOUND_LINE="# on_inbound disabled for rust_to_python compatibility lanes"
fi

cat > "${RUST_DIR}/launcher.toml" <<EOF
[lxmd]
rpc = "${RUST_RPC_ADDR}"
transport = "${RUST_TRANSPORT_ADDR}"
propagation_node = true
service = true
EOF

cat > "${RUST_DIR}/config" <<EOF
[propagation]
enable_node = yes
announce_at_start = ${RUST_ANNOUNCE_AT_START}
${RUST_ANNOUNCE_INTERVAL_LINE}
propagation_stamp_cost_target = 0
propagation_stamp_cost_flexibility = 0
autopeer = yes
autopeer_maxdepth = 6
peering_cost = ${PROPAGATION_PEERING_COST}
control_allowed = ${PY_CONTROL_IDENTITY_HASH}

[lxmf]
display_name = Rust Smoke Node
announce_at_start = ${RUST_ANNOUNCE_AT_START}
${RUST_ANNOUNCE_INTERVAL_LINE}
${RUST_ON_INBOUND_LINE}

[logging]
loglevel = 4
EOF

cat > "${RUST_DIR}/on_inbound.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
message_file="${1:-}"
state_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../hook-state && pwd)"
mkdir -p "${state_dir}"
{
  printf 'message_file=%s\n' "${message_file}"
  printf 'source=%s\n' "${LXMD_MESSAGE_SOURCE:-}"
  printf 'destination=%s\n' "${LXMD_MESSAGE_DESTINATION:-}"
  printf 'title=%s\n' "${LXMD_MESSAGE_TITLE:-}"
  printf 'content=%s\n' "${LXMD_MESSAGE_CONTENT:-}"
} >> "${state_dir}/rust-hook.log"
EOF
chmod +x "${RUST_DIR}/on_inbound.sh"

cat > "${PY_DIR}/on_inbound.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
message_file="${1:-}"
state_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../hook-state && pwd)"
mkdir -p "${state_dir}"
python3 - <<'PY' "${message_file}" "${state_dir}/python-hook.log"
import sys
from pathlib import Path

import LXMF

message_path = Path(sys.argv[1])
log_path = Path(sys.argv[2])
with message_path.open("rb") as handle:
    message = LXMF.LXMessage.unpack_from_file(handle)
if message is None:
    raise SystemExit("failed to unpack Python inbound message")
with log_path.open("a", encoding="utf-8") as handle:
    handle.write(f"message_file={message_path}\n")
    handle.write(f"source={message.source_hash.hex() if message.source_hash else ''}\n")
    handle.write(f"destination={message.destination_hash.hex() if message.destination_hash else ''}\n")
    handle.write(f"title={message.title_as_string() or ''}\n")
    handle.write(f"content={message.content_as_string() or ''}\n")
PY
EOF
chmod +x "${PY_DIR}/on_inbound.sh"

RUST_CONTROL_IDENTITY_HASH=""

cat > "${PY_RNS_DIR}/config" <<EOF
[reticulum]
  enable_transport = true
  share_instance = yes
  shared_instance_port = ${PY_SHARED_INSTANCE_PORT}
  instance_control_port = ${PY_INSTANCE_CONTROL_PORT}
  discover_interfaces = false
  autoconnect_discovered_interfaces = 0

[logging]
  loglevel = 4

[interfaces]
  [[Rust LXMD]]
    type = TCPClientInterface
    enabled = yes
    target_host = ${RUST_TRANSPORT_HOST}
    target_port = ${RUST_TRANSPORT_PORT}
EOF

cat > "${PY_SENDER_RNS_DIR}/config" <<EOF
[reticulum]
  enable_transport = true
  share_instance = no
  discover_interfaces = false
  autoconnect_discovered_interfaces = 0

[logging]
  loglevel = 4

[interfaces]
  [[Rust LXMD Sender]]
    type = TCPClientInterface
    enabled = yes
    target_host = ${RUST_TRANSPORT_HOST}
    target_port = ${RUST_TRANSPORT_PORT}
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p lxmf-cli --bin lxmd --quiet

RUST_LOG="${RUST_LOG}" SHELL="${HOST_BASH}" "${REPO_ROOT}/target/debug/lxmd" \
  --config "${RUST_DIR}/launcher.toml" >"${RUST_LXMD_LOG}" 2>&1 &
RUST_PID=$!

if ! wait_for_file_pattern "${RUST_LXMD_LOG}" "listening on http://|delivery destination hash=" "${TIMEOUT_SECS}"; then
  echo "Rust lxmd did not become ready" >&2
  exit 1
fi

RUST_DELIVERY_HASH="$(destination_hash_from_identity "${RUST_DIR}/identity" "lxmf" "delivery")"
RUST_PROPAGATION_HASH="$(destination_hash_from_identity "${RUST_DIR}/identity" "lxmf" "propagation")"
RUST_CONTROL_HASH="$(destination_hash_from_identity "${RUST_DIR}/identity" "lxmf" "propagation" "control")"
RUST_CONTROL_IDENTITY_HASH="$(identity_hash_from_file "${RUST_DIR}/identity")"

run_link_case() {
  local py_endpoint_storage="${PY_DIR}/endpoint-storage"
  local py_status_json=""
  local py_delivery_hash=""
  local smoke_message_marker="smoke-message-${COMPAT_CASE}-$(date +%s)"
  local smoke_message_content="${smoke_message_marker}"
  local active_snapshot=""
  local steady_snapshot=""
  local closed_snapshot=""
  local message_json=""
  local rust_message_id=""
  local keepalive_wait=""

  printf 'link lifecycle case via python endpoint helper\n' >"${PY_REMOTE_STATUS_LOG}"
  printf 'link lifecycle case via python endpoint helper\n' >"${RUST_REMOTE_STATUS_LOG}"
  echo "python endpoint control port=${PY_ENDPOINT_CONTROL_PORT}" >>"${PY_LOG}"

  PYTHONUNBUFFERED=1 "${PYTHON_BIN}" -u "${PY_ENDPOINT_HELPER}" \
    --name "python-link-endpoint" \
    --display-name "Python Link Endpoint" \
    --rnsconfig "${PY_SENDER_RNS_DIR}" \
    --storage "${py_endpoint_storage}" \
    --control-port "${PY_ENDPOINT_CONTROL_PORT}" >"${PY_LOG}" 2>&1 &
  PY_ENDPOINT_PID=$!

  if ! wait_for_python_control "${PY_ENDPOINT_CONTROL_PORT}"; then
    echo "Python endpoint helper did not become ready" >&2
    exit 1
  fi

  py_status_json="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "status" "null")"
  py_delivery_hash="$("${PYTHON_BIN}" - <<'PY' "${py_status_json}"
import json
import sys

print(json.loads(sys.argv[1])["delivery_destination_hash"])
PY
  )"

  case "${COMPAT_CASE}" in
    link_liveness_rust_to_python|link_teardown_rust_to_python)
      python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "announce" "{}" >/dev/null
      rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null
      if ! wait_for_rust_peer "${py_delivery_hash}"; then
        echo "Rust did not learn Python endpoint announce for ${COMPAT_CASE}" >&2
        exit 1
      fi
      rust_message_id="rust-link-${COMPAT_CASE}-$(date +%s)"
      rpc_call "${RUST_RPC_ADDR}" "send_message_v2" "$(cat <<EOF
{"id":"${rust_message_id}","source":"${RUST_DELIVERY_HASH}","destination":"${py_delivery_hash}","title":"","content":"${smoke_message_content}","method":"direct"}
EOF
)" >"${PY_SEND_LOG}"
      message_json="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "wait_message" "$(cat <<EOF
{"content":"${smoke_message_content}","timeout":${TIMEOUT_SECS}}
EOF
)")"
      active_snapshot="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "wait_link_state" "$(cat <<EOF
{"state":"active","timeout":${TIMEOUT_SECS}}
EOF
)")"
      echo "active_snapshot=${active_snapshot}" >>"${PY_LOG}"
      ;;
    link_liveness_python_to_rust|link_teardown_python_to_rust)
      rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null
      active_snapshot="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "open_link" "$(cat <<EOF
{"destination":"${RUST_DELIVERY_HASH}","timeout":${TIMEOUT_SECS}}
EOF
)")"
      echo "active_snapshot=${active_snapshot}" >>"${PY_LOG}"
      ;;
    *)
      echo "unsupported link lifecycle case: ${COMPAT_CASE}" >&2
      exit 2
      ;;
  esac

  case "${COMPAT_CASE}" in
    link_liveness_rust_to_python|link_liveness_python_to_rust)
      keepalive_wait="$("${PYTHON_BIN}" - <<'PY' "${active_snapshot}"
import json
import math
import sys

snapshot = json.loads(sys.argv[1])
print(max(7, int(math.ceil(snapshot["keepalive_seconds"])) + 2))
PY
      )"
      sleep "${keepalive_wait}"
      steady_snapshot="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "link_status" "null")"
      echo "steady_snapshot=${steady_snapshot}" >>"${PY_LOG}"
      "${PYTHON_BIN}" - <<'PY' "${COMPAT_CASE}" "${active_snapshot}" "${steady_snapshot}"
import json
import sys

case_id, active_raw, steady_raw = sys.argv[1:4]
active = json.loads(active_raw)
steady = json.loads(steady_raw)

assert active["status_name"] == "active", active
assert steady["status_name"] == "active", steady
assert steady["established_count"] >= 1, steady
assert steady["closed_count"] == 0, steady
assert steady["rtt_seconds"] is not None and steady["rtt_seconds"] > 0, steady
keepalive = steady["keepalive_seconds"]
assert keepalive is not None and keepalive >= 5, steady
assert steady["no_data_for_seconds"] >= max(4.0, keepalive - 1.0), steady
assert steady["inactive_for_seconds"] < keepalive + 3.0, steady
assert steady["no_inbound_for_seconds"] < keepalive + 3.0, steady
assert steady["no_outbound_for_seconds"] < keepalive + 3.0, steady
if case_id == "link_liveness_rust_to_python":
    assert steady["initiator"] is False, steady
else:
    assert steady["initiator"] is True, steady
PY
      kill_process_tree "${RUST_PID}"
      wait "${RUST_PID}" >/dev/null 2>&1 || true
      unset RUST_PID
      closed_snapshot="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "wait_link_state" "$(cat <<EOF
{"state":"closed","timeout":${TIMEOUT_SECS}}
EOF
)")"
      "${PYTHON_BIN}" - <<'PY' "${closed_snapshot}"
import json
import sys

closed = json.loads(sys.argv[1])
assert closed["status_name"] == "closed", closed
assert closed["closed_count"] >= 1, closed
reason = closed.get("teardown_reason") or {}
assert reason.get("name") == "timeout", closed
PY
      ;;
    link_teardown_python_to_rust)
      python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "teardown_link" "{}" >/dev/null
      closed_snapshot="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "wait_link_state" "$(cat <<EOF
{"state":"closed","timeout":${TIMEOUT_SECS}}
EOF
)")"
      "${PYTHON_BIN}" - <<'PY' "${closed_snapshot}"
import json
import sys

closed = json.loads(sys.argv[1])
assert closed["status_name"] == "closed", closed
reason = closed.get("teardown_reason") or {}
assert reason.get("name") == "initiator_closed", closed
assert closed["initiator"] is True, closed
PY
      if ! wait_for_file_pattern "${RUST_LXMD_LOG}" "link: close" "${TIMEOUT_SECS}"; then
        echo "Rust did not log link close after Python teardown" >&2
        exit 1
      fi
      ;;
    link_teardown_rust_to_python)
      python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "set_keepalive_responses" '{"enabled": false}' >/dev/null
      closed_snapshot="$(python_control_call "${PY_ENDPOINT_CONTROL_PORT}" "wait_link_state" "$(cat <<EOF
{"state":"closed","timeout":${TIMEOUT_SECS}}
EOF
)")"
      "${PYTHON_BIN}" - <<'PY' "${closed_snapshot}"
import json
import sys

closed = json.loads(sys.argv[1])
assert closed["status_name"] == "closed", closed
reason = closed.get("teardown_reason") or {}
assert reason.get("name") == "initiator_closed", closed
assert closed["initiator"] is False, closed
PY
      if ! wait_for_file_pattern "${RUST_LXMD_LOG}" "link: close" "${TIMEOUT_SECS}"; then
        echo "Rust did not log link close after watchdog teardown" >&2
        exit 1
      fi
      ;;
  esac

  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${PY_REMOTE_STATUS_LOG}" \
    "${RUST_REMOTE_STATUS_LOG}" \
    "${RUST_DELIVERY_HASH}" \
    "${py_delivery_hash}" \
    "${COMPAT_CASE}" \
    "${smoke_message_content}" \
    "${active_snapshot}" \
    "${steady_snapshot}" \
    "${closed_snapshot}" \
    "${message_json}"
import json
import sys

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    rust_delivery_hash,
    py_delivery_hash,
    compat_case,
    smoke_message_content,
    active_snapshot,
    steady_snapshot,
    closed_snapshot,
    message_json,
) = sys.argv[1:15]

def decode(value):
    return json.loads(value) if value else None

report = {
    "status": "pass",
    "case": compat_case,
    "proof": {
        "smoke_message_content": smoke_message_content,
        "active_link": decode(active_snapshot),
        "steady_link": decode(steady_snapshot),
        "closed_link": decode(closed_snapshot),
        "message": decode(message_json),
    },
    "hashes": {
        "rust_delivery": rust_delivery_hash,
        "python_delivery": py_delivery_hash,
    },
    "logs": {
        "tmp_root": tmp_root,
        "rust_lxmd": rust_log,
        "python_endpoint": py_log,
        "python_remote_status": py_remote_status_log,
        "rust_remote_status": rust_remote_status_log,
    },
}

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(report, handle, indent=2)
    handle.write("\n")
PY

  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
}

if [[ "${COMPAT_CASE}" == link_* ]]; then
  run_link_case
fi

cat > "${PY_DIR}/config" <<EOF
[propagation]
enable_node = yes
announce_at_start = ${PY_ANNOUNCE_AT_START}
${PY_ANNOUNCE_INTERVAL_LINE}
propagation_stamp_cost_target = 0
propagation_stamp_cost_flexibility = 0
autopeer = yes
autopeer_maxdepth = 6
peering_cost = ${PROPAGATION_PEERING_COST}
control_allowed = ${RUST_CONTROL_IDENTITY_HASH}

[lxmf]
display_name = Python Smoke Node
announce_at_start = ${PY_ANNOUNCE_AT_START}
${PY_ANNOUNCE_INTERVAL_LINE}
on_inbound = ${PY_DIR}/on_inbound.sh

[logging]
loglevel = 4
EOF

if ! start_python_lxmd ">"; then
  echo "Python lxmd did not become ready" >&2
  exit 1
fi

PY_DELIVERY_HASH="$(destination_hash_from_identity "${PY_DIR}/identity" "lxmf" "delivery")"
PY_PROPAGATION_HASH="$(destination_hash_from_identity "${PY_DIR}/identity" "lxmf" "propagation")"

if [[ "${COMPAT_CASE}" == "rns_path_request_python_to_rust" ]]; then
  if ! wait_for_rust_tcp_client_accept; then
    echo "Rust daemon did not accept Python TCP client before Python path request case" >&2
    exit 1
  fi

  PATH_DISCOVERY_START_NS="$(date +%s%N)"
  PYTHON_PATH_REQUEST_RESULT="$(request_python_destination_path_json "${RUST_DELIVERY_HASH}" "${PATH_DISCOVERY_TIMEOUT_SECS}" "${PATH_DISCOVERY_QUIET_SECS}")"
  PATH_DISCOVERY_END_NS="$(date +%s%N)"
  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${RUST_DELIVERY_HASH}" \
    "${PYTHON_PATH_REQUEST_RESULT}" \
    "${COMPAT_CASE}" \
    "${PATH_DISCOVERY_START_NS}" \
    "${PATH_DISCOVERY_END_NS}"
import json
import sys

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    rust_delivery_hash,
    python_path_request_raw,
    compat_case,
    path_discovery_start_ns,
    path_discovery_end_ns,
) = sys.argv[1:10]

python_path_request = json.loads(python_path_request_raw)
assert python_path_request["destination_hash"] == rust_delivery_hash, python_path_request
assert python_path_request["path_found_before"] is False, python_path_request
assert python_path_request["path_found"] is True, python_path_request
assert python_path_request["requested"] is True, python_path_request
assert python_path_request["identity_recalled"] is True, python_path_request

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump({
        "status": "pass",
        "case": compat_case,
        "proof": {
            "python_request_path": python_path_request,
        },
        "performance": {
            "boundary": "cold_path_request_to_route_available",
            "enqueue_to_delivery_ns": int(path_discovery_end_ns) - int(path_discovery_start_ns),
            "payload_size_bytes": 0,
            "startup_included": False,
            "route_warmup_included": False,
        },
        "hashes": {
            "rust_delivery": rust_delivery_hash,
        },
        "logs": {
            "tmp_root": tmp_root,
            "rust_lxmd": rust_log,
            "python_lxmd": py_log,
        },
    }, handle, indent=2)
    handle.write("\n")
PY

  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
fi

if [[ "${COMPAT_CASE}" == "rns_path_request_rust_to_python" || "${COMPAT_CASE}" == "rns_path_request_rust_to_python_scoped_refresh" ]]; then
  cargo build -p rns-tools --bin rnpath-rs --quiet

  if ! wait_for_rust_tcp_client_accept; then
    echo "Rust daemon did not accept Python TCP client before path request case" >&2
    exit 1
  fi

  PATH_STATUS_BEFORE="$(rpc_call "${RUST_RPC_ADDR}" "path_status" "{\"destination\":\"${PY_DELIVERY_HASH}\"}")"
  "${PYTHON_BIN}" - <<'PY' "${PATH_STATUS_BEFORE}" "${PY_DELIVERY_HASH}"
import json
import sys

status = json.loads(sys.argv[1])
destination_hash = sys.argv[2]
assert status["destination_hash"] == destination_hash, status
assert status.get("path_found") is False, status
assert status.get("known") is False, status
assert status.get("status") == "unknown", status
PY

  PATH_DISCOVERY_START_NS="$(date +%s%N)"
  PATH_REQUEST_RESULT="$(rpc_call "${RUST_RPC_ADDR}" "request_path" "{\"destination_hash\":\"${PY_DELIVERY_HASH}\",\"timeout_secs\":${PATH_DISCOVERY_TIMEOUT_SECS}}")"
  PATH_DISCOVERY_END_NS="$(date +%s%N)"
  PATH_STATUS_AFTER="$(rpc_call "${RUST_RPC_ADDR}" "path_status" "{\"destination\":\"${PY_DELIVERY_HASH}\"}")"
  RNPATH_JSON="${TMP_ROOT}/rnpath-rs.json"
  "${REPO_ROOT}/target/debug/rnpath-rs" \
    "${PY_DELIVERY_HASH}" \
    --rpc "${RUST_RPC_ADDR}" \
    --timeout "${PATH_DISCOVERY_TIMEOUT_SECS}" \
    --json >"${RNPATH_JSON}"

  SCOPED_RNPATH_JSON=""
  SCOPED_TAG_HEX="01020304"
  if [[ "${COMPAT_CASE}" == "rns_path_request_rust_to_python_scoped_refresh" ]]; then
    SCOPED_IFACE="$("${PYTHON_BIN}" - <<'PY' "${PATH_STATUS_AFTER}"
import json
import sys

status = json.loads(sys.argv[1])
iface = status.get("interface")
if not isinstance(iface, str):
    raise SystemExit(f"path status did not include a scoped interface: {status!r}")
normalized = iface.strip().strip("/").lower()
if len(normalized) != 32:
    raise SystemExit(f"path status did not include a scoped interface: {status!r}")
int(normalized, 16)
print(normalized)
PY
)"
    SCOPED_RNPATH_JSON="${TMP_ROOT}/rnpath-rs-scoped-refresh.json"
    "${REPO_ROOT}/target/debug/rnpath-rs" \
      "${PY_DELIVERY_HASH}" \
      --rpc "${RUST_RPC_ADDR}" \
      --timeout "${PATH_DISCOVERY_TIMEOUT_SECS}" \
      --on-iface "${SCOPED_IFACE}" \
      --tag-hex "${SCOPED_TAG_HEX}" \
      --json >"${SCOPED_RNPATH_JSON}"
  fi

  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${PY_DELIVERY_HASH}" \
    "${PATH_STATUS_BEFORE}" \
    "${PATH_REQUEST_RESULT}" \
    "${PATH_STATUS_AFTER}" \
    "${RNPATH_JSON}" \
    "${SCOPED_RNPATH_JSON}" \
    "${SCOPED_TAG_HEX}" \
    "${COMPAT_CASE}" \
    "${PATH_DISCOVERY_START_NS}" \
    "${PATH_DISCOVERY_END_NS}"
import json
import sys
from pathlib import Path

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_delivery_hash,
    path_status_before_raw,
    path_request_raw,
    path_status_after_raw,
    rnpath_json,
    scoped_rnpath_json,
    scoped_tag_hex,
    compat_case,
    path_discovery_start_ns,
    path_discovery_end_ns,
) = sys.argv[1:15]

path_status_before = json.loads(path_status_before_raw)
path_request = json.loads(path_request_raw)
path_status_after = json.loads(path_status_after_raw)
rnpath_result = json.loads(Path(rnpath_json).read_text(encoding="utf-8"))

for payload in (path_request, path_status_after, rnpath_result):
    assert payload["destination_hash"] == py_delivery_hash, payload
    assert payload["path_found"] is True, payload
    assert payload["status"] == "found", payload
    assert payload.get("next_hop"), payload
    assert payload.get("interface"), payload
    assert isinstance(payload.get("hops"), int), payload

assert path_request["requested"] is True, path_request
scoped_rnpath_result = None
if compat_case == "rns_path_request_rust_to_python_scoped_refresh":
    assert scoped_rnpath_json, "missing scoped rnpath result path"
    scoped_rnpath_result = json.loads(Path(scoped_rnpath_json).read_text(encoding="utf-8"))
    scoped_iface = path_status_after["interface"].strip().strip("/").lower()
    assert scoped_rnpath_result["destination_hash"] == py_delivery_hash, scoped_rnpath_result
    assert scoped_rnpath_result["path_found"] is True, scoped_rnpath_result
    assert scoped_rnpath_result["status"] == "found", scoped_rnpath_result
    assert scoped_rnpath_result["requested"] is True, scoped_rnpath_result
    assert scoped_rnpath_result["on_iface"] == scoped_iface, scoped_rnpath_result
    assert scoped_rnpath_result["interface_scope"] == scoped_iface, scoped_rnpath_result
    assert scoped_rnpath_result["tag_hex"] == scoped_tag_hex, scoped_rnpath_result
    assert scoped_rnpath_result.get("next_hop"), scoped_rnpath_result
    assert isinstance(scoped_rnpath_result.get("hops"), int), scoped_rnpath_result

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump({
        "status": "pass",
        "case": compat_case,
        "proof": {
            "path_status_before": path_status_before,
            "request_path": path_request,
            "path_status_after": path_status_after,
            "rnpath_json": rnpath_result,
            "scoped_rnpath_dispatch": scoped_rnpath_result,
        },
        "performance": {
            "boundary": "cold_path_request_to_route_available",
            "enqueue_to_delivery_ns": int(path_discovery_end_ns) - int(path_discovery_start_ns),
            "payload_size_bytes": 0,
            "startup_included": False,
            "route_warmup_included": False,
        },
        "hashes": {
            "python_delivery": py_delivery_hash,
        },
        "logs": {
            "tmp_root": tmp_root,
            "rust_lxmd": rust_log,
            "python_lxmd": py_log,
            "rnpath_json": rnpath_json,
            "scoped_rnpath_json": scoped_rnpath_json,
        },
    }, handle, indent=2)
    handle.write("\n")
PY

  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
fi

write_report() {
  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${PY_REMOTE_STATUS_LOG}" \
    "${RUST_REMOTE_STATUS_LOG}" \
    "${RUST_HOOK_LOG}" \
    "${PY_HOOK_LOG}" \
    "${PY_STORED_MESSAGE_JSON}" \
    "${RUST_EVIDENCE_DIR}" \
    "${RUST_DELIVERY_HASH}" \
    "${RUST_PROPAGATION_HASH}" \
    "${PY_DELIVERY_HASH}" \
    "${PY_PROPAGATION_HASH}" \
    "${HOOK_MESSAGE_FILE}" \
    "${SMOKE_MESSAGE_MARKER}" \
    "${COMPAT_CASE}" \
    "${PERFORMANCE_START_NS}" \
    "${PERFORMANCE_END_NS}" \
    "${PERFORMANCE_PAYLOAD_BYTES}"
import json
import sys

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    rust_hook_log,
    py_hook_log,
    py_stored_message_json,
    rust_evidence_dir,
    rust_delivery_hash,
    rust_propagation_hash,
    py_delivery_hash,
    py_propagation_hash,
    hook_message_file,
    smoke_message_content,
    compat_case,
    performance_start_ns,
    performance_end_ns,
    performance_payload_bytes,
) = sys.argv[1:21]

report = {
    "status": "pass",
    "case": compat_case,
    "proof": {
        "python_remote_status_to_rust": rust_propagation_hash,
        "rust_remote_status_to_python": py_propagation_hash,
        "smoke_message_content": smoke_message_content,
        "hook_message_file": hook_message_file,
        "python_stored_message": py_stored_message_json,
        "rust_evidence_dir": rust_evidence_dir,
    },
    "hashes": {
        "rust_delivery": rust_delivery_hash,
        "rust_propagation": rust_propagation_hash,
        "python_delivery": py_delivery_hash,
        "python_propagation": py_propagation_hash,
    },
    "logs": {
        "tmp_root": tmp_root,
        "rust_lxmd": rust_log,
        "python_lxmd": py_log,
        "python_remote_status": py_remote_status_log,
        "rust_remote_status": rust_remote_status_log,
        "rust_hook": rust_hook_log,
        "python_hook": py_hook_log,
        "python_stored_message": py_stored_message_json,
        "rust_evidence_dir": rust_evidence_dir,
    },
}

if performance_start_ns and performance_end_ns:
    start_ns = int(performance_start_ns)
    end_ns = int(performance_end_ns)
    if end_ns <= start_ns:
        raise SystemExit("invalid performance timing boundary")
    report["performance"] = {
        "boundary": "enqueue_to_receiver_evidence",
        "enqueue_to_delivery_ns": end_ns - start_ns,
        "payload_size_bytes": int(performance_payload_bytes),
        "startup_included": False,
        "route_warmup_included": False,
    }

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(report, handle, indent=2)
    handle.write("\n")
PY
}

if [[ "${COMPAT_CASE}" == "propagation_remote_status_bidir" || "${COMPAT_CASE}" == "propagation_remote_fetch_rust_to_python" || "${COMPAT_CASE}" == "propagation_remote_download_rust_to_python" || "${COMPAT_CASE}" == "propagation_remote_sync_rust_to_python" ]]; then
  REMOTE_STATUS_PREFLIGHT=1
fi

if [[ "${REMOTE_STATUS_PREFLIGHT}" == "1" && "${COMPAT_CASE}" != "propagated_python_to_rust" && "${COMPAT_CASE}" != "propagated_rust_to_python" ]]; then
  rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null
  if ! wait_for_python_remote_control "${RUST_PROPAGATION_HASH}" "${REMOTE_CONTROL_PATH_TIMEOUT_SECS}"; then
    echo "Python lxmd did not learn Rust propagation control path" >&2
    exit 1
  fi
  sleep "${REMOTE_CONTROL_SETTLE_SECS}"
  printf 'validated Python path to Rust propagation control destination %s\n' "${RUST_CONTROL_HASH}" >"${PY_REMOTE_STATUS_LOG}"
  if ! wait_for_rust_peer "${PY_PROPAGATION_HASH}"; then
    echo "Rust lxmd did not learn Python propagation announce" >&2
    exit 1
  fi

  RUST_REMOTE_STATUS_OK=0
  for _ in $(seq 1 "${REMOTE_STATUS_ATTEMPTS}"); do
    if SHELL="${HOST_BASH}" "${REPO_ROOT}/target/debug/lxmd" \
        --config "${RUST_DIR}/launcher.toml" \
        --timeout "${REMOTE_STATUS_TIMEOUT_SECS}" \
        --remote "${PY_PROPAGATION_HASH}" \
        --status >"${RUST_REMOTE_STATUS_LOG}" 2>&1; then
      RUST_REMOTE_STATUS_OK=1
      break
    fi
    sleep 1
  done

  if [[ "${RUST_REMOTE_STATUS_OK}" -ne 1 ]]; then
    echo "Rust lxmd could not query Python propagation node status" >&2
    cat "${RUST_REMOTE_STATUS_LOG}" >&2 || true
    exit 1
  fi
  assert_contains "${RUST_REMOTE_STATUS_LOG}" "Remote LXMF Propagation Node status" "Rust remote status against Python node"
else
  printf 'skipped remote-status preflight\n' >"${PY_REMOTE_STATUS_LOG}"
  printf 'skipped remote-status preflight\n' >"${RUST_REMOTE_STATUS_LOG}"
fi

if [[ "${COMPAT_CASE}" == "propagation_remote_status_bidir" ]]; then
  SMOKE_MESSAGE_MARKER="remote-status-${COMPAT_CASE}-$(date +%s)"
  HOOK_MESSAGE_FILE=""
  write_report
  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
fi

if [[ "${COMPAT_CASE}" == "propagation_remote_fetch_rust_to_python" || "${COMPAT_CASE}" == "propagation_remote_download_rust_to_python" ]]; then
  rpc_call "${RUST_RPC_ADDR}" "propagation_enable" "{\"enabled\":true,\"peering_cost\":${PROPAGATION_PEERING_COST}}" >/dev/null
  rpc_call "${RUST_RPC_ADDR}" "set_outbound_propagation_node" "{\"peer\":\"${PY_PROPAGATION_HASH}\"}" >/dev/null
  assert_contains <(
    rpc_call "${RUST_RPC_ADDR}" "get_outbound_propagation_node" "null"
  ) "\"peer\": *\"${PY_PROPAGATION_HASH}\"" "selected Python outbound propagation node"

  SMOKE_MESSAGE_MARKER="remote-lifecycle-${COMPAT_CASE}-$(date +%s)"
  RUST_MESSAGE_ID="rust-remote-lifecycle-${COMPAT_CASE}-$(date +%s)"
  rpc_call "${RUST_RPC_ADDR}" "send_message_v2" "$(cat <<EOF
{"id":"${RUST_MESSAGE_ID}","source":"${RUST_DELIVERY_HASH}","destination":"${RUST_DELIVERY_HASH}","title":"","content":"${SMOKE_MESSAGE_MARKER}","method":"propagated"}
EOF
)" >"${PY_SEND_LOG}"

  if ! wait_rust_trace_status "${RUST_MESSAGE_ID}" "sent: propagated resource" "${TIMEOUT_SECS}"; then
    echo "Rust daemon did not seed Python propagation node for ${COMPAT_CASE}" >&2
    exit 1
  fi

  PY_PROPAGATION_PROOF=""
  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if PY_PROPAGATION_PROOF="$(record_python_propagation_payload "${PY_DIR}/storage/lxmf/messagestore" "${RUST_DELIVERY_HASH}" "${PY_PROPAGATION_PAYLOAD_JSON}" 2>/dev/null)"; then
      break
    fi
    sleep 1
  done
  if [[ -z "${PY_PROPAGATION_PROOF}" ]]; then
    echo "Python propagation node did not store a payload for Rust delivery ${RUST_DELIVERY_HASH}" >&2
    exit 1
  fi

  EXPECTED_TRANSIENT="$("${PYTHON_BIN}" - <<'PY' "${PY_PROPAGATION_PAYLOAD_JSON}"
import json
import sys
from pathlib import Path

print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["transient_id"])
PY
)"
  EXPECTED_PAYLOAD_HEX="$("${PYTHON_BIN}" - <<'PY' "${PY_PROPAGATION_PAYLOAD_JSON}"
import json
import sys
from pathlib import Path

print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["payload_hex"])
PY
)"

  if [[ "${COMPAT_CASE}" == "propagation_remote_fetch_rust_to_python" ]]; then
    REMOTE_RESULT="$(rpc_call "${RUST_RPC_ADDR}" "propagation_remote_fetch" "{\"remote\":\"${PY_PROPAGATION_HASH}\",\"timeout_secs\":${REMOTE_STATUS_TIMEOUT_SECS}}")"
  else
    REMOTE_RESULT="$(rpc_call "${RUST_RPC_ADDR}" "propagation_remote_download" "{\"remote\":\"${PY_PROPAGATION_HASH}\",\"timeout_secs\":${REMOTE_STATUS_TIMEOUT_SECS}}")"
  fi
  LOCAL_FETCH="$(rpc_call "${RUST_RPC_ADDR}" "propagation_fetch" "{\"transient_id\":\"${EXPECTED_TRANSIENT}\"}")"
  PROPAGATION_STATUS="$(rpc_call "${RUST_RPC_ADDR}" "propagation_status" "null")"
  PEERS_AFTER="$(rpc_call "${RUST_RPC_ADDR}" "list_peers" "null")"

  "${PYTHON_BIN}" - <<'PY' \
    "${COMPAT_CASE}" \
    "${REMOTE_RESULT}" \
    "${LOCAL_FETCH}" \
    "${PROPAGATION_STATUS}" \
    "${PEERS_AFTER}" \
    "${PY_PROPAGATION_PAYLOAD_JSON}" \
    "${PY_PROPAGATION_HASH}" \
    "${EXPECTED_TRANSIENT}" \
    "${EXPECTED_PAYLOAD_HEX}"
import json
import sys
from pathlib import Path

(
    compat_case,
    remote_raw,
    local_fetch_raw,
    status_raw,
    peers_raw,
    python_payload_path,
    python_propagation_hash,
    expected_transient,
    expected_payload_hex,
) = sys.argv[1:10]

remote = json.loads(remote_raw)
local_fetch = json.loads(local_fetch_raw)
status = json.loads(status_raw)
peers = json.loads(peers_raw)
python_payload = json.loads(Path(python_payload_path).read_text(encoding="utf-8"))
result = remote.get("result", {})
propagation = remote.get("propagation", status.get("propagation", {}))
status_propagation = status.get("propagation", {})

assert remote.get("remote") == python_propagation_hash, remote
assert propagation.get("state_name") == "completed", remote
assert status_propagation.get("state_name") == "completed", status
assert local_fetch.get("transient_id") == expected_transient, local_fetch
assert local_fetch.get("payload_hex") == expected_payload_hex, local_fetch
assert python_payload["transient_id"] == expected_transient, python_payload
assert python_payload["payload_hex"] == expected_payload_hex, python_payload
assert local_fetch.get("payload_bytes") == python_payload["payload_bytes"], local_fetch

if compat_case == "propagation_remote_fetch_rust_to_python":
    assert result.get("available_count", 0) >= 1, result
    assert result.get("fetched_count", 0) >= 1, result
else:
    assert result.get("available_count", result.get("available", 0)) >= 1, result
    assert result.get("downloaded_count", result.get("downloaded", 0)) >= 1, result

source_row = next(
    (row for row in peers.get("peers", []) if row.get("peer", "").lower() == python_propagation_hash.lower()),
    None,
)
if source_row is not None:
    handled = source_row.get("messages", {}).get("handled_ids", [])
    assert expected_transient in handled, source_row
PY

  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${PY_REMOTE_STATUS_LOG}" \
    "${RUST_REMOTE_STATUS_LOG}" \
    "${RUST_PROPAGATION_HASH}" \
    "${PY_PROPAGATION_HASH}" \
    "${PY_PROPAGATION_PAYLOAD_JSON}" \
    "${REMOTE_RESULT}" \
    "${LOCAL_FETCH}" \
    "${PROPAGATION_STATUS}" \
    "${PEERS_AFTER}" \
    "${COMPAT_CASE}"
import json
import sys
from pathlib import Path

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    rust_propagation_hash,
    py_propagation_hash,
    py_payload_path,
    remote_result,
    local_fetch,
    propagation_status,
    peers_after,
    compat_case,
) = sys.argv[1:15]

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump({
        "status": "pass",
        "case": compat_case,
        "proof": {
            "python_stored_payload": json.loads(Path(py_payload_path).read_text(encoding="utf-8")),
            "remote_result": json.loads(remote_result),
            "local_fetch": json.loads(local_fetch),
            "propagation_status": json.loads(propagation_status),
            "peers_after": json.loads(peers_after),
        },
        "hashes": {
            "rust_propagation": rust_propagation_hash,
            "python_propagation": py_propagation_hash,
        },
        "logs": {
            "tmp_root": tmp_root,
            "rust_lxmd": rust_log,
            "python_lxmd": py_log,
            "python_remote_status": py_remote_status_log,
            "rust_remote_status": rust_remote_status_log,
        },
    }, handle, indent=2)
    handle.write("\n")
PY
  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
fi

if [[ "${COMPAT_CASE}" == "propagation_remote_sync_rust_to_python" ]]; then
  rpc_call "${RUST_RPC_ADDR}" "propagation_enable" "{\"enabled\":true,\"peering_cost\":${PROPAGATION_PEERING_COST}}" >/dev/null
  rpc_call "${RUST_RPC_ADDR}" "set_outbound_propagation_node" "{\"peer\":\"${PY_PROPAGATION_HASH}\"}" >/dev/null

  SMOKE_MESSAGE_MARKER="remote-lifecycle-${COMPAT_CASE}-$(date +%s)"
  RUST_MESSAGE_ID="rust-remote-lifecycle-${COMPAT_CASE}-$(date +%s)"
  rpc_call "${RUST_RPC_ADDR}" "send_message_v2" "$(cat <<EOF
{"id":"${RUST_MESSAGE_ID}","source":"${RUST_DELIVERY_HASH}","destination":"${RUST_DELIVERY_HASH}","title":"","content":"${SMOKE_MESSAGE_MARKER}","method":"propagated"}
EOF
)" >"${PY_SEND_LOG}"

  if ! wait_rust_trace_status "${RUST_MESSAGE_ID}" "sent: propagated resource" "${TIMEOUT_SECS}"; then
    echo "Rust daemon did not seed Python propagation node for ${COMPAT_CASE}" >&2
    exit 1
  fi

  PY_PROPAGATION_PROOF=""
  for _ in $(seq 1 "${TIMEOUT_SECS}"); do
    if PY_PROPAGATION_PROOF="$(record_python_propagation_payload "${PY_DIR}/storage/lxmf/messagestore" "${RUST_DELIVERY_HASH}" "${PY_PROPAGATION_PAYLOAD_JSON}" 2>/dev/null)"; then
      break
    fi
    sleep 1
  done
  if [[ -z "${PY_PROPAGATION_PROOF}" ]]; then
    echo "Python propagation node did not store a payload for Rust delivery ${RUST_DELIVERY_HASH}" >&2
    exit 1
  fi

  EXPECTED_TRANSIENT="$("${PYTHON_BIN}" - <<'PY' "${PY_PROPAGATION_PAYLOAD_JSON}"
import json
import sys
from pathlib import Path

print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["transient_id"])
PY
)"
  EXPECTED_PAYLOAD_HEX="$("${PYTHON_BIN}" - <<'PY' "${PY_PROPAGATION_PAYLOAD_JSON}"
import json
import sys
from pathlib import Path

print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["payload_hex"])
PY
)"

  PY_SYNC_PEER_JSON="${TMP_ROOT}/python-sync-peer.json"
  if [[ -n "${PY_PID:-}" ]]; then
    kill_process_tree "${PY_PID}"
    wait "${PY_PID}" >/dev/null 2>&1 || true
    unset PY_PID
  fi

  if ! seed_python_sync_peer "${EXPECTED_TRANSIENT}" "${PY_SYNC_PEER_JSON}" >/dev/null; then
    echo "Failed to seed Python LXMRouter peer row for Rust propagation peer" >&2
    exit 1
  fi

  if ! start_python_lxmd ">>"; then
    echo "Python lxmd did not become ready after peer seeding restart" >&2
    exit 1
  fi

  rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null
  if ! wait_for_python_remote_control "${RUST_PROPAGATION_HASH}" "${REMOTE_CONTROL_PATH_TIMEOUT_SECS}"; then
    echo "Restarted Python lxmd did not learn Rust propagation control path" >&2
    exit 1
  fi
  if ! wait_for_python_destination_path "${RUST_PROPAGATION_HASH}" "${REMOTE_CONTROL_PATH_TIMEOUT_SECS}"; then
    echo "Restarted Python lxmd did not learn Rust propagation path" >&2
    exit 1
  fi
  if ! wait_for_rust_peer "${PY_PROPAGATION_HASH}"; then
    echo "Rust lxmd did not re-learn Python propagation announce after peer seeding restart" >&2
    exit 1
  fi

  SYNC_STDOUT="${TMP_ROOT}/remote-sync-stdout.json"
  SYNC_STDERR="${TMP_ROOT}/remote-sync-stderr.log"
  rpc_call "${RUST_RPC_ADDR}" "propagation_remote_sync" "{\"remote\":\"${PY_PROPAGATION_HASH}\",\"peer\":\"${RUST_PROPAGATION_HASH}\",\"timeout_secs\":${REMOTE_STATUS_TIMEOUT_SECS}}" >"${SYNC_STDOUT}" 2>"${SYNC_STDERR}"

  LOCAL_FETCH_PATH="${TMP_ROOT}/remote-sync-local-fetch.json"
  LOCAL_FETCH_OK=0
  for _ in $(seq 1 "${REMOTE_STATUS_TIMEOUT_SECS}"); do
    if rpc_call "${RUST_RPC_ADDR}" "propagation_fetch" "{\"transient_id\":\"${EXPECTED_TRANSIENT}\"}" >"${LOCAL_FETCH_PATH}" 2>/dev/null; then
      LOCAL_FETCH_OK=1
      break
    fi
    sleep 1
  done
  if [[ "${LOCAL_FETCH_OK}" -ne 1 ]]; then
    echo "Rust did not import Python sync payload ${EXPECTED_TRANSIENT}" >&2
    exit 1
  fi

  SYNC_RESULT="$(cat "${SYNC_STDOUT}")"
  LOCAL_FETCH="$(cat "${LOCAL_FETCH_PATH}")"
  PROPAGATION_STATUS="$(rpc_call "${RUST_RPC_ADDR}" "propagation_status" "null")"
  PEERS_AFTER="$(rpc_call "${RUST_RPC_ADDR}" "list_peers" "null")"
  "${PYTHON_BIN}" - <<'PY' \
    "${SYNC_RESULT}" \
    "${LOCAL_FETCH}" \
    "${PROPAGATION_STATUS}" \
    "${PEERS_AFTER}" \
    "${PY_PROPAGATION_PAYLOAD_JSON}" \
    "${PY_SYNC_PEER_JSON}" \
    "${RUST_PROPAGATION_HASH}" \
    "${PY_PROPAGATION_HASH}" \
    "${EXPECTED_TRANSIENT}" \
    "${EXPECTED_PAYLOAD_HEX}"
import json
import sys
from pathlib import Path

(sync_raw, local_fetch_raw, status_raw, peers_raw, py_payload_path, py_peer_path, rust_peer, python_peer, expected_transient, expected_payload_hex) = sys.argv[1:11]
sync = json.loads(sync_raw)
local_fetch = json.loads(local_fetch_raw)
status = json.loads(status_raw)
peers = json.loads(peers_raw)
python_payload = json.loads(Path(py_payload_path).read_text(encoding="utf-8"))
python_peer_seed = json.loads(Path(py_peer_path).read_text(encoding="utf-8"))

assert sync.get("propagation", {}).get("state_name") == "completed", sync
assert status.get("propagation", {}).get("state_name") == "completed", status
assert sync.get("remote") == python_peer, sync
sync_result = sync.get("result")
if isinstance(sync_result, dict):
    assert sync_result.get("synced", True) is not False, sync_result
else:
    assert sync_result is True, sync

assert local_fetch.get("transient_id") == expected_transient, local_fetch
assert local_fetch.get("payload_hex") == expected_payload_hex, local_fetch
assert local_fetch.get("payload_bytes") == python_payload["payload_bytes"], local_fetch
assert local_fetch.get("transferred_bytes") == python_payload["payload_bytes"], local_fetch
assert python_payload["transient_id"] == expected_transient, python_payload
assert python_payload["payload_hex"] == expected_payload_hex, python_payload
assert expected_transient in python_peer_seed["unhandled_ids"], python_peer_seed
assert python_peer_seed["peer"] == rust_peer, python_peer_seed
propagation = status.get("propagation", {})
# LinkIdentify retains the verified identity before the resource transfer.
# This sync must remain in the peered lane; an unpeered count is an auth regression.
assert propagation.get("unpeered_propagation_incoming", 0) == 0, propagation
assert propagation.get("unpeered_propagation_rx_bytes", 0) == 0, propagation

source_row = next(
    (row for row in peers.get("peers", []) if row.get("peer", "").lower() == python_peer.lower()),
    None,
)
assert source_row is not None, peers
messages = source_row.get("messages", {})
handled = messages.get("handled_ids", [])
unhandled = messages.get("unhandled_ids", [])
assert expected_transient in handled or expected_transient in unhandled, source_row
assert source_row.get("tx_bytes", 0) >= python_payload["stored_bytes"], source_row
PY

  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${PY_REMOTE_STATUS_LOG}" \
    "${RUST_REMOTE_STATUS_LOG}" \
    "${RUST_PROPAGATION_HASH}" \
    "${PY_PROPAGATION_HASH}" \
    "${PY_PROPAGATION_PAYLOAD_JSON}" \
    "${PY_SYNC_PEER_JSON}" \
    "${SYNC_RESULT}" \
    "${LOCAL_FETCH}" \
    "${SYNC_STDERR}" \
    "${PROPAGATION_STATUS}" \
    "${PEERS_AFTER}" \
    "${COMPAT_CASE}"
import json
import sys
from pathlib import Path

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    rust_propagation_hash,
    py_propagation_hash,
    python_payload_path,
    python_peer_path,
    sync_result,
    local_fetch,
    sync_stderr,
    propagation_status,
    peers_after,
    compat_case,
) = sys.argv[1:17]

stderr_text = Path(sync_stderr).read_text(encoding="utf-8", errors="replace").strip()
with open(report_path, "w", encoding="utf-8") as handle:
    json.dump({
        "status": "pass",
        "case": compat_case,
        "proof": {
            "python_stored_payload": json.loads(Path(python_payload_path).read_text(encoding="utf-8")),
            "python_seeded_peer": json.loads(Path(python_peer_path).read_text(encoding="utf-8")),
            "remote_sync": json.loads(sync_result),
            "local_fetch": json.loads(local_fetch),
            "remote_sync_stderr": stderr_text,
            "propagation_status": json.loads(propagation_status),
            "peers_after": json.loads(peers_after),
        },
        "hashes": {
            "rust_propagation": rust_propagation_hash,
            "python_propagation": py_propagation_hash,
        },
        "logs": {
            "tmp_root": tmp_root,
            "rust_lxmd": rust_log,
            "python_lxmd": py_log,
            "python_remote_status": py_remote_status_log,
            "rust_remote_status": rust_remote_status_log,
        },
    }, handle, indent=2)
    handle.write("\n")
PY
  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
fi

if [[ "${COMPAT_CASE}" == "propagation_get_haves_python_to_rust" ]]; then
  rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null

  PY_GET_IDENTITY="${PY_SENDER_DIR}/get_haves_identity"
  PY_GET_HASHES="$("${PYTHON_BIN}" - <<'PY' "${PY_SENDER_RNS_DIR}" "${PY_GET_IDENTITY}"
import json
import sys
from pathlib import Path

import LXMF
import RNS

rns_config, identity_path = sys.argv[1:3]
Path(identity_path).parent.mkdir(parents=True, exist_ok=True)

RNS.Reticulum(configdir=rns_config, loglevel=0)
identity = RNS.Identity()
identity.to_file(identity_path)
delivery = RNS.Destination(
    identity,
    RNS.Destination.IN,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "delivery",
)
propagation = RNS.Destination(
    identity,
    RNS.Destination.IN,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "propagation",
)
print(json.dumps({
    "delivery": RNS.hexrep(delivery.hash, delimit=False).lower(),
    "propagation": RNS.hexrep(propagation.hash, delimit=False).lower(),
}))
PY
)"
  PY_GET_DELIVERY_HASH="$("${PYTHON_BIN}" - <<'PY' "${PY_GET_HASHES}"
import json
import sys

print(json.loads(sys.argv[1])["delivery"])
PY
)"
  PY_GET_PROPAGATION_HASH="$("${PYTHON_BIN}" - <<'PY' "${PY_GET_HASHES}"
import json
import sys

print(json.loads(sys.argv[1])["propagation"])
PY
)"

  "${PYTHON_BIN}" - <<'PY' "${PY_SENDER_RNS_DIR}" "${PY_GET_IDENTITY}"
import sys
import time

import LXMF
import RNS

rns_config, identity_path = sys.argv[1:3]
RNS.Reticulum(configdir=rns_config, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load identity from {identity_path}")

propagation = RNS.Destination(
    identity,
    RNS.Destination.IN,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "propagation",
)
propagation.announce()
time.sleep(1.0)
PY
  if ! wait_for_rust_peer "${PY_GET_PROPAGATION_HASH}"; then
    echo "Rust lxmd did not admit Python get-haves peer before payload ingest" >&2
    exit 1
  fi

  GET_HAVES_PAYLOAD_HEX="$("${PYTHON_BIN}" - <<'PY' "${PY_GET_DELIVERY_HASH}"
import sys

payload = bytes.fromhex(sys.argv[1]) + b" python-get-haves-payload"
print(payload.hex())
PY
)"
  GET_HAVES_INGEST="$(rpc_call "${RUST_RPC_ADDR}" "propagation_ingest" "$("${PYTHON_BIN}" - <<'PY' "${GET_HAVES_PAYLOAD_HEX}"
import json
import sys

print(json.dumps({"payload_hex": sys.argv[1]}))
PY
)")"
  GET_HAVES_TRANSIENT="$("${PYTHON_BIN}" - <<'PY' "${GET_HAVES_INGEST}"
import json
import sys

print(json.loads(sys.argv[1])["transient_id"])
PY
)"

  PEER_ROW="$(rpc_call "${RUST_RPC_ADDR}" "list_peers" "null")"
  "${PYTHON_BIN}" - <<'PY' "${PEER_ROW}" "${PY_GET_PROPAGATION_HASH}" "${GET_HAVES_TRANSIENT}"
import json
import sys

peers_raw, peer_hash, transient_id = sys.argv[1:4]
rows = json.loads(peers_raw)["peers"]
row = next((row for row in rows if row.get("peer") == peer_hash), None)
assert row is not None, rows
messages = row["messages"]
assert transient_id in messages["unhandled_ids"], row
PY

  "${PYTHON_BIN}" - <<'PY' \
    "${PY_SENDER_RNS_DIR}" \
    "${PY_GET_IDENTITY}" \
    "${RUST_PROPAGATION_HASH}" \
    "${GET_HAVES_TRANSIENT}" \
    "${TIMEOUT_SECS}" >"${PY_SEND_LOG}"
import json
import sys
import time

import LXMF
import RNS

rns_config, identity_path, propagation_hash_hex, transient_hex, timeout_secs = sys.argv[1:6]
timeout_secs = max(float(timeout_secs), 1.0)
propagation_hash = bytes.fromhex(propagation_hash_hex)
transient_id = bytes.fromhex(transient_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
identity = RNS.Identity.from_file(identity_path)
if identity is None:
    raise SystemExit(f"failed to load identity from {identity_path}")

deadline = time.time() + timeout_secs
while time.time() < deadline:
    if RNS.Transport.has_path(propagation_hash):
        break
    RNS.Transport.request_path(propagation_hash)
    time.sleep(0.5)
else:
    raise SystemExit("timed out waiting for Rust propagation path")

deadline = time.time() + max(15.0, timeout_secs / 3.0)
remote_identity = None
while time.time() < deadline:
    remote_identity = RNS.Identity.recall(propagation_hash)
    if remote_identity is not None:
        break
    time.sleep(0.2)
if remote_identity is None:
    raise SystemExit("timed out recalling Rust propagation identity")

destination = RNS.Destination(
    remote_identity,
    RNS.Destination.OUT,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "propagation",
)
link = RNS.Link(destination)
deadline = time.time() + timeout_secs
while time.time() < deadline:
    if link.status == RNS.Link.ACTIVE:
        break
    time.sleep(0.2)
else:
    raise SystemExit("timed out opening Rust propagation link")

link.identify(identity)
receipt = link.request(
    "/get",
    data=[None, [transient_id]],
    response_callback=None,
    failed_callback=None,
)
deadline = time.time() + timeout_secs
while time.time() < deadline:
    if receipt.get_status() == RNS.RequestReceipt.READY:
        break
    if receipt.get_status() == RNS.RequestReceipt.FAILED:
        raise SystemExit("propagation get haves request failed")
    time.sleep(0.2)
else:
    raise SystemExit("timed out waiting for propagation get haves response")

response = receipt.get_response()
link.teardown()
if response is not True:
    raise SystemExit(f"expected haves-only get response True, got {response!r}")

print(json.dumps({
    "case": "propagation_get_haves_python_to_rust",
    "response": response,
    "transient_id": transient_id.hex(),
}))
PY

  if rpc_call "${RUST_RPC_ADDR}" "propagation_fetch" "{\"transient_id\":\"${GET_HAVES_TRANSIENT}\"}" >/dev/null 2>&1; then
    echo "haves-only /get did not purge the declared propagation payload" >&2
    exit 1
  fi

  PEER_ROW="$(rpc_call "${RUST_RPC_ADDR}" "list_peers" "null")"
  "${PYTHON_BIN}" - <<'PY' "${PEER_ROW}" "${PY_GET_PROPAGATION_HASH}" "${GET_HAVES_TRANSIENT}"
import json
import sys

peers_raw, peer_hash, transient_id = sys.argv[1:4]
rows = json.loads(peers_raw)["peers"]
row = next((row for row in rows if row.get("peer") == peer_hash), None)
assert row is not None, rows
messages = row["messages"]
assert transient_id not in messages["unhandled_ids"], row
PY

  GET_HAVES_RESULT="$("${PYTHON_BIN}" - <<'PY' "${PY_SEND_LOG}"
import json
import sys
from pathlib import Path

print(Path(sys.argv[1]).read_text(encoding="utf-8").strip().splitlines()[-1])
PY
)"
  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${PY_REMOTE_STATUS_LOG}" \
    "${RUST_REMOTE_STATUS_LOG}" \
    "${RUST_PROPAGATION_HASH}" \
    "${PY_GET_DELIVERY_HASH}" \
    "${PY_GET_PROPAGATION_HASH}" \
    "${GET_HAVES_RESULT}" \
    "${COMPAT_CASE}"
import json
import sys

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    rust_propagation_hash,
    python_delivery_hash,
    python_propagation_hash,
    get_haves_result,
    compat_case,
) = sys.argv[1:12]

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump({
        "status": "pass",
        "case": compat_case,
        "proof": json.loads(get_haves_result),
        "hashes": {
            "rust_propagation": rust_propagation_hash,
            "python_delivery": python_delivery_hash,
            "python_propagation": python_propagation_hash,
        },
        "logs": {
            "tmp_root": tmp_root,
            "rust_lxmd": rust_log,
            "python_lxmd": py_log,
            "python_remote_status": py_remote_status_log,
            "rust_remote_status": rust_remote_status_log,
        },
    }, handle, indent=2)
    handle.write("\n")
PY
  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
fi

if [[ "${COMPAT_CASE}" == "propagation_offer_python_to_rust" || "${COMPAT_CASE}" == "propagation_offer_queue_python_to_rust" || "${COMPAT_CASE}" == "propagation_offer_duplicate_wanted_source_completed_python_to_rust" ]]; then
  rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null
  if ! wait_for_python_remote_control "${RUST_PROPAGATION_HASH}" "${REMOTE_CONTROL_PATH_TIMEOUT_SECS}"; then
    echo "Python lxmd did not learn Rust propagation control path" >&2
    exit 1
  fi

  rpc_call "${RUST_RPC_ADDR}" "propagation_enable" "{\"enabled\":true,\"peering_cost\":${PROPAGATION_PEERING_COST}}" >/dev/null
  KNOWN_PAYLOAD_HEX="$("${PYTHON_BIN}" - <<'PY' "${RUST_DELIVERY_HASH}"
import sys

destination = bytes.fromhex(sys.argv[1])
payload = destination + b" python-offer-known-payload"
print(payload.hex())
PY
)"
  KNOWN_INGEST="$(rpc_call "${RUST_RPC_ADDR}" "propagation_ingest" "$("${PYTHON_BIN}" - <<'PY' "${KNOWN_PAYLOAD_HEX}"
import json
import sys

print(json.dumps({"payload_hex": sys.argv[1]}))
PY
)")"
  KNOWN_TRANSIENT="$("${PYTHON_BIN}" - <<'PY' "${KNOWN_INGEST}"
import json
import sys

print(json.loads(sys.argv[1])["transient_id"])
PY
)"
  MISSING_TRANSIENT="$(printf 'bc%.0s' $(seq 1 32))"

  "${PYTHON_BIN}" - <<'PY' \
    "${COMPAT_CASE}" \
    "${PY_SENDER_RNS_DIR}" \
    "${PY_SENDER_DIR}" \
    "${RUST_PROPAGATION_HASH}" \
    "${RUST_CONTROL_IDENTITY_HASH}" \
    "${KNOWN_TRANSIENT}" \
    "${MISSING_TRANSIENT}" \
    "${TIMEOUT_SECS}" \
    "${PROPAGATION_PEERING_COST}" >"${PY_SEND_LOG}"
import json
import sys
import time
from pathlib import Path

import RNS
import LXMF

(
    compat_case,
    rns_config,
    storage_dir,
    propagation_hash_hex,
    rust_identity_hash_hex,
    known_hex,
    missing_hex,
    timeout_secs,
    peering_cost_text,
) = sys.argv[1:10]
timeout_secs = max(float(timeout_secs), 1.0)
peering_cost = int(peering_cost_text)
storage = Path(storage_dir)
storage.mkdir(parents=True, exist_ok=True)

propagation_hash = bytes.fromhex(propagation_hash_hex)
rust_identity_hash = bytes.fromhex(rust_identity_hash_hex)
known = bytes.fromhex(known_hex)
missing = bytes.fromhex(missing_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
identity_path = storage / "offer_identity"
identity = RNS.Identity()
identity.to_file(str(identity_path))
source_propagation = RNS.Destination(
    identity,
    RNS.Destination.OUT,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "propagation",
)

deadline = time.time() + timeout_secs
while time.time() < deadline:
    if RNS.Transport.has_path(propagation_hash):
        break
    RNS.Transport.request_path(propagation_hash)
    time.sleep(0.5)
else:
    raise SystemExit("timed out waiting for Rust propagation path")

deadline = time.time() + max(15.0, timeout_secs / 3.0)
remote_identity = None
while time.time() < deadline:
    remote_identity = RNS.Identity.recall(propagation_hash)
    if remote_identity is not None:
        break
    time.sleep(0.2)
if remote_identity is None:
    raise SystemExit("timed out recalling Rust propagation identity")

destination = RNS.Destination(
    remote_identity,
    RNS.Destination.OUT,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "propagation",
)
link = RNS.Link(destination)
deadline = time.time() + timeout_secs
while time.time() < deadline:
    if link.status == RNS.Link.ACTIVE:
        break
    time.sleep(0.2)
else:
    raise SystemExit("timed out opening Rust propagation link")

link.identify(identity)
peering_id = rust_identity_hash + identity.hash
original_stamp_time = LXMF.LXStamper.time.time
LXMF.LXStamper.time.time = time.perf_counter
try:
    peering_key, peering_value = LXMF.LXStamper.generate_stamp(
        peering_id,
        peering_cost,
        expand_rounds=LXMF.LXStamper.WORKBLOCK_EXPAND_ROUNDS_PEERING,
    )
finally:
    LXMF.LXStamper.time.time = original_stamp_time

offered_ids = [known, missing]
if compat_case == "propagation_offer_duplicate_wanted_source_completed_python_to_rust":
    offered_ids.append(missing)
offer = [peering_key, offered_ids]
receipt = link.request("/offer", data=offer, response_callback=None, failed_callback=None)
deadline = time.time() + timeout_secs
while time.time() < deadline:
    if receipt.get_status() == RNS.RequestReceipt.READY:
        break
    if receipt.get_status() == RNS.RequestReceipt.FAILED:
        raise SystemExit("first propagation offer request failed")
    time.sleep(0.2)
else:
    raise SystemExit("timed out waiting for first propagation offer response")
first_response = receipt.get_response()

second_receipt = link.request("/offer", data=offer, response_callback=None, failed_callback=None)
deadline = time.time() + timeout_secs
while time.time() < deadline:
    if second_receipt.get_status() == RNS.RequestReceipt.READY:
        break
    if second_receipt.get_status() == RNS.RequestReceipt.FAILED:
        raise SystemExit("second propagation offer request failed")
    time.sleep(0.2)
else:
    raise SystemExit("timed out waiting for second propagation offer response")
second_response = second_receipt.get_response()
link.teardown()

if first_response != [missing]:
    raise SystemExit(f"expected partial wanted-id list {[missing.hex()]}, got {first_response!r}")
if second_response != 0xF6:
    raise SystemExit(f"expected throttled response 0xF6, got {second_response!r}")

print(json.dumps({
    "case": compat_case,
    "source_propagation": RNS.hexrep(source_propagation.hash, delimit=False).lower(),
    "known_transient": known.hex(),
    "missing_transient": missing.hex(),
    "first_response": [item.hex() if isinstance(item, bytes) else item for item in first_response],
    "second_response": second_response,
    "offered_ids": [item.hex() for item in offered_ids],
    "peering_key_value": peering_value,
}))
PY

  OFFER_RESULT="$("${PYTHON_BIN}" - <<'PY' "${PY_SEND_LOG}"
import json
import sys
from pathlib import Path

print(Path(sys.argv[1]).read_text(encoding="utf-8").strip().splitlines()[-1])
PY
)"
  SOURCE_PROPAGATION_HASH="$("${PYTHON_BIN}" - <<'PY' "${OFFER_RESULT}"
import json
import sys

print(json.loads(sys.argv[1])["source_propagation"])
PY
)"
  if rpc_call "${RUST_RPC_ADDR}" "list_peers" "null" | grep -Eq "\"peer\": *\"${SOURCE_PROPAGATION_HASH}\""; then
    echo "known-offer source peer was admitted before transfer" >&2
    exit 1
  fi
  PEER_SYNC_RESULT="$(rpc_call "${RUST_RPC_ADDR}" "peer_sync" "{\"peer\":\"${SOURCE_PROPAGATION_HASH}\",\"force_sync\":true}")"
  PEER_ROW="$(rpc_call "${RUST_RPC_ADDR}" "list_peers" "null")"
  "${PYTHON_BIN}" - <<'PY' "${PEER_ROW}" "${PEER_SYNC_RESULT}" "${SOURCE_PROPAGATION_HASH}" "${KNOWN_TRANSIENT}" "${MISSING_TRANSIENT}" "${OFFER_RESULT}"
import json
import sys

peers_raw, sync_raw, peer_hash, known, missing, offer_raw = sys.argv[1:7]
rows = json.loads(peers_raw)["peers"]
row = next((row for row in rows if row.get("peer") == peer_hash), None)
assert row is not None, rows
messages = row["messages"]
assert known in messages["handled_ids"], row
assert known not in messages["unhandled_ids"], row
assert missing not in messages["unhandled_ids"], row
assert row["last_sync_attempt"] > 0, row
assert row["sync_backoff"] == 0, row
assert row["next_sync_attempt"] == 0, row
sync = json.loads(sync_raw)
offer = json.loads(offer_raw)
if offer["case"] == "propagation_offer_duplicate_wanted_source_completed_python_to_rust":
    assert offer["offered_ids"].count(missing) == 2, offer
assert sync["synced"] is True, sync
assert sync["propagation"]["synced"] is True, sync
PY

  "${PYTHON_BIN}" - <<'PY' \
    "${REPORT_PATH}" \
    "${TMP_ROOT}" \
    "${RUST_LXMD_LOG}" \
    "${PY_LOG}" \
    "${PY_REMOTE_STATUS_LOG}" \
    "${RUST_REMOTE_STATUS_LOG}" \
    "${RUST_PROPAGATION_HASH}" \
    "${SOURCE_PROPAGATION_HASH}" \
    "${OFFER_RESULT}" \
    "${PEER_SYNC_RESULT}" \
    "${PEER_ROW}" \
    "${COMPAT_CASE}"
import json
import sys

(
    report_path,
    tmp_root,
    rust_log,
    py_log,
    py_remote_status_log,
    rust_remote_status_log,
    rust_propagation_hash,
    source_propagation_hash,
    offer_result,
    peer_sync_result,
    peer_row,
    compat_case,
) = sys.argv[1:13]

with open(report_path, "w", encoding="utf-8") as handle:
    json.dump({
        "status": "pass",
        "case": compat_case,
        "proof": {
            "offer": json.loads(offer_result),
            "peer_sync": json.loads(peer_sync_result),
            "peer_row": json.loads(peer_row),
        },
        "hashes": {
            "rust_propagation": rust_propagation_hash,
            "python_source_propagation": source_propagation_hash,
        },
        "logs": {
            "tmp_root": tmp_root,
            "rust_lxmd": rust_log,
            "python_lxmd": py_log,
            "python_remote_status": py_remote_status_log,
            "rust_remote_status": rust_remote_status_log,
        },
    }, handle, indent=2)
    handle.write("\n")
PY
  echo "[python-lxmd-rust-lxmd-smoke] pass"
  echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
  echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
  exit 0
fi

SMOKE_MESSAGE_MARKER="smoke-message-${COMPAT_CASE}-$(date +%s)"
SMOKE_MESSAGE_CONTENT="${SMOKE_MESSAGE_MARKER}"
if [[ "${COMPAT_CASE}" == "resource_transfer" ]]; then
  SMOKE_MESSAGE_CONTENT="${SMOKE_MESSAGE_MARKER}:$(printf 'x%.0s' $(seq 1 16384))"
fi
if (( PERFORMANCE_PAYLOAD_BYTES > 0 )); then
  SMOKE_MESSAGE_CONTENT="${SMOKE_MESSAGE_MARKER}:$(printf 'x%.0s' $(seq 1 "${PERFORMANCE_PAYLOAD_BYTES}"))"
fi
HOOK_MESSAGE_FILE=""

if [[ "${COMPAT_CASE}" == *_python_to_rust ]]; then
  "${PYTHON_BIN}" - <<'PY' \
  "${COMPAT_CASE}" \
  "${PY_SENDER_RNS_DIR}" \
  "${PY_SENDER_DIR}" \
  "${RUST_DELIVERY_HASH}" \
  "${RUST_PROPAGATION_HASH}" \
  "${TIMEOUT_SECS}" \
  "${SMOKE_MESSAGE_CONTENT}" >"${PY_SEND_LOG}"
import json
import os
import sys
import time

import RNS
import LXMF

case_id, rns_config, storage_dir, destination_hash_hex, propagation_hash_hex, timeout_secs, content = sys.argv[1:8]
timeout_secs = max(float(timeout_secs), 1.0)
destination_hash = bytes.fromhex(destination_hash_hex)
propagation_hash = bytes.fromhex(propagation_hash_hex)

RNS.Reticulum(configdir=rns_config, loglevel=0)
identity = RNS.Identity()
router = LXMF.LXMRouter(identity=identity, storagepath=storage_dir)
source = router.register_delivery_identity(identity, display_name="Python Smoke Sender")

deadline = time.time() + timeout_secs
remote_identity = None
desired_method = LXMF.LXMessage.OPPORTUNISTIC
if case_id in ("direct_python_to_rust", "opportunistic_python_to_rust"):
    while time.time() < deadline:
        if RNS.Transport.has_path(destination_hash):
            break
        RNS.Transport.request_path(destination_hash)
        time.sleep(0.5)
    else:
        raise SystemExit("timed out waiting for Rust delivery path")

    deadline = time.time() + 15
    while time.time() < deadline:
        remote_identity = RNS.Identity.recall(destination_hash)
        if remote_identity is not None:
            break
        time.sleep(0.2)

    if remote_identity is None:
        raise SystemExit("timed out recalling Rust delivery identity")

    if case_id == "direct_python_to_rust":
        desired_method = LXMF.LXMessage.DIRECT
elif case_id == "propagated_python_to_rust":
    desired_method = LXMF.LXMessage.PROPAGATED
    deadline = time.time() + timeout_secs
    while time.time() < deadline:
        if RNS.Transport.has_path(propagation_hash):
            break
        RNS.Transport.request_path(propagation_hash)
        time.sleep(0.5)
    else:
        raise SystemExit("timed out waiting for Rust propagation path")

    deadline = time.time() + max(15.0, timeout_secs / 3.0)
    while time.time() < deadline:
        remote_identity = RNS.Identity.recall(propagation_hash)
        if remote_identity is not None:
            break
        time.sleep(0.2)

    if remote_identity is None:
        raise SystemExit("timed out recalling Rust identity from propagation path")

    router.set_outbound_propagation_node(propagation_hash)
elif case_id != "opportunistic_python_to_rust":
    raise SystemExit(f"unsupported smoke case: {case_id}")

if remote_identity is None:
    while time.time() < deadline:
        remote_identity = RNS.Identity.recall(destination_hash)
        if remote_identity is not None:
            break
        time.sleep(0.2)

    if remote_identity is None:
        raise SystemExit("timed out recalling Rust delivery identity")

destination = RNS.Destination(
    remote_identity,
    RNS.Destination.OUT,
    RNS.Destination.SINGLE,
    LXMF.APP_NAME,
    "delivery",
)

message = LXMF.LXMessage(
    destination,
    source,
    content=content,
    desired_method=desired_method,
)
started_ns = time.time_ns()
router.handle_outbound(message)

deadline = time.time() + timeout_secs
while time.time() < deadline:
    if message.state in (LXMF.LXMessage.DELIVERED, LXMF.LXMessage.SENT):
        print(
            json.dumps(
                {
                    "state": int(message.state),
                    "case": case_id,
                    "destination": destination_hash_hex,
                    "source": RNS.hexrep(source.hash, delimit=False).lower(),
                    "started_unix_ns": started_ns,
                }
            )
        )
        raise SystemExit(0)
    time.sleep(0.2)

raise SystemExit(f"timed out waiting for Python message delivery, state={message.state}")
PY

  PY_SENDER_SOURCE_HASH="$("${PYTHON_BIN}" - <<'PY' "${PY_SEND_LOG}"
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(payload["source"])
PY
  )"
  PERFORMANCE_START_NS="$("${PYTHON_BIN}" - <<'PY' "${PY_SEND_LOG}"
import json
import sys
from pathlib import Path
print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["started_unix_ns"])
PY
  )"

  for _ in $(seq 1 "${PERFORMANCE_POLL_ATTEMPTS}"); do
    if [[ -f "${RUST_HOOK_LOG}" ]] && grep -q "${SMOKE_MESSAGE_CONTENT}" "${RUST_HOOK_LOG}"; then
      break
    fi
    sleep "${PERFORMANCE_POLL_SECS}"
  done

  assert_contains "${RUST_HOOK_LOG}" "${SMOKE_MESSAGE_CONTENT}" "Rust lxmd on-inbound hook content"
  assert_contains "${RUST_HOOK_LOG}" "${PY_SENDER_SOURCE_HASH}" "Rust lxmd on-inbound hook source hash"
  PERFORMANCE_END_NS="$(date +%s%N)"

  HOOK_MESSAGE_FILE="$("${PYTHON_BIN}" - <<'PY' "${RUST_HOOK_LOG}"
import sys
from pathlib import Path

for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    if line.startswith("message_file="):
        print(line.split("=", 1)[1])
        raise SystemExit(0)
raise SystemExit(1)
PY
  )"

  if [[ ! -s "${HOOK_MESSAGE_FILE}" ]]; then
    echo "expected inbound message file at ${HOOK_MESSAGE_FILE}" >&2
    exit 1
  fi
else
  case "${COMPAT_CASE}" in
    direct_rust_to_python)
      RUST_SEND_METHOD="direct"
      ;;
    opportunistic_rust_to_python)
      RUST_SEND_METHOD="opportunistic"
      ;;
    propagated_rust_to_python)
      RUST_SEND_METHOD="propagated"
      rpc_call "${RUST_RPC_ADDR}" "set_outbound_propagation_node" "{\"peer\":\"${PY_PROPAGATION_HASH}\"}" >/dev/null
      assert_contains <(
        rpc_call "${RUST_RPC_ADDR}" "get_outbound_propagation_node" "null"
      ) "\"peer\": *\"${PY_PROPAGATION_HASH}\"" "selected outbound propagation node"
      ;;
    resource_transfer)
      RUST_SEND_METHOD="direct"
      ;;
    *)
      echo "unsupported compatibility case: ${COMPAT_CASE}" >&2
      exit 2
      ;;
  esac

  RUST_MESSAGE_ID="rust-smoke-${COMPAT_CASE}-$(date +%s)"
  rpc_call "${RUST_RPC_ADDR}" "announce_now" "null" >/dev/null
  if [[ "${COMPAT_CASE}" == "direct_rust_to_python" || "${COMPAT_CASE}" == "opportunistic_rust_to_python" || "${COMPAT_CASE}" == "resource_transfer" ]]; then
    if ! wait_for_rust_peer "${PY_DELIVERY_HASH}"; then
      echo "Rust did not learn Python delivery announce for ${COMPAT_CASE}" >&2
      exit 1
    fi
  fi

  PERFORMANCE_START_NS="$(date +%s%N)"
  rpc_call "${RUST_RPC_ADDR}" "send_message_v2" "$(cat <<EOF
{"id":"${RUST_MESSAGE_ID}","source":"${RUST_DELIVERY_HASH}","destination":"${PY_DELIVERY_HASH}","title":"","content":"${SMOKE_MESSAGE_CONTENT}","method":"${RUST_SEND_METHOD}"}
EOF
)" >"${PY_SEND_LOG}"

  capture_rust_message_evidence "${RUST_MESSAGE_ID}"
  for _ in $(seq 1 "${PERFORMANCE_POLL_ATTEMPTS}"); do
    if [[ -f "${PY_HOOK_LOG}" ]] && grep -q "${SMOKE_MESSAGE_MARKER}" "${PY_HOOK_LOG}"; then
      break
    fi
    if record_python_stored_message "${PY_DIR}/storage/messages" "${SMOKE_MESSAGE_CONTENT}" "${PY_STORED_MESSAGE_JSON}" >/dev/null 2>&1; then
      break
    fi
    sleep "${PERFORMANCE_POLL_SECS}"
  done

  if [[ -f "${PY_HOOK_LOG}" ]] && grep -q "${SMOKE_MESSAGE_MARKER}" "${PY_HOOK_LOG}"; then
    assert_contains "${PY_HOOK_LOG}" "${SMOKE_MESSAGE_MARKER}" "Python lxmd on-inbound hook content"
    assert_contains "${PY_HOOK_LOG}" "${PY_DELIVERY_HASH}" "Python lxmd on-inbound hook destination hash"
  else
    HOOK_MESSAGE_FILE="$(record_python_stored_message "${PY_DIR}/storage/messages" "${SMOKE_MESSAGE_CONTENT}" "${PY_STORED_MESSAGE_JSON}")"
    "${PYTHON_BIN}" - <<'PY' "${PY_STORED_MESSAGE_JSON}" "${PY_HOOK_LOG}"
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
with Path(sys.argv[2]).open("a", encoding="utf-8") as handle:
    handle.write(f"message_file={payload['message_file']}\n")
    handle.write(f"source={payload['source']}\n")
    handle.write(f"destination={payload['destination']}\n")
    handle.write(f"title={payload['title']}\n")
    handle.write(f"content_len={payload['content_len']}\n")
    handle.write(f"content_prefix={payload['content_prefix']}\n")
PY
    assert_contains "${PY_STORED_MESSAGE_JSON}" "\"exact_content_match\": *true" "Python stored LXMF exact content"
    assert_contains "${PY_STORED_MESSAGE_JSON}" "\"destination\": *\"${PY_DELIVERY_HASH}\"" "Python stored LXMF destination hash"
  fi
  PERFORMANCE_END_NS="$(date +%s%N)"
  if [[ -z "${HOOK_MESSAGE_FILE}" && -f "${PY_STORED_MESSAGE_JSON}" ]]; then
    HOOK_MESSAGE_FILE="$("${PYTHON_BIN}" - <<'PY' "${PY_STORED_MESSAGE_JSON}"
import json
import sys
from pathlib import Path
print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["message_file"])
PY
    )"
  fi
  capture_rust_message_evidence "${RUST_MESSAGE_ID}"
  if [[ "${COMPAT_CASE}" == "direct_rust_to_python" ]]; then
    if ! wait_rust_trace_status "${RUST_MESSAGE_ID}" "delivered" "${TIMEOUT_SECS}"; then
      echo "Rust daemon did not record delivered receipt for ${COMPAT_CASE}" >&2
      exit 1
    fi
  fi
  if [[ "${COMPAT_CASE}" == "resource_transfer" ]]; then
    if ! wait_rust_trace_status "${RUST_MESSAGE_ID}" "sent: link resource" "${TIMEOUT_SECS}"; then
      echo "Rust daemon did not record sent: link resource for resource transfer" >&2
      exit 1
    fi
    if ! trace_lacks_status_prefix "${RUST_EVIDENCE_DIR}/${RUST_MESSAGE_ID}/message_delivery_trace.json" "failed:"; then
      echo "Rust daemon recorded resource failure despite Python stored-message evidence" >&2
      exit 1
    fi
    assert_contains \
      "${RUST_EVIDENCE_DIR}/${RUST_MESSAGE_ID}/message_delivery_trace.json" \
      '"status": *"(sending|sent): link resource"' \
      "Rust resource transfer trace"
  elif [[ "${COMPAT_CASE}" == "propagated_rust_to_python" ]]; then
    if ! wait_rust_trace_status "${RUST_MESSAGE_ID}" "sent: propagated resource" "${TIMEOUT_SECS}"; then
      echo "Rust daemon did not record sent: propagated resource for propagated transfer" >&2
      exit 1
    fi
    if ! trace_lacks_status_prefix "${RUST_EVIDENCE_DIR}/${RUST_MESSAGE_ID}/message_delivery_trace.json" "failed:"; then
      echo "Rust daemon recorded propagated resource failure despite Python evidence" >&2
      exit 1
    fi
    if [[ "${PERFORMANCE_MODE}" != "1" ]]; then
      assert_contains "${RUST_LXMD_LOG}" "resource_hash=|sending: propagated resource|sent: propagated resource" "Rust propagated resource trace"
    fi
  fi

  if [[ -z "${HOOK_MESSAGE_FILE}" ]]; then
    HOOK_MESSAGE_FILE="$("${PYTHON_BIN}" - <<'PY' "${PY_HOOK_LOG}"
import sys
from pathlib import Path

for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    if line.startswith("message_file="):
        print(line.split("=", 1)[1])
        raise SystemExit(0)
raise SystemExit(1)
PY
    )"
  fi

  if [[ ! -s "${HOOK_MESSAGE_FILE}" ]]; then
    echo "expected inbound message file at ${HOOK_MESSAGE_FILE}" >&2
    exit 1
  fi

fi

write_report

echo "[python-lxmd-rust-lxmd-smoke] pass"
echo "[python-lxmd-rust-lxmd-smoke] report=${REPORT_PATH}"
echo "[python-lxmd-rust-lxmd-smoke] logs=${TMP_ROOT}"
