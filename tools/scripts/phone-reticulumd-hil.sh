#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

# Preflight command used by this harness: adb devices -l
# Reverse command used by this harness: adb reverse tcp:<phone-port> tcp:37429

usage() {
  cat >&2 <<'EOF'
Usage: tools/scripts/phone-reticulumd-hil.sh [options]

Runs the two-phone reticulumd field harness with an S8/Sideband phone and a
Pixel/Columba phone connected through ADB reverse.

Options:
  --preflight-only           Check device visibility and write a report only
  --no-build                 Skip cargo build steps
  --interactive             Prompt for manual phone-visible evidence
  --s8-serial SERIAL         Override S8/Sideband device serial
  --pixel-serial SERIAL      Override Pixel/Columba device serial
  --sideband-hash HASH       Sideband LXMF destination hash
  --columba-hash HASH        Columba LXMF destination hash
  --help                     Show this help

Environment:
  ADB, PYTHON_BIN
  SIDE_BAND_SERIAL or S8_SERIAL
  PIXEL_SERIAL or COLUMBA_SERIAL
  SIDE_BAND_HASH or SIDEBAND_HASH
  COLUMBA_HASH
  RPC_ADDR                   default 127.0.0.1:4243
  TRANSPORT_PORT             default 37429
  SIDE_BAND_REVERSE_PORT     default TRANSPORT_PORT
  PIXEL_REVERSE_PORT         default TRANSPORT_PORT
  AUTO_DISCOVER_PHONE_HASHES default 0
  RUN_ID                     default UTC timestamp
  LOG_DIR                    default target/phone-hil/<timestamp>
  BURST_COUNT                default 25
  PER_PEER_IN_FLIGHT         default 1
  LARGE_BYTES                default 4096
  PHONE_PEER_WAIT_SECS       default 180
  MANUAL_WAIT_SECS           default 120
  PHONE_HIL_IDENTITY_PATH    default target/phone-hil/reticulumd.identity
  KEEP_DAEMON                default 0
EOF
}

ADB="${ADB:-adb}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
RPC_ADDR="${RPC_ADDR:-127.0.0.1:4243}"
TRANSPORT_PORT="${TRANSPORT_PORT:-37429}"
TRANSPORT_ADDR="${TRANSPORT_ADDR:-127.0.0.1:${TRANSPORT_PORT}}"
SIDE_BAND_REVERSE_PORT="${SIDE_BAND_REVERSE_PORT:-${SIDE_BAND_DEVICE_PORT:-${TRANSPORT_PORT}}}"
PIXEL_REVERSE_PORT="${PIXEL_REVERSE_PORT:-${PIXEL_DEVICE_PORT:-${TRANSPORT_PORT}}}"
AUTO_DISCOVER_PHONE_HASHES="${AUTO_DISCOVER_PHONE_HASHES:-0}"
RUN_ID="${RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
ARTIFACT_ROOT="${LOG_DIR:-${REPO_ROOT}/target/phone-hil/${RUN_ID}}"
REPORT_PATH="${REPORT_PATH:-${ARTIFACT_ROOT}/report.json}"
RESULTS_JSONL="${ARTIFACT_ROOT}/results.jsonl"
DB_PATH="${ARTIFACT_ROOT}/reticulum.db"
IDENTITY_PATH="${PHONE_HIL_IDENTITY_PATH:-${REPO_ROOT}/target/phone-hil/reticulumd.identity}"
RETICULUMD_LOG="${ARTIFACT_ROOT}/reticulumd.log"
CONFIG_PATH="${ARTIFACT_ROOT}/reticulumd-phone-hil.toml"
ADB_DEVICES_LOG="${ARTIFACT_ROOT}/adb-devices.txt"
COMMAND_LOG="${ARTIFACT_ROOT}/commands.log"
SIDE_BAND_LOGCAT="${ARTIFACT_ROOT}/s8-sideband.logcat"
COLUMBA_LOGCAT="${ARTIFACT_ROOT}/pixel-columba.logcat"
S8_MODEL_PATTERN="${S8_MODEL_PATTERN:-SM_G950|dreamqlte|S8}"
PIXEL_MODEL_PATTERN="${PIXEL_MODEL_PATTERN:-Pixel|pixel|panther|cheetah|oriole|raven|bluejay|shiba|husky|tokay|caiman|komodo}"
SIDE_BAND_SERIAL="${SIDE_BAND_SERIAL:-${S8_SERIAL:-}}"
PIXEL_DEVICE_SERIAL="${PIXEL_SERIAL:-${COLUMBA_SERIAL:-}}"
SIDE_BAND_HASH="${SIDE_BAND_HASH:-${SIDEBAND_HASH:-}}"
COLUMBA_HASH="${COLUMBA_HASH:-}"
BURST_COUNT="${BURST_COUNT:-25}"
PER_PEER_IN_FLIGHT="${PER_PEER_IN_FLIGHT:-1}"
LARGE_BYTES="${LARGE_BYTES:-4096}"
PHONE_PEER_WAIT_SECS="${PHONE_PEER_WAIT_SECS:-180}"
MANUAL_WAIT_SECS="${MANUAL_WAIT_SECS:-120}"
KEEP_DAEMON="${KEEP_DAEMON:-0}"
PRE_FLIGHT_ONLY=0
BUILD=1
INTERACTIVE="${INTERACTIVE:-0}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --preflight-only)
      PRE_FLIGHT_ONLY=1
      shift
      ;;
    --no-build)
      BUILD=0
      shift
      ;;
    --interactive)
      INTERACTIVE=1
      shift
      ;;
    --s8-serial)
      SIDE_BAND_SERIAL="${2:-}"
      shift 2
      ;;
    --pixel-serial)
      PIXEL_DEVICE_SERIAL="${2:-}"
      shift 2
      ;;
    --sideband-hash)
      SIDE_BAND_HASH="${2:-}"
      shift 2
      ;;
    --columba-hash)
      COLUMBA_HASH="${2:-}"
      shift 2
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      usage
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

mkdir -p "${ARTIFACT_ROOT}"
: >"${RESULTS_JSONL}"
: >"${COMMAND_LOG}"

RETICULUMD_PID=""
SIDE_BAND_LOGCAT_PID=""
COLUMBA_LOGCAT_PID=""
DAEMON_HASH=""
DAEMON_PROPAGATION_HASH=""

cleanup() {
  if [[ -n "${SIDE_BAND_LOGCAT_PID}" ]]; then
    kill "${SIDE_BAND_LOGCAT_PID}" >/dev/null 2>&1 || true
    wait "${SIDE_BAND_LOGCAT_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${COLUMBA_LOGCAT_PID}" ]]; then
    kill "${COLUMBA_LOGCAT_PID}" >/dev/null 2>&1 || true
    wait "${COLUMBA_LOGCAT_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${RETICULUMD_PID}" && "${KEEP_DAEMON}" != "1" ]]; then
    kill "${RETICULUMD_PID}" >/dev/null 2>&1 || true
    wait "${RETICULUMD_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

log_cmd() {
  printf '[phone-hil] %s\n' "$*" | tee -a "${COMMAND_LOG}" >&2
}

record_result() {
  local test_id="$1"
  local status="$2"
  local reason="$3"
  shift 3
  "${PYTHON_BIN}" - "${RESULTS_JSONL}" "${test_id}" "${status}" "${reason}" "$@" <<'PY'
import json
import sys

path, test_id, status, reason, *artifacts = sys.argv[1:]
artifact_map = {}
for index, item in enumerate(artifacts):
    if "=" in item:
        key, value = item.split("=", 1)
    else:
        key, value = f"artifact_{index}", item
    artifact_map[key] = value

with open(path, "a", encoding="utf-8") as handle:
    handle.write(json.dumps({
        "test_id": test_id,
        "status": status,
        "reason": reason,
        "artifacts": artifact_map,
    }, sort_keys=True) + "\n")
PY
}

write_report() {
  local status="$1"
  local reason="$2"
  "${PYTHON_BIN}" - \
    "${REPORT_PATH}" \
    "${RESULTS_JSONL}" \
    "${status}" \
    "${reason}" \
    "${RUN_ID}" \
    "${ARTIFACT_ROOT}" \
    "${RPC_ADDR}" \
    "${TRANSPORT_ADDR}" \
    "${SIDE_BAND_SERIAL}" \
    "${PIXEL_DEVICE_SERIAL}" \
    "${DAEMON_HASH}" \
    "${DAEMON_PROPAGATION_HASH}" \
    "${SIDE_BAND_HASH}" \
    "${COLUMBA_HASH}" \
    "${ADB_DEVICES_LOG}" \
    "${RETICULUMD_LOG}" \
    "${IDENTITY_PATH}" \
    "${SIDE_BAND_LOGCAT}" \
    "${COLUMBA_LOGCAT}" <<'PY'
import json
import os
import sys

(
    report_path,
    results_path,
    status,
    reason,
    run_id,
    artifact_root,
    rpc_addr,
    transport_addr,
    sideband_serial,
    columba_serial,
    daemon_hash,
    daemon_propagation_hash,
    sideband_hash,
    columba_hash,
    adb_devices_log,
    reticulumd_log,
    identity_path,
    sideband_logcat,
    columba_logcat,
) = sys.argv[1:20]

tests = []
if os.path.exists(results_path):
    with open(results_path, "r", encoding="utf-8") as handle:
        tests = [json.loads(line) for line in handle if line.strip()]

payload = {
    "status": status,
    "reason": reason,
    "run_id": run_id,
    "artifact_root": artifact_root,
    "rpc_addr": rpc_addr,
    "transport_addr": transport_addr,
    "devices": {
        "s8_sideband": {
            "serial": sideband_serial or None,
            "lxmf_hash": sideband_hash or None,
            "logcat": sideband_logcat,
        },
        "pixel_columba": {
            "serial": columba_serial or None,
            "lxmf_hash": columba_hash or None,
            "logcat": columba_logcat,
        },
    },
    "daemon": {
        "delivery_hash": daemon_hash or None,
        "propagation_hash": daemon_propagation_hash or None,
        "log": reticulumd_log,
        "identity": identity_path,
    },
    "artifacts": {
        "adb_devices": adb_devices_log,
        "results_jsonl": results_path,
        "report": report_path,
    },
    "tests": tests,
}

os.makedirs(os.path.dirname(report_path), exist_ok=True)
with open(report_path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle, indent=2, sort_keys=True)
PY
  echo "[phone-hil] report=${REPORT_PATH}"
}

finalize_report() {
  local explicit_status="${1:-}"
  local explicit_reason="${2:-}"
  if [[ -n "${explicit_status}" ]]; then
    write_report "${explicit_status}" "${explicit_reason}"
    return
  fi
  local computed
  computed="$("${PYTHON_BIN}" - "${RESULTS_JSONL}" <<'PY'
import json
import sys

path = sys.argv[1]
tests = []
try:
    with open(path, "r", encoding="utf-8") as handle:
        tests = [json.loads(line) for line in handle if line.strip()]
except FileNotFoundError:
    pass

bad = [test for test in tests if test.get("status") != "pass"]
if bad:
    print("fail|one or more required phone HIL checks failed or were unsupported")
else:
    print("pass|all recorded phone HIL checks passed")
PY
  )"
  write_report "${computed%%|*}" "${computed#*|}"
}

detect_serial() {
  local pattern="$1"
  "${PYTHON_BIN}" - "${ADB_DEVICES_LOG}" "${pattern}" <<'PY'
import re
import sys

path, pattern = sys.argv[1:3]
regex = re.compile(pattern, re.IGNORECASE)
with open(path, "r", encoding="utf-8", errors="ignore") as handle:
    for line in handle:
        parts = line.split()
        if len(parts) < 2 or parts[1] != "device":
            continue
        if regex.search(line):
            print(parts[0])
            raise SystemExit(0)
raise SystemExit(1)
PY
}

require_hash_shape() {
  local label="$1"
  local hash="$2"
  if [[ ! "${hash}" =~ ^[0-9a-fA-F]{32}$ ]]; then
    record_result "${label}_hash_preflight" "fail" "${label} LXMF hash must be 32 hex characters"
    return 1
  fi
}

run_capture() {
  local label="$1"
  local out="$2"
  shift 2
  log_cmd "$*"
  "$@" >"${out}" 2>&1
  local status=$?
  if [[ "${status}" -eq 0 ]]; then
    return 0
  fi
  record_result "${label}" "fail" "command exited with ${status}" "log=${out}"
  return "${status}"
}

write_config() {
  cat >"${CONFIG_PATH}" <<EOF
[propagation_node]
enabled = true
control_allowed = []
peer_announce_at_start = true
peer_announce_interval_secs = 30
node_announce_at_start = true
node_announce_interval_secs = 30
transfer_limit_kb = 256
sync_limit_kb = 10240
stamp_cost = 8
stamp_cost_flexibility = 3
peering_cost = 8
EOF
}

wait_for_log_pattern() {
  local file="$1"
  local pattern="$2"
  local timeout_secs="$3"
  local start
  start="$(date +%s)"
  while true; do
    if [[ -f "${file}" ]] && grep -Eq "${pattern}" "${file}"; then
      return 0
    fi
    if (( "$(date +%s)" - start >= timeout_secs )); then
      return 1
    fi
    sleep 1
  done
}

extract_daemon_hash() {
  sed -n 's/.*delivery destination hash=\([0-9a-fA-F]*\).*/\1/p' "${RETICULUMD_LOG}" \
    | tail -n1 \
    | tr '[:upper:]' '[:lower:]'
}

extract_daemon_propagation_hash() {
  sed -n 's/.*propagation destination hash=\([0-9a-fA-F]*\).*/\1/p' "${RETICULUMD_LOG}" \
    | tail -n1 \
    | tr '[:upper:]' '[:lower:]'
}

rpc_call() {
  local method="$1"
  local params_json="$2"
  local output_path="$3"
  "${PYTHON_BIN}" - "${RPC_ADDR}" "${method}" "${params_json}" >"${output_path}" <<'PY'
import json
import socket
import sys
import time

try:
    import RNS.vendor.umsgpack as msgpack  # type: ignore
except Exception:
    import msgpack  # type: ignore

rpc_addr, method, params_json = sys.argv[1:4]
host, port_s = rpc_addr.rsplit(":", 1)
params = None if params_json == "null" else json.loads(params_json)

def normalize(value):
    if isinstance(value, list) and len(value) == 3:
        return {"id": value[0], "result": value[1], "error": value[2]}
    return value

last_error = None
for _attempt in range(30):
    payload = {"id": 1, "method": method, "params": params}
    packed = msgpack.packb(payload)
    frame = len(packed).to_bytes(4, "big") + packed
    request = (
        f"POST /rpc HTTP/1.1\r\n"
        f"Host: {rpc_addr}\r\n"
        f"Content-Length: {len(frame)}\r\n"
        "Connection: close\r\n\r\n"
    ).encode("utf-8") + frame
    try:
        with socket.create_connection((host, int(port_s)), timeout=5) as sock:
            sock.sendall(request)
            response = bytearray()
            while True:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                response.extend(chunk)
        header_end = response.find(b"\r\n\r\n")
        if header_end < 0:
            raise RuntimeError("missing HTTP body")
        body = response[header_end + 4:]
        if len(body) < 4:
            raise RuntimeError("short RPC frame")
        frame_len = int.from_bytes(body[:4], "big")
        if len(body) < 4 + frame_len:
            raise RuntimeError("incomplete RPC frame")
        print(json.dumps(normalize(msgpack.unpackb(body[4:4 + frame_len])), sort_keys=True))
        raise SystemExit(0)
    except Exception as exc:  # noqa: BLE001 - shell harness reports the last retry error.
        last_error = exc
        time.sleep(1)

raise SystemExit(f"rpc call {method} failed: {last_error}")
PY
}

json_send_params() {
  local message_id="$1"
  local source="$2"
  local destination="$3"
  local title="$4"
  local content="$5"
  local method="$6"
  "${PYTHON_BIN}" - "${message_id}" "${source}" "${destination}" "${title}" "${content}" "${method}" <<'PY'
import json
import sys

message_id, source, destination, title, content, method = sys.argv[1:7]
payload = {
    "id": message_id,
    "source": source,
    "destination": destination,
    "title": title,
    "content": content,
    "method": method,
}
if method == "propagated":
    payload["include_ticket"] = True
    payload["try_propagation_on_fail"] = True
    payload["stamp_cost"] = 8
print(json.dumps(payload))
PY
}

message_evidence() {
  local label="$1"
  local message_id="$2"
  local dir="${ARTIFACT_ROOT}/${label}"
  mkdir -p "${dir}"

  "${REPO_ROOT}/target/debug/lxmf-cli" --rpc "${RPC_ADDR}" --output json status \
    --message-id "${message_id}" >"${dir}/status.json" 2>"${dir}/status.err" || true
  "${REPO_ROOT}/target/debug/lxmf-cli" --rpc "${RPC_ADDR}" --output json poll \
    --max 64 >"${dir}/poll.json" 2>"${dir}/poll.err" || true
  "${REPO_ROOT}/target/debug/lxmf-cli" --rpc "${RPC_ADDR}" --output json snapshot \
    >"${dir}/snapshot.json" 2>"${dir}/snapshot.err" || true
  rpc_call "sdk_snapshot_v2" "{}" "${dir}/sdk_snapshot_v2.json" || true
  rpc_call "sdk_status_v2" \
    "{\"message_id\":\"${message_id}\"}" \
    "${dir}/sdk_status_v2.json" || true
  rpc_call "list_messages" "{}" "${dir}/list_messages.json" || true
  rpc_call "list_peers" "{}" "${dir}/list_peers.json" || true
  rpc_call "message_delivery_trace" \
    "{\"message_id\":\"${message_id}\"}" \
    "${dir}/message_delivery_trace.json" || true
}

trace_contains() {
  local trace_path="$1"
  local pattern="$2"
  "${PYTHON_BIN}" - "${trace_path}" "${pattern}" <<'PY'
import json
import re
import sys

path, pattern = sys.argv[1:3]
try:
    with open(path, "r", encoding="utf-8") as handle:
        payload = json.load(handle)
except FileNotFoundError:
    raise SystemExit(1)
result = payload.get("result") if isinstance(payload, dict) else None
transitions = result.get("transitions", []) if isinstance(result, dict) else []
haystack = json.dumps(transitions).lower()
raise SystemExit(0 if re.search(pattern, haystack) else 1)
PY
}

wait_trace_contains() {
  local label="$1"
  local message_id="$2"
  local pattern="$3"
  local timeout_secs="$4"
  local dir="${ARTIFACT_ROOT}/${label}"
  mkdir -p "${dir}"
  local start
  start="$(date +%s)"
  while true; do
    rpc_call "message_delivery_trace" \
      "{\"message_id\":\"${message_id}\"}" \
      "${dir}/message_delivery_trace.json" || true
    if trace_contains "${dir}/message_delivery_trace.json" "${pattern}"; then
      return 0
    fi
    if (( "$(date +%s)" - start >= timeout_secs )); then
      return 1
    fi
    sleep 1
  done
}

wait_manifest_terminal() {
  local manifest="$1"
  local timeout_secs="$2"
  local pending_path="$3"
  local pattern="sent|delivered|failed|expired|rejected|cancelled"
  local start
  start="$(date +%s)"
  while true; do
    local pending=0
    : >"${pending_path}"
    while IFS=$'\t' read -r label message_id; do
      [[ -z "${label}" || -z "${message_id}" ]] && continue
      local dir="${ARTIFACT_ROOT}/${label}"
      mkdir -p "${dir}"
      rpc_call "message_delivery_trace" \
        "{\"message_id\":\"${message_id}\"}" \
        "${dir}/message_delivery_trace.json" || true
      if ! trace_contains "${dir}/message_delivery_trace.json" "${pattern}"; then
        printf '%s\t%s\n' "${label}" "${message_id}" >>"${pending_path}"
        pending=$((pending + 1))
      fi
    done <"${manifest}"
    if [[ "${pending}" -eq 0 ]]; then
      return 0
    fi
    if (( "$(date +%s)" - start >= timeout_secs )); then
      return 1
    fi
    sleep 3
  done
}

send_daemon_message() {
  local label="$1"
  local destination="$2"
  local content="$3"
  local method="$4"
  local message_id="phone-hil-${label}-${RUN_ID}"
  local dir="${ARTIFACT_ROOT}/${label}"
  mkdir -p "${dir}"
  local params
  params="$(json_send_params "${message_id}" "${DAEMON_HASH}" "${destination}" "${label}" "${content}" "${method}")"
  if rpc_call "send_message_v2" "${params}" "${dir}/send_message_v2.json"; then
    message_evidence "${label}" "${message_id}"
    echo "${message_id}"
    return 0
  fi
  record_result "${label}" "fail" "send_message_v2 failed" "send=${dir}/send_message_v2.json"
  echo "${message_id}"
  return 1
}

capture_screenshot() {
  local serial="$1"
  local out="$2"
  "${ADB}" -s "${serial}" exec-out screencap -p >"${out}" 2>/dev/null || true
}

manual_confirmation() {
  local test_id="$1"
  local prompt="$2"
  local serial="$3"
  local screenshot="$4"
  if [[ "${INTERACTIVE}" != "1" ]]; then
    record_result "${test_id}" "fail" "manual phone-visible evidence was not collected"
    return
  fi
  echo
  echo "${prompt}"
  echo "Waiting up to ${MANUAL_WAIT_SECS}s for operator confirmation."
  local answer=""
  read -r -t "${MANUAL_WAIT_SECS}" -p "Confirm pass? [y/N] " answer || true
  if [[ "${answer}" =~ ^[Yy]$ ]]; then
    capture_screenshot "${serial}" "${screenshot}"
    record_result "${test_id}" "pass" "operator confirmed phone-visible evidence" "screenshot=${screenshot}"
  else
    record_result "${test_id}" "fail" "operator did not confirm phone-visible evidence"
  fi
}

run_preflight() {
  if ! command -v "${ADB}" >/dev/null 2>&1; then
    record_result "adb_available" "fail" "adb was not found on PATH"
    write_report "blocked" "adb was not found"
    exit 1
  fi
  "${ADB}" devices -l >"${ADB_DEVICES_LOG}"
  if [[ -z "${SIDE_BAND_SERIAL}" ]]; then
    SIDE_BAND_SERIAL="$(detect_serial "${S8_MODEL_PATTERN}" || true)"
  fi
  if [[ -z "${PIXEL_DEVICE_SERIAL}" ]]; then
    PIXEL_DEVICE_SERIAL="$(detect_serial "${PIXEL_MODEL_PATTERN}" || true)"
  fi
  if [[ -z "${SIDE_BAND_SERIAL}" ]]; then
    record_result "s8_sideband_device_preflight" "fail" "S8/Sideband phone not visible in adb devices -l" "adb=${ADB_DEVICES_LOG}"
  else
    record_result "s8_sideband_device_preflight" "pass" "S8/Sideband phone visible" "adb=${ADB_DEVICES_LOG}"
  fi
  if [[ -z "${PIXEL_DEVICE_SERIAL}" ]]; then
    record_result "pixel_columba_device_preflight" "fail" "Pixel/Columba phone not visible in adb devices -l" "adb=${ADB_DEVICES_LOG}"
  else
    record_result "pixel_columba_device_preflight" "pass" "Pixel/Columba phone visible" "adb=${ADB_DEVICES_LOG}"
  fi
  if [[ -z "${SIDE_BAND_SERIAL}" || -z "${PIXEL_DEVICE_SERIAL}" ]]; then
    write_report "blocked" "both phones must be connected before the field test can run"
    exit 1
  fi
}

start_logcat() {
  "${ADB}" -s "${SIDE_BAND_SERIAL}" logcat -c >/dev/null 2>&1 || true
  "${ADB}" -s "${PIXEL_DEVICE_SERIAL}" logcat -c >/dev/null 2>&1 || true
  "${ADB}" -s "${SIDE_BAND_SERIAL}" logcat -v threadtime >"${SIDE_BAND_LOGCAT}" 2>&1 &
  SIDE_BAND_LOGCAT_PID=$!
  "${ADB}" -s "${PIXEL_DEVICE_SERIAL}" logcat -v threadtime >"${COLUMBA_LOGCAT}" 2>&1 &
  COLUMBA_LOGCAT_PID=$!
  record_result "logcat_capture" "pass" "started phone logcat capture" \
    "sideband_logcat=${SIDE_BAND_LOGCAT}" \
    "columba_logcat=${COLUMBA_LOGCAT}"
}

configure_adb_reverse() {
  local s8_reverse="${ARTIFACT_ROOT}/adb-reverse-s8.txt"
  local pixel_reverse="${ARTIFACT_ROOT}/adb-reverse-pixel.txt"
  local failed=0
  "${ADB}" -s "${SIDE_BAND_SERIAL}" reverse --remove "tcp:${SIDE_BAND_REVERSE_PORT}" >/dev/null 2>&1 || true
  "${ADB}" -s "${PIXEL_DEVICE_SERIAL}" reverse --remove "tcp:${PIXEL_REVERSE_PORT}" >/dev/null 2>&1 || true
  if ! run_capture "s8_adb_reverse" "${s8_reverse}" \
    "${ADB}" -s "${SIDE_BAND_SERIAL}" reverse "tcp:${SIDE_BAND_REVERSE_PORT}" "tcp:${TRANSPORT_PORT}"; then
    failed=1
  fi
  if ! run_capture "pixel_adb_reverse" "${pixel_reverse}" \
    "${ADB}" -s "${PIXEL_DEVICE_SERIAL}" reverse "tcp:${PIXEL_REVERSE_PORT}" "tcp:${TRANSPORT_PORT}"; then
    failed=1
  fi
  if [[ "${failed}" -ne 0 ]]; then
    return 1
  fi
  record_result "adb_reverse" "pass" "configured adb reverse for both phones" \
    "s8_mapping=tcp:${SIDE_BAND_REVERSE_PORT}->tcp:${TRANSPORT_PORT}" \
    "pixel_mapping=tcp:${PIXEL_REVERSE_PORT}->tcp:${TRANSPORT_PORT}" \
    "s8=${s8_reverse}" \
    "pixel=${pixel_reverse}"
}

start_daemon() {
  write_config
  mkdir -p "$(dirname "${IDENTITY_PATH}")"
  (
    RETICULUMD_DIAGNOSTICS=1 \
      LXMD_DELIVERY_PER_PEER_IN_FLIGHT="${PER_PEER_IN_FLIGHT}" \
      "${REPO_ROOT}/target/debug/reticulumd" \
      --rpc "${RPC_ADDR}" \
      --db "${DB_PATH}" \
      --identity "${IDENTITY_PATH}" \
      --transport "${TRANSPORT_ADDR}" \
      --announce-interval-secs 2 \
      --config "${CONFIG_PATH}" >"${RETICULUMD_LOG}" 2>&1
  ) &
  RETICULUMD_PID=$!
  if ! wait_for_log_pattern "${RETICULUMD_LOG}" "delivery destination hash=" 60; then
    record_result "reticulumd_start" "fail" "reticulumd did not report a delivery destination hash" "log=${RETICULUMD_LOG}"
    write_report "blocked" "reticulumd did not start cleanly"
    exit 1
  fi
  if ! wait_for_log_pattern "${RETICULUMD_LOG}" "propagation destination hash=" 60; then
    record_result "reticulumd_start" "fail" "reticulumd did not report a propagation destination hash" "log=${RETICULUMD_LOG}"
    write_report "blocked" "reticulumd did not activate propagation destination"
    exit 1
  fi
  DAEMON_HASH="$(extract_daemon_hash)"
  DAEMON_PROPAGATION_HASH="$(extract_daemon_propagation_hash)"
  record_result "reticulumd_start" "pass" "reticulumd started with diagnostics and propagation enabled" \
    "log=${RETICULUMD_LOG}" \
    "config=${CONFIG_PATH}" \
    "identity=${IDENTITY_PATH}" \
    "delivery_hash=${DAEMON_HASH}" \
    "propagation_hash=${DAEMON_PROPAGATION_HASH}"
}

require_phone_hashes_or_block() {
  if [[ "${AUTO_DISCOVER_PHONE_HASHES}" == "1" && ( -z "${SIDE_BAND_HASH}" || -z "${COLUMBA_HASH}" ) ]]; then
    discover_phone_hashes || true
  fi
  local missing=0
  require_hash_shape "SIDE_BAND_HASH" "${SIDE_BAND_HASH}" || missing=1
  require_hash_shape "COLUMBA_HASH" "${COLUMBA_HASH}" || missing=1
  if [[ "${missing}" -ne 0 ]]; then
    write_report "blocked" "set SIDE_BAND_HASH and COLUMBA_HASH after reading the phone LXMF destination hashes"
    echo "[phone-hil] daemon_hash=${DAEMON_HASH}"
    echo "[phone-hil] daemon_propagation_hash=${DAEMON_PROPAGATION_HASH}"
    echo "[phone-hil] configure Sideband phone ${SIDE_BAND_SERIAL} to TCP client 127.0.0.1:${SIDE_BAND_REVERSE_PORT}"
    echo "[phone-hil] configure Sideband phone ${PIXEL_DEVICE_SERIAL} to TCP client 127.0.0.1:${PIXEL_REVERSE_PORT}"
    exit 2
  fi
}

discover_phone_hashes() {
  local dir="${ARTIFACT_ROOT}/phone-peer-readiness"
  local peers_path="${dir}/autodiscovered-peers.json"
  mkdir -p "${dir}"
  local start
  start="$(date +%s)"
  while true; do
    rpc_call "list_peers" "{}" "${peers_path}" || true
    local discovered
    discovered="$("${PYTHON_BIN}" - "${peers_path}" "${DAEMON_HASH}" "${DAEMON_PROPAGATION_HASH}" <<'PY'
import json
import sys

path, *daemon_hashes = sys.argv[1:]
excluded_hashes = {value.lower() for value in daemon_hashes if len(value) == 32}
try:
    with open(path, "r", encoding="utf-8") as handle:
        payload = json.load(handle)
except Exception:
    raise SystemExit(0)
result = payload.get("result") if isinstance(payload, dict) else None
peers = result.get("peers", []) if isinstance(result, dict) else []
seen = []
ranked = []
for peer in peers:
    if not isinstance(peer, dict):
        continue
    distance = peer.get("network_distance")
    try:
        distance_rank = int(distance)
    except Exception:
        distance_rank = 999
    last_seen = peer.get("last_seen")
    try:
        last_seen_rank = -int(last_seen)
    except Exception:
        last_seen_rank = 0
    for key in ("destination_hash", "peer", "hash"):
        value = peer.get(key)
        if not isinstance(value, str):
            continue
        value = value.lower()
        if len(value) == 32 and value not in excluded_hashes and value not in seen:
            seen.append(value)
            ranked.append((distance_rank, last_seen_rank, value))
direct = [value for distance, _, value in sorted(ranked) if distance <= 1]
fallback = [value for _, _, value in sorted(ranked) if value not in direct]
print("\n".join(direct + fallback))
PY
    )"
    mapfile -t discovered_hashes <<<"${discovered}"
    for index in "${!discovered_hashes[@]}"; do
      discovered_hashes[${index}]="${discovered_hashes[${index}]//$'\r'/}"
    done
    if [[ -z "${SIDE_BAND_HASH}" && "${#discovered_hashes[@]}" -ge 1 && -n "${discovered_hashes[0]}" ]]; then
      SIDE_BAND_HASH="${discovered_hashes[0]}"
    fi
    if [[ -z "${COLUMBA_HASH}" ]]; then
      for candidate in "${discovered_hashes[@]}"; do
        if [[ -n "${candidate}" && "${candidate}" != "${SIDE_BAND_HASH}" ]]; then
          COLUMBA_HASH="${candidate}"
          break
        fi
      done
    fi
    if [[ -n "${SIDE_BAND_HASH}" && -n "${COLUMBA_HASH}" ]]; then
      record_result "phone_hash_autodiscovery" "pass" "autodiscovered phone LXMF destination hashes from list_peers" \
        "peers=${peers_path}"
      return 0
    fi
    if (( "$(date +%s)" - start >= PHONE_PEER_WAIT_SECS )); then
      record_result "phone_hash_autodiscovery" "fail" "could not autodiscover two phone LXMF destination hashes from list_peers" \
        "peers=${peers_path}"
      return 1
    fi
    sleep 2
  done
}

phone_peer_seen() {
  local peers_path="$1"
  local hash="$2"
  "${PYTHON_BIN}" - "${peers_path}" "${hash}" <<'PY'
import json
import sys

path, expected_hash = sys.argv[1:3]
expected_hash = expected_hash.lower()
with open(path, "r", encoding="utf-8") as handle:
    payload = json.load(handle)
result = payload.get("result") if isinstance(payload, dict) else None
peers = result.get("peers", []) if isinstance(result, dict) else []
for peer in peers:
    if not isinstance(peer, dict):
        continue
    values = [
        peer.get("destination_hash"),
        peer.get("peer"),
        peer.get("hash"),
    ]
    if any(str(value).lower() == expected_hash for value in values if value):
        raise SystemExit(0)
raise SystemExit(1)
PY
}

wait_for_phone_peer() {
  local label="$1"
  local hash="$2"
  local timeout_secs="$3"
  local dir="${ARTIFACT_ROOT}/phone-peer-readiness"
  local peers_path="${dir}/list_peers.${label}.json"
  mkdir -p "${dir}"
  local start
  start="$(date +%s)"
  while true; do
    rpc_call "list_peers" "{}" "${peers_path}" || true
    if phone_peer_seen "${peers_path}" "${hash}"; then
      return 0
    fi
    if (( "$(date +%s)" - start >= timeout_secs )); then
      return 1
    fi
    sleep 2
  done
}

run_phone_peer_readiness() {
  local dir="${ARTIFACT_ROOT}/phone-peer-readiness"
  mkdir -p "${dir}"
  local missing=0
  if ! wait_for_phone_peer "s8_sideband" "${SIDE_BAND_HASH}" "${PHONE_PEER_WAIT_SECS}"; then
    printf '%s\n' "${SIDE_BAND_HASH}" >"${dir}/missing-s8-sideband.hash"
    missing=1
  fi
  if ! wait_for_phone_peer "pixel_columba" "${COLUMBA_HASH}" "${PHONE_PEER_WAIT_SECS}"; then
    printf '%s\n' "${COLUMBA_HASH}" >"${dir}/missing-pixel-columba.hash"
    missing=1
  fi
  if [[ "${missing}" -eq 0 ]]; then
    record_result "phone_peer_readiness" "pass" "daemon list_peers observed both phone LXMF destination hashes before sends" \
      "s8=${dir}/list_peers.s8_sideband.json" \
      "pixel=${dir}/list_peers.pixel_columba.json"
    return 0
  fi
  record_result "phone_peer_readiness" "fail" "daemon list_peers did not observe every phone LXMF destination hash before sends" \
    "s8=${dir}/list_peers.s8_sideband.json" \
    "pixel=${dir}/list_peers.pixel_columba.json"
  write_report "blocked" "phone peer announces were not observed before delivery tests"
  exit 1
}

run_daemon_to_phone_cases() {
  local s8_packet_id
  local pixel_packet_id
  s8_packet_id="$(send_daemon_message "daemon-to-s8-packet" "${SIDE_BAND_HASH}" "phone-hil daemon to S8 packet ${RUN_ID}" "direct")" || true
  pixel_packet_id="$(send_daemon_message "daemon-to-pixel-packet" "${COLUMBA_HASH}" "phone-hil daemon to Pixel packet ${RUN_ID}" "direct")" || true

  if wait_trace_contains "daemon-to-s8-packet" "${s8_packet_id}" "queued|sending" 15; then
    record_result "packetreceipt_queued_sending_s8" "pass" "daemon-to-S8 trace exposed queued or sending state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-packet/message_delivery_trace.json"
  else
    record_result "packetreceipt_queued_sending_s8" "fail" "daemon-to-S8 trace did not expose queued or sending state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-packet/message_delivery_trace.json"
  fi
  if wait_trace_contains "daemon-to-pixel-packet" "${pixel_packet_id}" "queued|sending" 15; then
    record_result "packetreceipt_queued_sending_pixel" "pass" "daemon-to-Pixel trace exposed queued or sending state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-packet/message_delivery_trace.json"
  else
    record_result "packetreceipt_queued_sending_pixel" "fail" "daemon-to-Pixel trace did not expose queued or sending state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-packet/message_delivery_trace.json"
  fi

  if wait_trace_contains "daemon-to-s8-packet" "${s8_packet_id}" "sent" 45; then
    record_result "packetreceipt_sent_s8" "pass" "daemon-to-S8 trace reached sent handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-packet/message_delivery_trace.json"
  else
    record_result "packetreceipt_sent_s8" "fail" "daemon-to-S8 trace did not reach sent handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-packet/message_delivery_trace.json"
  fi
  if wait_trace_contains "daemon-to-pixel-packet" "${pixel_packet_id}" "sent" 45; then
    record_result "packetreceipt_sent_pixel" "pass" "daemon-to-Pixel trace reached sent handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-packet/message_delivery_trace.json"
  else
    record_result "packetreceipt_sent_pixel" "fail" "daemon-to-Pixel trace did not reach sent handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-packet/message_delivery_trace.json"
  fi

  if wait_trace_contains "daemon-to-s8-packet" "${s8_packet_id}" "delivered" 30; then
    record_result "packetreceipt_delivered_s8" "pass" "S8 emitted delivery receipt" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-packet/message_delivery_trace.json"
  else
    record_result "packetreceipt_delivered_s8" "fail" "S8 delivery receipt was not observed" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-packet/message_delivery_trace.json"
  fi
  if wait_trace_contains "daemon-to-pixel-packet" "${pixel_packet_id}" "delivered" 30; then
    record_result "packetreceipt_delivered_pixel" "pass" "Pixel emitted delivery receipt" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-packet/message_delivery_trace.json"
  else
    record_result "packetreceipt_delivered_pixel" "fail" "Pixel delivery receipt was not observed" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-packet/message_delivery_trace.json"
  fi

  manual_confirmation \
    "daemon_to_s8_app_visible" \
    "Verify Sideband on the S8 displays: phone-hil daemon to S8 packet ${RUN_ID}" \
    "${SIDE_BAND_SERIAL}" \
    "${ARTIFACT_ROOT}/daemon-to-s8-visible.png"
  manual_confirmation \
    "daemon_to_pixel_app_visible" \
    "Verify Columba on the Pixel displays: phone-hil daemon to Pixel packet ${RUN_ID}" \
    "${PIXEL_DEVICE_SERIAL}" \
    "${ARTIFACT_ROOT}/daemon-to-pixel-visible.png"
}

run_resource_cases() {
  local large_content
  large_content="$("${PYTHON_BIN}" - "${LARGE_BYTES}" "${RUN_ID}" <<'PY'
import sys
size = max(int(sys.argv[1]), 1024)
run_id = sys.argv[2]
prefix = f"phone-hil resource {run_id}:"
print(prefix + ("R" * size))
PY
  )"
  local s8_resource_id
  local pixel_resource_id
  s8_resource_id="$(send_daemon_message "daemon-to-s8-resource" "${SIDE_BAND_HASH}" "${large_content}" "direct")" || true
  pixel_resource_id="$(send_daemon_message "daemon-to-pixel-resource" "${COLUMBA_HASH}" "${large_content}" "direct")" || true

  if wait_trace_contains "daemon-to-s8-resource" "${s8_resource_id}" "sending: link resource" 90; then
    record_result "resource_sending_s8" "pass" "S8 large payload entered link resource sending state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-resource/message_delivery_trace.json"
  else
    record_result "resource_sending_s8" "fail" "S8 large payload did not expose sending: link resource" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-resource/message_delivery_trace.json"
  fi
  if wait_trace_contains "daemon-to-pixel-resource" "${pixel_resource_id}" "sending: link resource" 90; then
    record_result "resource_sending_pixel" "pass" "Pixel large payload entered link resource sending state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-resource/message_delivery_trace.json"
  else
    record_result "resource_sending_pixel" "fail" "Pixel large payload did not expose sending: link resource" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-resource/message_delivery_trace.json"
  fi

  if wait_trace_contains "daemon-to-s8-resource" "${s8_resource_id}" "sent: link resource" 90; then
    record_result "resource_transfer_s8" "pass" "S8 large payload reached resource completion handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-resource/message_delivery_trace.json"
  else
    record_result "resource_transfer_s8" "fail" "S8 large payload did not reach sent: link resource" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-resource/message_delivery_trace.json"
  fi
  if wait_trace_contains "daemon-to-pixel-resource" "${pixel_resource_id}" "sent: link resource" 90; then
    record_result "resource_transfer_pixel" "pass" "Pixel large payload reached resource completion handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-resource/message_delivery_trace.json"
  else
    record_result "resource_transfer_pixel" "fail" "Pixel large payload did not reach sent: link resource" \
      "trace=${ARTIFACT_ROOT}/daemon-to-pixel-resource/message_delivery_trace.json"
  fi
}

run_failed_receipt_case() {
  local failed_id
  failed_id="$(send_daemon_message "daemon-to-unreachable-failure" "ffffffffffffffffffffffffffffffff" "phone-hil unreachable ${RUN_ID}" "direct")" || true
  if wait_trace_contains "daemon-to-unreachable-failure" "${failed_id}" "failed" 45; then
    record_result "packetreceipt_failed" "pass" "unreachable destination produced failed receipt state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-unreachable-failure/message_delivery_trace.json"
  else
    record_result "packetreceipt_failed" "fail" "unreachable destination did not produce failed receipt state" \
      "trace=${ARTIFACT_ROOT}/daemon-to-unreachable-failure/message_delivery_trace.json"
  fi
}

run_manual_phone_to_phone_cases() {
  manual_confirmation \
    "s8_to_daemon_inbound" \
    "From Sideband on the S8, send exactly: phone-hil S8 to daemon ${RUN_ID} to daemon ${DAEMON_HASH}." \
    "${SIDE_BAND_SERIAL}" \
    "${ARTIFACT_ROOT}/s8-to-daemon.png"
  manual_confirmation \
    "pixel_to_daemon_inbound" \
    "From Columba on the Pixel, send exactly: phone-hil Pixel to daemon ${RUN_ID} to daemon ${DAEMON_HASH}." \
    "${PIXEL_DEVICE_SERIAL}" \
    "${ARTIFACT_ROOT}/pixel-to-daemon.png"
  manual_confirmation \
    "s8_to_pixel_routing" \
    "From Sideband on the S8, send exactly: phone-hil S8 to Pixel ${RUN_ID} to Columba ${COLUMBA_HASH}." \
    "${PIXEL_DEVICE_SERIAL}" \
    "${ARTIFACT_ROOT}/s8-to-pixel-visible.png"
  manual_confirmation \
    "pixel_to_s8_routing" \
    "From Columba on the Pixel, send exactly: phone-hil Pixel to S8 ${RUN_ID} to Sideband ${SIDE_BAND_HASH}." \
    "${SIDE_BAND_SERIAL}" \
    "${ARTIFACT_ROOT}/pixel-to-s8-visible.png"
}

run_queue_burst() {
  local burst_dir="${ARTIFACT_ROOT}/queue-burst"
  mkdir -p "${burst_dir}"
  local manifest="${burst_dir}/message-manifest.tsv"
  : >"${manifest}"
  local failures=0
  local i
  for i in $(seq 1 "${BURST_COUNT}"); do
    local s8_message_id=""
    local pixel_message_id=""
    if ! s8_message_id="$(send_daemon_message "burst-s8-${i}" "${SIDE_BAND_HASH}" "phone-hil burst S8 ${RUN_ID} ${i}" "direct")"; then
      failures=$((failures + 1))
    fi
    printf '%s\n' "${s8_message_id}" >"${burst_dir}/s8-${i}.message_id"
    printf 'burst-s8-%s\t%s\n' "${i}" "${s8_message_id}" >>"${manifest}"

    if ! pixel_message_id="$(send_daemon_message "burst-pixel-${i}" "${COLUMBA_HASH}" "phone-hil burst Pixel ${RUN_ID} ${i}" "direct")"; then
      failures=$((failures + 1))
    fi
    printf '%s\n' "${pixel_message_id}" >"${burst_dir}/pixel-${i}.message_id"
    printf 'burst-pixel-%s\t%s\n' "${i}" "${pixel_message_id}" >>"${manifest}"
  done
  rpc_call "sdk_snapshot_v2" "{}" "${burst_dir}/sdk_snapshot_v2-after-burst.json" || true
  if "${PYTHON_BIN}" - "${burst_dir}/sdk_snapshot_v2-after-burst.json" "${PER_PEER_IN_FLIGHT}" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
expected = int(sys.argv[2])
pipeline = (payload.get("result") or {}).get("delivery_pipeline") or {}
actual = pipeline.get("per_peer_in_flight")
raise SystemExit(0 if actual == expected else 1)
PY
  then
    record_result "queue_burst_pipeline_bounds" "pass" "delivery_pipeline reports configured per-peer in-flight bound" \
      "snapshot=${burst_dir}/sdk_snapshot_v2-after-burst.json"
  else
    record_result "queue_burst_pipeline_bounds" "fail" "delivery_pipeline did not report configured per-peer in-flight bound" \
      "snapshot=${burst_dir}/sdk_snapshot_v2-after-burst.json"
  fi
  if [[ "${failures}" -eq 0 ]] && wait_manifest_terminal "${manifest}" 180 "${burst_dir}/pending-terminal.tsv"; then
    record_result "queue_burst_acceptance" "pass" "accepted ${BURST_COUNT} sends per phone and observed terminal delivery states" \
      "snapshot=${burst_dir}/sdk_snapshot_v2-after-burst.json"
  else
    record_result "queue_burst_acceptance" "fail" "${failures} burst sends failed or did not reach terminal states" \
      "snapshot=${burst_dir}/sdk_snapshot_v2-after-burst.json" \
      "pending=${burst_dir}/pending-terminal.tsv"
  fi
}

run_failure_recovery() {
  local dir="${ARTIFACT_ROOT}/failure-recovery"
  mkdir -p "${dir}"
  "${ADB}" -s "${PIXEL_DEVICE_SERIAL}" reverse --remove "tcp:${PIXEL_REVERSE_PORT}" >"${dir}/pixel-reverse-remove.txt" 2>&1 || true
  local failed_id
  failed_id="$(send_daemon_message "failure-recovery-pixel-disconnected" "${COLUMBA_HASH}" "phone-hil disconnected Pixel ${RUN_ID}" "direct")" || true
  message_evidence "failure-recovery-pixel-disconnected" "${failed_id}"
  if ! "${ADB}" -s "${PIXEL_DEVICE_SERIAL}" reverse "tcp:${PIXEL_REVERSE_PORT}" "tcp:${TRANSPORT_PORT}" >"${dir}/pixel-reverse-restore.txt" 2>&1; then
    record_result "failure_recovery" "fail" "Pixel reverse restore command failed" \
      "restore=${dir}/pixel-reverse-restore.txt"
    return
  fi
  local restored_id
  restored_id="$(send_daemon_message "failure-recovery-pixel-restored" "${COLUMBA_HASH}" "phone-hil restored Pixel ${RUN_ID}" "direct")" || true
  if wait_trace_contains "failure-recovery-pixel-restored" "${restored_id}" "sent" 60; then
    record_result "failure_recovery" "pass" "Pixel reverse restored and later daemon send reached sent handoff" \
      "trace=${ARTIFACT_ROOT}/failure-recovery-pixel-restored/message_delivery_trace.json"
  else
    record_result "failure_recovery" "fail" "Pixel reverse restore did not recover later daemon send" \
      "trace=${ARTIFACT_ROOT}/failure-recovery-pixel-restored/message_delivery_trace.json"
  fi
}

run_propagation_cases() {
  local dir="${ARTIFACT_ROOT}/propagation"
  mkdir -p "${dir}"
  rpc_call "propagation_status" "{}" "${dir}/propagation_status.initial.json" || true
  rpc_call "list_propagation_nodes" "{}" "${dir}/list_propagation_nodes.json" || true
  rpc_call "propagation_peer_maintenance" "{}" "${dir}/propagation_peer_maintenance.json" || true
  if "${PYTHON_BIN}" - "${dir}/propagation_peer_maintenance.json" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
result = payload.get("result", {})
raise SystemExit(0 if int(result.get("culled", 0) or 0) > 0 else 1)
PY
  then
    record_result "packetreceipt_culled" "pass" "propagation maintenance reported culled peers" \
      "maintenance=${dir}/propagation_peer_maintenance.json"
  else
    record_result "packetreceipt_culled" "fail" "propagation maintenance did not cull any peers" \
      "maintenance=${dir}/propagation_peer_maintenance.json"
  fi

  if "${PYTHON_BIN}" - "${dir}/list_propagation_nodes.json" "${SIDE_BAND_HASH}" "${COLUMBA_HASH}" <<'PY'
import json
import sys
with open(sys.argv[1], "r", encoding="utf-8") as handle:
    payload = json.load(handle)
haystack = json.dumps(payload).lower()
phones = [value.lower() for value in sys.argv[2:4] if value]
raise SystemExit(0 if any(phone in haystack for phone in phones) else 1)
PY
  then
    record_result "link_request_phone_capability" "pass" "phone-announced propagation/control node is visible" \
      "nodes=${dir}/list_propagation_nodes.json"
  else
    record_result "link_request_phone_capability" "unsupported-by-phone-app" "no phone-announced propagation/control destination was visible for Link.request coverage" \
      "nodes=${dir}/list_propagation_nodes.json"
  fi

  rpc_call "get_outbound_propagation_cost" "{\"peer\":\"${DAEMON_PROPAGATION_HASH}\"}" "${dir}/get_outbound_propagation_cost.self.json" || true
  rpc_call "set_outbound_propagation_node" "{\"peer\":\"${DAEMON_PROPAGATION_HASH}\"}" "${dir}/set_outbound_propagation_node.self.json" || true
  rpc_call "propagation_status" "{}" "${dir}/propagation_status.after-set.json" || true
  local propagated_id
  propagated_id="$(send_daemon_message "daemon-to-s8-propagated" "${SIDE_BAND_HASH}" "phone-hil propagated to S8 ${RUN_ID}" "propagated")" || true
  if wait_trace_contains "daemon-to-s8-propagated" "${propagated_id}" "sent: propagated resource" 90; then
    record_result "propagation_node_delivery_attempt" "pass" "propagated delivery reached local propagation storage handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-propagated/message_delivery_trace.json" \
      "status=${dir}/propagation_status.after-set.json"
  else
    record_result "propagation_node_delivery_attempt" "fail" "propagated delivery did not reach local propagation storage handoff" \
      "trace=${ARTIFACT_ROOT}/daemon-to-s8-propagated/message_delivery_trace.json" \
      "status=${dir}/propagation_status.after-set.json"
  fi
}

run_channel_case() {
  if [[ "${PHONE_CHANNEL_CONFIRMED:-0}" == "1" ]]; then
    record_result "channel_reliable_delivery" "pass" "operator supplied PHONE_CHANNEL_CONFIRMED=1 for a phone-app channel-capable path"
  else
    record_result "channel_reliable_delivery" "unsupported-by-phone-app" "Sideband/Columba phone UI path did not expose Channel sequence/retry callbacks to this phones-only harness"
  fi
}

main() {
  run_preflight
  if [[ "${PRE_FLIGHT_ONLY}" == "1" ]]; then
    finalize_report
    exit 0
  fi
  if [[ "${BUILD}" == "1" ]]; then
    if ! run_capture "build_reticulumd" "${ARTIFACT_ROOT}/cargo-build-reticulumd.log" \
      cargo build -p reticulumd --bin reticulumd; then
      write_report "blocked" "cargo build -p reticulumd --bin reticulumd failed"
      exit 1
    fi
    if ! run_capture "build_lxmf_cli" "${ARTIFACT_ROOT}/cargo-build-lxmf-cli.log" \
      cargo build -p lxmf-cli --bin lxmf-cli; then
      write_report "blocked" "cargo build -p lxmf-cli --bin lxmf-cli failed"
      exit 1
    fi
    record_result "build" "pass" "built reticulumd and lxmf-cli" \
      "reticulumd=${ARTIFACT_ROOT}/cargo-build-reticulumd.log" \
      "lxmf_cli=${ARTIFACT_ROOT}/cargo-build-lxmf-cli.log"
  fi
  start_logcat
  if ! configure_adb_reverse; then
    write_report "blocked" "adb reverse setup failed"
    exit 1
  fi
  start_daemon
  require_phone_hashes_or_block
  run_phone_peer_readiness

  run_daemon_to_phone_cases
  run_failed_receipt_case
  run_resource_cases
  run_manual_phone_to_phone_cases
  run_queue_burst
  run_failure_recovery
  run_propagation_cases
  run_channel_case

  finalize_report
}

main "$@"
