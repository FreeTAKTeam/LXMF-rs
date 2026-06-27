#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/weave-fake-pty-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-weave-fake-pty.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
WEAVE_DISPLAY_JSON="${RUN_DIR}/weave-display.json"
WEAVE_DISPLAY_HUMAN="${RUN_DIR}/weave-display.txt"
WEAVECONF_ENABLE_JSON="${RUN_DIR}/weaveconf-enable.json"
WEAVECONF_DISABLE_JSON="${RUN_DIR}/weaveconf-disable.json"
FAKE_LOG="${RUN_DIR}/fake-weave.log"
FAKE_STATE="${RUN_DIR}/fake-weave-state.json"
FAKE_PORT_FILE="${RUN_DIR}/fake-weave.port"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$WEAVE_DISPLAY_JSON" "$WEAVE_DISPLAY_HUMAN" "$WEAVECONF_ENABLE_JSON" "$WEAVECONF_DISABLE_JSON" "$FAKE_LOG" "$FAKE_STATE"
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
    weave_display_json,
    weave_display_human,
    weaveconf_enable_json,
    weaveconf_disable_json,
    fake_log,
    fake_state,
) = sys.argv[1:16]
report = {
    "status": status,
    "reason": reason or None,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
    "reticulumd_log": reticulumd_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
    "weave_display_json": weave_display_json,
    "weave_display_human": weave_display_human,
    "weaveconf_enable_json": weaveconf_enable_json,
    "weaveconf_disable_json": weaveconf_disable_json,
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
                if item.get("type") == "weave" and item.get("name") == "weave-fake-pty"
            ),
            None,
        )
        if row:
            runtime = (row.get("settings") or {}).get("_runtime") or {}
            status_root = ((runtime.get("weave") or {}).get("status") or {})
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_iface"] = runtime.get("iface") or runtime.get("runtime_iface")
            report["link_state"] = status_root.get("link_state")
            report["wdcl_connected"] = status_root.get("wdcl_connected")
            report["remote_switch_id"] = status_root.get("remote_switch_id")
            report["local_endpoint_id"] = status_root.get("local_endpoint_id")
            report["endpoint_count"] = status_root.get("endpoint_count")
            report["last_error"] = status_root.get("last_error")
            report["bytes_rx"] = status_root.get("bytes_rx")
            report["bytes_tx"] = status_root.get("bytes_tx")
            report["frames_rx"] = status_root.get("frames_rx")
            report["frames_tx"] = status_root.get("frames_tx")
            report["last_log_event"] = status_root.get("last_log_event")
            report["display"] = status_root.get("display")
            report["device_stats"] = status_root.get("device_stats")
    except Exception as exc:
        report["status_parse_error"] = str(exc)
for key, path in [
    ("weave_display_report", weave_display_json),
    ("weaveconf_enable_result", weaveconf_enable_json),
    ("weaveconf_disable_result", weaveconf_disable_json),
    ("fake_peer", fake_state),
]:
    value_path = pathlib.Path(path)
    if value_path.exists():
        try:
            report[key] = json.loads(value_path.read_text(encoding="utf-8"))
        except Exception as exc:
            report[f"{key}_parse_error"] = str(exc)
for key, path in [
    ("human_summary", rnstatus_human),
    ("weave_display_human_summary", weave_display_human),
]:
    value_path = pathlib.Path(path)
    if value_path.exists():
        report[key] = value_path.read_text(encoding="utf-8", errors="replace")
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
    echo "[weave-fake-pty-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[weave-fake-pty-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - "$FAKE_PORT_FILE" "$FAKE_LOG" "$FAKE_STATE" <<'PY' &
import hashlib
import json
import os
import pathlib
import pty
import select
import sys
import termios
import threading
import time
import tty

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
except Exception as exc:
    pathlib.Path(sys.argv[1]).write_text("", encoding="utf-8")
    pathlib.Path(sys.argv[2]).write_text(f"missing cryptography Ed25519 support: {exc}\n", encoding="utf-8")
    raise

port_path, log_path, state_path = sys.argv[1:4]

HDLC_FLAG = 0x7E
HDLC_ESCAPE = 0x7D
HDLC_MASK = 0x20
WDCL_T_DISCOVER = 0x00
WDCL_T_CONNECT = 0x01
WDCL_T_CMD = 0x02
WDCL_T_LOG = 0x03
WDCL_T_DISP = 0x04
WDCL_BROADCAST = b"\xff\xff\xff\xff"
WDCL_CMD_REMOTE_DISPLAY = 0x0A00
ET_PROTO_WDCL_CONNECTION = 0x3002
ET_PROTO_WDCL_HOST_ENDPOINT = 0x3003
ET_PROTO_WEAVE_EP_ALIVE = 0x3102
ET_STAT_CPU = 0xE003
ET_STAT_TASK_CPU = 0xE004
ET_STAT_MEMORY = 0xE005

lock = threading.Lock()
state = {
    "port": None,
    "frames": [],
    "local_switch_id": None,
    "remote_switch_id": None,
    "discovery_response_sent": False,
    "connect_seen": False,
    "connection_log_sent": False,
    "display_frame_sent": False,
    "device_stats_sent": False,
    "remote_display_enable_seen": False,
    "remote_display_disable_seen": False,
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


def hdlc_encode(payload):
    out = bytearray([HDLC_FLAG])
    for value in payload:
        if value in (HDLC_FLAG, HDLC_ESCAPE):
            out.extend([HDLC_ESCAPE, value ^ HDLC_MASK])
        else:
            out.append(value)
    out.append(HDLC_FLAG)
    return bytes(out)


def hdlc_decode(frame):
    out = bytearray()
    started = False
    escape = False
    for value in frame:
        if escape:
            out.append(value ^ HDLC_MASK)
            escape = False
        elif value == HDLC_FLAG:
            if started:
                return bytes(out)
            started = True
        elif value == HDLC_ESCAPE:
            escape = True
        elif started:
            out.append(value)
    return None


def pop_frames(buffer):
    frames = []
    while True:
        try:
            start = buffer.index(HDLC_FLAG)
            end = buffer.index(HDLC_FLAG, start + 1)
        except ValueError:
            if len(buffer) > 8192:
                del buffer[:]
            return frames
        raw = bytes(buffer[start : end + 1])
        del buffer[: end + 1]
        decoded = hdlc_decode(raw)
        if decoded:
            frames.append(decoded)


def wdcl_frame(target, packet_type, payload=b""):
    return bytes(target) + bytes([packet_type]) + bytes(payload)


def write_frame(fd, target, packet_type, payload=b""):
    os.write(fd, hdlc_encode(wdcl_frame(target, packet_type, payload)))


def identity_from_name(name):
    first = hashlib.sha256(name.encode("utf-8")).digest()
    seed = hashlib.sha256(first).digest()
    key = Ed25519PrivateKey.from_private_bytes(seed)
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    return key, public, public[-4:]


remote_key, remote_public, remote_switch = identity_from_name("weave-fake-pty-remote")
endpoint = bytes([0x42, 0x23, 0x42, 0x23, 0x42, 0x23, 0x42, 0x23])

master_fd, slave_fd = pty.openpty()
tty.setraw(master_fd)
tty.setraw(slave_fd)
slave_path = os.ttyname(slave_fd)
state["port"] = slave_path
state["remote_switch_id"] = remote_switch.hex()
save_state()
pathlib.Path(port_path).write_text(slave_path, encoding="utf-8")
log(f"fake Weave PTY slave={slave_path} remote_switch={remote_switch.hex()}")

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
        if len(frame) < 5:
            continue
        target = frame[:4]
        packet_type = frame[4]
        payload = frame[5:]
        record = {"target": target.hex(), "packet_type": packet_type, "payload_hex": payload.hex()}
        if packet_type == WDCL_T_CMD and len(payload) >= 3:
            record["command"] = int.from_bytes(payload[:2], "big")
            record["command_value"] = payload[2]
        with lock:
            state["frames"].append(record)
            save_state()
        log(f"frame target={target.hex()} type=0x{packet_type:02x} payload={payload.hex()}")

        if target == WDCL_BROADCAST and packet_type == WDCL_T_DISCOVER and len(payload) == 4:
            local_switch = payload
            signature = remote_key.sign(local_switch)
            response_payload = remote_public + signature
            write_frame(master_fd, local_switch, WDCL_T_DISCOVER, response_payload)
            with lock:
                state["local_switch_id"] = local_switch.hex()
                state["discovery_response_sent"] = True
                save_state()
            log(f"sent discovery response target={local_switch.hex()}")
            continue

        local_switch_hex = state.get("local_switch_id")
        local_switch = bytes.fromhex(local_switch_hex) if local_switch_hex else None

        if target == remote_switch and packet_type == WDCL_T_CONNECT and local_switch:
            with lock:
                state["connect_seen"] = True
                save_state()
            log("observed daemon connect handshake")
            for event, data in [
                (ET_PROTO_WDCL_CONNECTION, b""),
                (ET_PROTO_WDCL_HOST_ENDPOINT, endpoint),
                (ET_PROTO_WEAVE_EP_ALIVE, endpoint),
                (ET_STAT_CPU, bytes([37])),
                (ET_STAT_TASK_CPU, bytes([11]) + b"fake-task\x00"),
                (ET_STAT_MEMORY, (4096).to_bytes(4, "big") + (8192).to_bytes(4, "big")),
            ]:
                payload = b"\x00\x00\x00\x00\x00\x00" + event.to_bytes(2, "big") + data
                write_frame(master_fd, local_switch, WDCL_T_LOG, payload)
            display_payload = (
                bytes([1])
                + (0).to_bytes(4, "big")
                + (4).to_bytes(4, "big")
                + bytes([0xAA, 0xBB, 0xCC, 0xDD])
            )
            write_frame(master_fd, local_switch, WDCL_T_DISP, display_payload)
            with lock:
                state["connection_log_sent"] = True
                state["display_frame_sent"] = True
                state["device_stats_sent"] = True
                save_state()
            log("sent connected/status/display frames")
            continue

        if target == remote_switch and packet_type == WDCL_T_CMD and len(payload) >= 3:
            command = int.from_bytes(payload[:2], "big")
            value = payload[2]
            if command == WDCL_CMD_REMOTE_DISPLAY and value == 1:
                with lock:
                    state["remote_display_enable_seen"] = True
                    save_state()
                log("observed remote display enable")
            if command == WDCL_CMD_REMOTE_DISPLAY and value == 0:
                with lock:
                    state["remote_display_disable_seen"] = True
                    save_state()
                log("observed remote display disable")
            if state.get("remote_display_enable_seen") and state.get("remote_display_disable_seen"):
                deadline = time.monotonic() + 2

log("fake Weave PTY exiting")
PY
FAKE_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while [[ ! -s "$FAKE_PORT_FILE" ]]; do
  if (( SECONDS >= deadline )); then
    fail "timed out waiting for fake Weave PTY port"
  fi
  if ! kill -0 "$FAKE_PID" >/dev/null 2>&1; then
    fail "fake Weave PTY process exited before publishing port"
  fi
  sleep 0.1
done
WEAVE_PORT="$(cat "$FAKE_PORT_FILE")"

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "WeaveInterface"
enabled = true
name = "weave-fake-pty"
port = "${WEAVE_PORT}"
speed = 3000000
mtu = 1024
configured_bitrate = 250000
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --bin weaveconf-rs --quiet

"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$RPC_ADDR" \
  --rpc-unix "$RPC_UNIX" \
  --db "$DB_PATH" \
  --config "$CONFIG_PATH" \
  --strict-interface-startup >"$RETICULUMD_LOG" 2>&1 &
RET_PID=$!

while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before fake Weave PTY status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --weave-display weave-fake-pty --json >"$WEAVE_DISPLAY_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --weave-display weave-fake-pty >"$WEAVE_DISPLAY_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$WEAVE_DISPLAY_JSON" "$WEAVE_DISPLAY_HUMAN" "$WEAVE_PORT"
import json
import sys

rnstatus_json, rnstatus_human, display_json, display_human, expected_port = sys.argv[1:6]
payload = json.load(open(rnstatus_json, "r", encoding="utf-8"))
row = next(
    (
        item
        for item in payload.get("interfaces", [])
        if item.get("type") == "weave" and item.get("name") == "weave-fake-pty"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime = (row.get("settings") or {}).get("_runtime") or {}
if runtime.get("startup_status") != "spawned":
    raise SystemExit(1)
runtime_iface = runtime.get("iface") or runtime.get("runtime_iface")
if not runtime_iface:
    raise SystemExit(1)
status = ((runtime.get("weave") or {}).get("status") or {})
if status.get("device") != expected_port:
    raise SystemExit(1)
if status.get("link_state") != "connected":
    raise SystemExit(1)
if status.get("wdcl_connected") is not True:
    raise SystemExit(1)
if status.get("last_error") is not None:
    raise SystemExit(1)
if not status.get("remote_switch_id"):
    raise SystemExit(1)
if not status.get("local_endpoint_id"):
    raise SystemExit(1)
if status.get("endpoint_count") != 1:
    raise SystemExit(1)
if (status.get("frames_tx") or 0) < 2 or (status.get("frames_rx") or 0) < 7:
    raise SystemExit(1)
display = status.get("display") or {}
if display.get("complete") is not True or display.get("buffer_hex") != "aabbccdd":
    raise SystemExit(1)
if display.get("received_size") != 4 or display.get("total_size") != 4:
    raise SystemExit(1)
stats = status.get("device_stats") or {}
if stats.get("cpu_load") != 37:
    raise SystemExit(1)
if stats.get("memory_free") != 4096 or stats.get("memory_total") != 8192:
    raise SystemExit(1)
if len(stats.get("task_cpu") or {}) != 1:
    raise SystemExit(1)
human = open(rnstatus_human, "r", encoding="utf-8", errors="replace").read()
if "weave-fake-pty" not in human:
    raise SystemExit(1)
if "weave link=connected endpoints=1 wdcl=true" not in human:
    raise SystemExit(1)
if "display=128x64/true" not in human or "cpu=37" not in human:
    raise SystemExit(1)
display_report = json.load(open(display_json, "r", encoding="utf-8"))
if display_report.get("interface") != "weave-fake-pty":
    raise SystemExit(1)
if (display_report.get("display") or {}).get("buffer_hex") != "aabbccdd":
    raise SystemExit(1)
display_text = open(display_human, "r", encoding="utf-8", errors="replace").read()
if "Weave Display: weave-fake-pty" not in display_text:
    raise SystemExit(1)
if "buffer_hex=aabbccdd" not in display_text:
    raise SystemExit(1)
PY
    then
      break
    fi
  fi
  sleep 1
done

if (( SECONDS >= deadline )); then
  fail "timed out waiting for healthy fake Weave PTY runtime status"
fi

"${ROOT_DIR}/target/debug/weaveconf-rs" \
  --rpc "$RPC_ADDR" \
  enable-remote-display \
  --interface weave-fake-pty >"$WEAVECONF_ENABLE_JSON" 2>>"$RETICULUMD_LOG" \
  || fail "weaveconf-rs enable-remote-display dispatch failed"

"${ROOT_DIR}/target/debug/weaveconf-rs" \
  --rpc "$RPC_ADDR" \
  disable-remote-display \
  --interface weave-fake-pty >"$WEAVECONF_DISABLE_JSON" 2>>"$RETICULUMD_LOG" \
  || fail "weaveconf-rs disable-remote-display dispatch failed"

deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  if python3 - <<'PY' "$FAKE_STATE" "$WEAVECONF_ENABLE_JSON" "$WEAVECONF_DISABLE_JSON"
import json
import sys

state_path, enable_path, disable_path = sys.argv[1:4]
state = json.load(open(state_path, "r", encoding="utf-8"))
enable = json.load(open(enable_path, "r", encoding="utf-8"))
disable = json.load(open(disable_path, "r", encoding="utf-8"))
if enable.get("queued") is not True or enable.get("enable") is not True:
    raise SystemExit(1)
if disable.get("queued") is not True or disable.get("enable") is not False:
    raise SystemExit(1)
if state.get("remote_display_enable_seen") is not True:
    raise SystemExit(1)
if state.get("remote_display_disable_seen") is not True:
    raise SystemExit(1)
commands = [
    (frame.get("command"), frame.get("command_value"))
    for frame in state.get("frames") or []
    if frame.get("packet_type") == 0x02
]
if (0x0A00, 1) not in commands or (0x0A00, 0) not in commands:
    raise SystemExit(1)
PY
  then
    write_report "pass"
    echo "[weave-fake-pty-smoke] pass"
    echo "[weave-fake-pty-smoke] report=${REPORT_PATH}"
    echo "[weave-fake-pty-smoke] logs=${RUN_DIR}"
    exit 0
  fi
  sleep 1
done

fail "timed out waiting for fake Weave PTY remote-display command frames"
