#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/kiss-fake-pty-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-kiss-fake-pty.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
FAKE_LOG="${RUN_DIR}/fake-kiss.log"
FAKE_STATE="${RUN_DIR}/fake-kiss-state.json"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_LOG" "$FAKE_STATE"
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
    fake_log,
    fake_state,
) = sys.argv[1:12]
report = {
    "status": status,
    "reason": reason or None,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
    "reticulumd_log": reticulumd_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
    "fake_log": fake_log,
    "fake_state": fake_state,
}
json_path = pathlib.Path(rnstatus_json)
if json_path.exists():
    try:
        payload = json.loads(json_path.read_text(encoding="utf-8"))
        summaries = {}
        for expected in ["kiss-fake-pty", "ax25-kiss-fake-pty"]:
            row = next(
                (
                    item
                    for item in payload.get("interfaces", [])
                    if item.get("name") == expected
                    and item.get("type") in {"kiss", "ax25_kiss"}
                ),
                None,
            )
            if row:
                runtime = (row.get("settings") or {}).get("_runtime") or {}
                status_root = ((runtime.get("kiss") or {}).get("status") or {})
                summaries[expected] = {
                    "type": row.get("type"),
                    "startup_status": runtime.get("startup_status"),
                    "runtime_iface": runtime.get("iface") or runtime.get("runtime_iface"),
                    "status": status_root,
                }
        report["interfaces"] = summaries
    except Exception as exc:
        report["status_parse_error"] = str(exc)
state_path = pathlib.Path(fake_state)
if state_path.exists():
    try:
        report["fake_peer"] = json.loads(state_path.read_text(encoding="utf-8"))
    except Exception as exc:
        report["fake_state_parse_error"] = str(exc)
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
    echo "[kiss-fake-pty-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[kiss-fake-pty-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - "$FAKE_LOG" "$FAKE_STATE" <<'PY' &
import json
import os
import pathlib
import pty
import select
import sys
import time
import tty

log_path, state_path = sys.argv[1:3]

FEND = 0xC0
FESC = 0xDB
TFEND = 0xDC
TFESC = 0xDD
CMD_TXDELAY = 0x01
CMD_P = 0x02
CMD_SLOTTIME = 0x03
CMD_TXTAIL = 0x04
CMD_READY = 0x0F

EXPECTED_INIT = [CMD_TXDELAY, CMD_TXTAIL, CMD_P, CMD_SLOTTIME, CMD_READY]
PEERS = ["kiss", "ax25"]

state = {
    "peers": {},
}
fd_to_label = {}


def save_state():
    pathlib.Path(state_path).write_text(
        json.dumps(state, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def log(message):
    with open(log_path, "a", encoding="utf-8") as handle:
        handle.write(message + "\n")
        handle.flush()


def encode_frame(command, payload):
    out = bytearray([FEND, command])
    for value in payload:
        if value == FEND:
            out.extend([FESC, TFEND])
        elif value == FESC:
            out.extend([FESC, TFESC])
        else:
            out.append(value)
    out.append(FEND)
    return bytes(out)


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
        command = raw[0]
        decoded = bytearray()
        escape = False
        for value in raw[1:]:
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
        frames.append((command, list(decoded)))


for label in PEERS:
    master_fd, slave_fd = pty.openpty()
    tty.setraw(master_fd)
    tty.setraw(slave_fd)
    port = os.ttyname(slave_fd)
    fd_to_label[master_fd] = label
    state["peers"][label] = {
        "port": port,
        "frames": [],
        "init_commands_seen": [],
        "ready_response_sent": False,
        "pty_raw_mode": True,
    }
    log(f"fake {label} KISS PTY slave={port}")

save_state()

buffers = {fd: bytearray() for fd in fd_to_label}
deadline = time.monotonic() + 300
while time.monotonic() < deadline:
    readable, _, _ = select.select(list(fd_to_label), [], [], 0.1)
    if not readable:
        continue
    for fd in readable:
        label = fd_to_label[fd]
        try:
            chunk = os.read(fd, 4096)
        except OSError as exc:
            log(f"{label}: read error: {exc}")
            continue
        if not chunk:
            continue
        buffers[fd].extend(chunk)
        for command, payload in pop_frames(buffers[fd]):
            peer = state["peers"][label]
            peer["frames"].append({"command": command, "payload": payload})
            if command in EXPECTED_INIT:
                peer["init_commands_seen"].append(command)
            log(f"{label}: frame command=0x{command:02x} payload={payload}")
            if command == CMD_READY and not peer["ready_response_sent"]:
                os.write(fd, encode_frame(CMD_READY, [1]))
                peer["ready_response_sent"] = True
                log(f"{label}: sent READY")
            save_state()
    if all(state["peers"][label]["ready_response_sent"] for label in PEERS):
        deadline = time.monotonic() + 5

log("fake KISS PTYs exiting")
PY
FAKE_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while true; do
  if [[ -s "$FAKE_STATE" ]] && python3 - <<'PY' "$FAKE_STATE"
import json
import sys
state = json.load(open(sys.argv[1], "r", encoding="utf-8"))
peers = state.get("peers") or {}
if all((peers.get(label) or {}).get("port") for label in ["kiss", "ax25"]):
    raise SystemExit(0)
raise SystemExit(1)
PY
  then
    break
  fi
  if (( SECONDS >= deadline )); then
    fail "timed out waiting for fake KISS PTY ports"
  fi
  if ! kill -0 "$FAKE_PID" >/dev/null 2>&1; then
    fail "fake KISS PTY process exited before publishing ports"
  fi
  sleep 0.1
done

KISS_PORT="$(
  python3 - <<'PY' "$FAKE_STATE"
import json
import sys
print(json.load(open(sys.argv[1], "r", encoding="utf-8"))["peers"]["kiss"]["port"])
PY
)"
AX25_PORT="$(
  python3 - <<'PY' "$FAKE_STATE"
import json
import sys
print(json.load(open(sys.argv[1], "r", encoding="utf-8"))["peers"]["ax25"]["port"])
PY
)"

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "KISSInterface"
enabled = true
name = "kiss-fake-pty"
port = "${KISS_PORT}"
speed = 9600
preamble = 350
txtail = 20
persistence = 64
slottime = 20
flow_control = true
id_callsign = "FAKE-0"
id_interval = 600

[[interfaces]]
type = "AX25KISSInterface"
enabled = true
name = "ax25-kiss-fake-pty"
port = "${AX25_PORT}"
speed = 1200
callsign = "N0CALL"
ssid = 1
preamble = 350
txtail = 20
persistence = 64
slottime = 20
flow_control = true
id_callsign = "N0CALL-1"
id_interval = 600
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet

"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$RPC_ADDR" \
  --rpc-unix "$RPC_UNIX" \
  --db "$DB_PATH" \
  --config "$CONFIG_PATH" \
  --strict-interface-startup >"$RETICULUMD_LOG" 2>&1 &
RET_PID=$!

while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before fake KISS PTY status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_STATE" "$KISS_PORT" "$AX25_PORT"
import json
import sys

json_path, human_path, fake_state_path, kiss_port, ax25_port = sys.argv[1:6]
payload = json.load(open(json_path, "r", encoding="utf-8"))
fake_state = json.load(open(fake_state_path, "r", encoding="utf-8"))
human = open(human_path, "r", encoding="utf-8", errors="replace").read()

expectations = {
    "kiss-fake-pty": {
        "type": "kiss",
        "device": kiss_port,
        "baud_rate": 9600,
        "ax25": False,
        "fake_label": "kiss",
    },
    "ax25-kiss-fake-pty": {
        "type": "ax25_kiss",
        "device": ax25_port,
        "baud_rate": 1200,
        "ax25": True,
        "fake_label": "ax25",
    },
}

for name, expected in expectations.items():
    row = next(
        (
            item
            for item in payload.get("interfaces", [])
            if item.get("name") == name and item.get("type") == expected["type"]
        ),
        None,
    )
    if row is None:
        raise SystemExit(1)
    runtime = (row.get("settings") or {}).get("_runtime") or {}
    if runtime.get("startup_status") != "spawned":
        raise SystemExit(1)
    status = ((runtime.get("kiss") or {}).get("status") or {})
    if status.get("link_state") != "running":
        raise SystemExit(1)
    if status.get("bearer") != "serial":
        raise SystemExit(1)
    if status.get("device") != expected["device"]:
        raise SystemExit(1)
    if status.get("baud_rate") != expected["baud_rate"]:
        raise SystemExit(1)
    if status.get("kiss_flow_control") is not True:
        raise SystemExit(1)
    if status.get("ax25") is not expected["ax25"]:
        raise SystemExit(1)
    if status.get("interface_ready") is not True:
        raise SystemExit(1)
    if (status.get("init_frames_tx") or 0) < 5:
        raise SystemExit(1)
    if (status.get("bytes_tx") or 0) < 20:
        raise SystemExit(1)
    if (status.get("bytes_rx") or 0) < 4:
        raise SystemExit(1)
    if (status.get("command_frames_rx") or 0) < 1:
        raise SystemExit(1)
    if (status.get("ready_frames_rx") or 0) < 1:
        raise SystemExit(1)
    if status.get("last_error") is not None:
        raise SystemExit(1)
    peer = (fake_state.get("peers") or {}).get(expected["fake_label"]) or {}
    if peer.get("ready_response_sent") is not True:
        raise SystemExit(1)
    if peer.get("pty_raw_mode") is not True:
        raise SystemExit(1)
    if sorted(set(peer.get("init_commands_seen") or [])) != [1, 2, 3, 4, 15]:
        raise SystemExit(1)
    if name not in human:
        raise SystemExit(1)
    if f"kiss state=running bearer=serial device={expected['device']}" not in human:
        raise SystemExit(1)
    if "flow=true" not in human or "ready=true" not in human:
        raise SystemExit(1)
    if "cmd_rx=1" not in human or "ready_rx=1" not in human or "init_tx=5" not in human:
        raise SystemExit(1)
if "ax25=true" not in human:
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[kiss-fake-pty-smoke] pass"
      echo "[kiss-fake-pty-smoke] report=${REPORT_PATH}"
      echo "[kiss-fake-pty-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for healthy fake KISS PTY runtime status"
