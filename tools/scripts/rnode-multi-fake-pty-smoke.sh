#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/rnode-multi-fake-pty-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-rnode-multi-fake-pty.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
RNodeCONF_JSON="${RUN_DIR}/rnodeconf-blink.json"
FAKE_LOG="${RUN_DIR}/fake-rnode-multi.log"
FAKE_STATE="${RUN_DIR}/fake-rnode-multi-state.json"
FAKE_PORT_FILE="${RUN_DIR}/fake-rnode-multi.port"

: >"$RETICULUMD_LOG"
: >"$FAKE_LOG"

if [[ -z "${RPC_ADDR:-}" ]]; then
  RPC_ADDR="$(
    python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(f"127.0.0.1:{sock.getsockname()[1]}")
PY
  )"
fi

write_report() {
  local status="$1"
  local reason="${2:-}"
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$RNodeCONF_JSON" "$FAKE_LOG" "$FAKE_STATE"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    rpc_addr,
    run_dir,
    config_path,
    reticulumd_log,
    rnstatus_json,
    rnstatus_human,
    rnodeconf_json,
    fake_log,
    fake_state,
) = sys.argv[1:13]
report = {
    "status": status,
    "evidence_scope": "software_fake_pty_rnode_multi",
    "product_boundary": (
        "This proves the RNodeMulti serial parent software path, startup probe, "
        "runtime status, and vport management dispatch against a local PTY fake "
        "peer; it is not prepared-host or broad hardware production parity."
    ),
    "reason": reason or None,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
    "reticulumd_log": reticulumd_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
    "rnodeconf_json": rnodeconf_json,
    "fake_log": fake_log,
    "fake_state": fake_state,
}
json_path = pathlib.Path(rnstatus_json)
if json_path.exists():
    try:
        payload = json.loads(json_path.read_text(encoding="utf-8"))
        row = next(
            (
                item
                for item in payload.get("interfaces", [])
                if item.get("type") == "rnode_multi"
                and item.get("name") == "rnode-multi-fake-pty"
            ),
            None,
        )
        if row:
            runtime = (row.get("settings") or {}).get("_runtime") or {}
            radio = ((runtime.get("rnode_multi") or {}).get("radio_status") or {})
            probe = radio.get("startup_probe") or {}
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_iface"] = runtime.get("runtime_iface") or runtime.get("iface")
            report["stream_state"] = radio.get("stream_state")
            report["selected_vport"] = radio.get("selected_vport")
            report["vports"] = radio.get("vports")
            report["last_error"] = radio.get("last_error")
            report["startup_probe"] = probe
    except Exception as exc:
        report["status_parse_error"] = str(exc)
state_path = pathlib.Path(fake_state)
if state_path.exists():
    try:
        report["fake_peer"] = json.loads(state_path.read_text(encoding="utf-8"))
    except Exception as exc:
        report["fake_state_parse_error"] = str(exc)
conf_path = pathlib.Path(rnodeconf_json)
if conf_path.exists():
    try:
        report["rnodeconf_result"] = json.loads(conf_path.read_text(encoding="utf-8"))
    except Exception as exc:
        report["rnodeconf_parse_error"] = str(exc)
human_path = pathlib.Path(rnstatus_human)
if human_path.exists():
    report["human_summary"] = human_path.read_text(encoding="utf-8", errors="replace")
pathlib.Path(report_path).write_text(
    json.dumps(report, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY
}

cleanup() {
  local status=$?
  if [[ -n "${RET_PID:-}" ]]; then
    kill "$RET_PID" >/dev/null 2>&1 || true
    wait "$RET_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${FAKE_PID:-}" ]]; then
    kill "$FAKE_PID" >/dev/null 2>&1 || true
    wait "$FAKE_PID" >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[rnode-multi-fake-pty-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[rnode-multi-fake-pty-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - "$FAKE_PORT_FILE" "$FAKE_LOG" "$FAKE_STATE" <<'PY' &
import json
import os
import pathlib
import pty
import select
import sys
import threading
import time
import tty

port_path, log_path, state_path = sys.argv[1:4]

FEND = 0xC0
FESC = 0xDB
TFEND = 0xDC
TFESC = 0xDD
CMD_DETECT = 0x08
CMD_FW_VERSION = 0x50
CMD_PLATFORM = 0x48
CMD_MCU = 0x49
CMD_INTERFACES = 0x71
CMD_SEL_INT = 0x1F
CMD_BLINK = 0x30
DETECT_RESP = 0x46
PLATFORM_ESP32 = 0x80

lock = threading.Lock()
state = {
    "port": None,
    "frames": [],
    "probe_responses": [],
    "management_blink_seen": False,
    "selected_vport": None,
    "pty_raw_mode": True,
}


def save_state():
    with open(state_path, "w", encoding="utf-8") as handle:
        json.dump(state, handle, indent=2, sort_keys=True)
        handle.write("\n")


def log(message):
    with lock:
        with open(log_path, "a", encoding="utf-8") as handle:
            handle.write(message + "\n")
            handle.flush()


def encode_frame(command, payload):
    body = bytes([command]) + bytes(payload)
    escaped = bytearray()
    for value in body:
        if value == FEND:
            escaped.extend([FESC, TFEND])
        elif value == FESC:
            escaped.extend([FESC, TFESC])
        else:
            escaped.append(value)
    return bytes([FEND]) + bytes(escaped) + bytes([FEND])


def pop_frames(buffer):
    frames = []
    while True:
        try:
            start = buffer.index(FEND)
            end = buffer.index(FEND, start + 1)
        except ValueError:
            if len(buffer) > 8192:
                del buffer[:]
            return frames
        raw = bytes(buffer[start + 1 : end])
        del buffer[: end + 1]
        if not raw:
            continue
        decoded = bytearray()
        escape = False
        for value in raw:
            if escape:
                if value == TFEND:
                    decoded.append(FEND)
                elif value == TFESC:
                    decoded.append(FESC)
                else:
                    decoded.append(value)
                escape = False
            elif value == FESC:
                escape = True
            else:
                decoded.append(value)
        if decoded:
            frames.append(bytes(decoded))


def command_response(command):
    if command == CMD_DETECT:
        return [DETECT_RESP]
    if command == CMD_FW_VERSION:
        return [1, 74]
    if command == CMD_PLATFORM:
        return [PLATFORM_ESP32]
    if command == CMD_MCU:
        return [0x01]
    if command == CMD_INTERFACES:
        return [2, 0x11, 3, 0x21]
    return None


master_fd, slave_fd = pty.openpty()
tty.setraw(master_fd)
tty.setraw(slave_fd)
slave_path = os.ttyname(slave_fd)
state["port"] = slave_path
save_state()
pathlib.Path(port_path).write_text(slave_path, encoding="utf-8")
log(f"fake RNodeMulti PTY slave={slave_path}")

buffer = bytearray()
deadline = time.monotonic() + 300
while time.monotonic() < deadline:
    readable, _, _ = select.select([master_fd], [], [], 0.1)
    if not readable:
        continue
    try:
        chunk = os.read(master_fd, 4096)
    except OSError as exc:
        log(f"read error: {exc}")
        break
    if not chunk:
        continue
    buffer.extend(chunk)
    for frame in pop_frames(buffer):
        command = frame[0]
        payload = list(frame[1:])
        log(f"frame command=0x{command:02x} payload={payload}")
        response = command_response(command)
        with lock:
            if command == CMD_SEL_INT and payload:
                state["selected_vport"] = payload[0]
            if command == CMD_BLINK and payload == [3] and state.get("selected_vport") == 2:
                state["management_blink_seen"] = True
            state["frames"].append({"command": command, "payload": payload})
            save_state()
        if response is not None:
            with lock:
                state["probe_responses"].append({"command": command, "payload": response})
                save_state()
            os.write(master_fd, encode_frame(command, response))
            log(f"response command=0x{command:02x} payload={response}")
        if state.get("management_blink_seen"):
            deadline = time.monotonic() + 2

log("fake RNodeMulti PTY exiting")
PY
FAKE_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while [[ ! -s "$FAKE_PORT_FILE" ]]; do
  if (( SECONDS >= deadline )); then
    fail "timed out waiting for fake RNodeMulti PTY port"
  fi
  if ! kill -0 "$FAKE_PID" >/dev/null 2>&1; then
    fail "fake RNodeMulti PTY process exited before publishing port"
  fi
  sleep 0.1
done
FAKE_PORT="$(cat "$FAKE_PORT_FILE")"

python3 - <<'PY' "$CONFIG_PATH" "$FAKE_PORT" || fail "failed to generate RNodeMulti fake PTY config"
import pathlib
import sys

config_path, fake_port = sys.argv[1:3]
pathlib.Path(config_path).write_text(
    "\n".join(
        [
            "[[interfaces]]",
            'type = "RNodeMultiInterface"',
            "enabled = true",
            'name = "rnode-multi-fake-pty"',
            f'port = "{fake_port}"',
            "speed = 115200",
            'id_callsign = "FAKE-1"',
            "id_interval = 600",
            "radio0 = { name = \"fake-v2\", vport = 2, region = \"US915\", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }",
            "radio1 = { name = \"fake-v3\", vport = 3, region = \"US915\", frequency = 920000000, bandwidth = 125000, spreadingfactor = 10, codingrate = 5, txpower = 14, outgoing = false }",
        ]
    )
    + "\n",
    encoding="utf-8",
)
PY

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --bin rnodeconf-rs --quiet

"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$RPC_ADDR" \
  --rpc-unix "$RPC_UNIX" \
  --db "$DB_PATH" \
  --config "$CONFIG_PATH" \
  --strict-interface-startup >"$RETICULUMD_LOG" 2>&1 &
RET_PID=$!

while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before fake PTY RNodeMulti status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN"
import json
import sys

json_path, human_path = sys.argv[1:3]
payload = json.load(open(json_path, "r", encoding="utf-8"))
row = next(
    (
        item
        for item in payload.get("interfaces", [])
        if item.get("type") == "rnode_multi" and item.get("name") == "rnode-multi-fake-pty"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime_root = (row.get("settings") or {}).get("_runtime") or {}
if runtime_root.get("startup_status") != "spawned":
    raise SystemExit(1)
runtime_iface = runtime_root.get("runtime_iface") or runtime_root.get("iface")
if not isinstance(runtime_iface, str) or not runtime_iface:
    raise SystemExit(1)
radio = ((runtime_root.get("rnode_multi") or {}).get("radio_status") or {})
if radio.get("stream_state") != "running":
    raise SystemExit(1)
if radio.get("last_error") is not None:
    raise SystemExit(1)
if sorted(radio.get("vports") or []) != [2, 3]:
    raise SystemExit(1)
probe = radio.get("startup_probe") or {}
if probe.get("detected") is not True:
    raise SystemExit(1)
if (probe.get("firmware_version") or {}).get("label") != "1.74":
    raise SystemExit(1)
if probe.get("platform") != 128:
    raise SystemExit(1)
if probe.get("mcu") != 1:
    raise SystemExit(1)
if probe.get("interfaces") != {"2": "SX126X", "3": "SX128X"}:
    raise SystemExit(1)
if probe.get("interface_summary") != "2:SX126X,3:SX128X":
    raise SystemExit(1)
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
if "rnode-multi-fake-pty" not in human:
    raise SystemExit(1)
if "rnode_multi stream=running selected=2 vports=2" not in human:
    raise SystemExit(1)
if "detected=true" not in human or "fw=1.74" not in human:
    raise SystemExit(1)
if "probe=2:SX126X,3:SX128X" not in human:
    raise SystemExit(1)
PY
    then
      break
    fi
  fi
  sleep 1
done

if (( SECONDS >= deadline )); then
  fail "timed out waiting for healthy fake PTY RNodeMulti runtime status"
fi

"${ROOT_DIR}/target/debug/rnodeconf-rs" \
  --rpc "$RPC_ADDR" \
  blink \
  --interface rnode-multi-fake-pty \
  --vport 2 \
  --pattern 3 >"$RNodeCONF_JSON" 2>>"$RETICULUMD_LOG" || fail "rnodeconf-rs blink dispatch failed"

deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  if python3 - <<'PY' "$FAKE_STATE" "$RNodeCONF_JSON"
import json
import sys

state_path, rnodeconf_path = sys.argv[1:3]
state = json.load(open(state_path, "r", encoding="utf-8"))
result = json.load(open(rnodeconf_path, "r", encoding="utf-8"))
if result.get("queued") is not True:
    raise SystemExit(1)
if result.get("command") != "blink" or result.get("vport") != 2:
    raise SystemExit(1)
if state.get("management_blink_seen") is not True:
    raise SystemExit(1)
if state.get("pty_raw_mode") is not True:
    raise SystemExit(1)
frames = state.get("frames") or []
for idx, frame in enumerate(frames[:-1]):
    if frame.get("command") == 0x1F and frame.get("payload") == [2]:
        nxt = frames[idx + 1]
        if nxt.get("command") == 0x30 and nxt.get("payload") == [3]:
            raise SystemExit(0)
raise SystemExit(1)
PY
  then
    write_report "pass"
    echo "[rnode-multi-fake-pty-smoke] pass"
    echo "[rnode-multi-fake-pty-smoke] report=${REPORT_PATH}"
    echo "[rnode-multi-fake-pty-smoke] logs=${RUN_DIR}"
    exit 0
  fi
  sleep 1
done

fail "timed out waiting for fake PTY RNodeMulti vport blink management frame"
