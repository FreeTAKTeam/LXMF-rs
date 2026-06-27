#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/kiss-fake-tcp-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-kiss-fake-tcp.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
FAKE_LOG="${RUN_DIR}/fake-kiss-tcp.log"
FAKE_STATE="${RUN_DIR}/fake-kiss-tcp-state.json"

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
        row = next(
            (
                item
                for item in payload.get("interfaces", [])
                if item.get("name") == "kiss-fake-tcp"
                and item.get("type") == "kiss_tcp_client"
            ),
            None,
        )
        if row:
            runtime = (row.get("settings") or {}).get("_runtime") or {}
            report["interface"] = {
                "type": row.get("type"),
                "startup_status": runtime.get("startup_status"),
                "runtime_iface": runtime.get("iface") or runtime.get("runtime_iface"),
                "status": ((runtime.get("kiss_tcp") or {}).get("status") or {}),
            }
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
    echo "[kiss-fake-tcp-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[kiss-fake-tcp-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - "$FAKE_LOG" "$FAKE_STATE" <<'PY' &
import json
import pathlib
import select
import socket
import sys
import time

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

state = {
    "host": "127.0.0.1",
    "port": None,
    "accepted_connections": 0,
    "closed_connections": 0,
    "frames": [],
    "init_commands_seen": [],
    "ready_response_sent": False,
}


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


listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", 0))
listener.listen()
listener.setblocking(False)
state["port"] = listener.getsockname()[1]
save_state()
log(f"fake KISS TCP listening on 127.0.0.1:{state['port']}")

clients = {}
deadline = time.monotonic() + 300
while time.monotonic() < deadline:
    sockets = [listener, *clients.keys()]
    readable, _, _ = select.select(sockets, [], [], 0.1)
    for sock in readable:
        if sock is listener:
            conn, addr = listener.accept()
            conn.setblocking(False)
            clients[conn] = bytearray()
            state["accepted_connections"] += 1
            log(f"accepted connection {state['accepted_connections']} from {addr[0]}:{addr[1]}")
            save_state()
            continue
        try:
            chunk = sock.recv(4096)
        except OSError as exc:
            log(f"read error: {exc}")
            chunk = b""
        if not chunk:
            clients.pop(sock, None)
            try:
                sock.close()
            except OSError:
                pass
            state["closed_connections"] += 1
            save_state()
            continue
        buffer = clients[sock]
        buffer.extend(chunk)
        for command, payload in pop_frames(buffer):
            state["frames"].append({"command": command, "payload": payload})
            if command in EXPECTED_INIT:
                state["init_commands_seen"].append(command)
            log(f"frame command=0x{command:02x} payload={payload}")
            if command == CMD_READY and not state["ready_response_sent"]:
                sock.sendall(encode_frame(CMD_READY, [1]))
                state["ready_response_sent"] = True
                log("sent READY")
            save_state()
    if state["ready_response_sent"]:
        deadline = min(deadline, time.monotonic() + 10)

log("fake KISS TCP exiting")
save_state()
PY
FAKE_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while true; do
  if [[ -s "$FAKE_STATE" ]] && python3 - <<'PY' "$FAKE_STATE"
import json
import sys
state = json.load(open(sys.argv[1], "r", encoding="utf-8"))
if state.get("port"):
    raise SystemExit(0)
raise SystemExit(1)
PY
  then
    break
  fi
  if (( SECONDS >= deadline )); then
    fail "timed out waiting for fake KISS TCP listener"
  fi
  if ! kill -0 "$FAKE_PID" >/dev/null 2>&1; then
    fail "fake KISS TCP process exited before publishing listener"
  fi
  sleep 0.1
done

KISS_TCP_PORT="$(
  python3 - <<'PY' "$FAKE_STATE"
import json
import sys
print(json.load(open(sys.argv[1], "r", encoding="utf-8"))["port"])
PY
)"
KISS_TCP_ENDPOINT="127.0.0.1:${KISS_TCP_PORT}"

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "TCPClientInterface"
enabled = true
name = "kiss-fake-tcp"
target_host = "127.0.0.1"
target_port = ${KISS_TCP_PORT}
kiss_framing = true
fixed_mtu = 512
preamble_ms = 350
tx_tail_ms = 20
persistence = 64
slot_time_ms = 20
flow_control = true
id_callsign = "TCPFAKE-0"
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
    fail "reticulumd exited before fake KISS TCP status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_STATE" "$KISS_TCP_ENDPOINT"
import json
import sys

json_path, human_path, fake_state_path, endpoint = sys.argv[1:5]
payload = json.load(open(json_path, "r", encoding="utf-8"))
fake_state = json.load(open(fake_state_path, "r", encoding="utf-8"))
human = open(human_path, "r", encoding="utf-8", errors="replace").read()

row = next(
    (
        item
        for item in payload.get("interfaces", [])
        if item.get("name") == "kiss-fake-tcp"
        and item.get("type") == "kiss_tcp_client"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime = (row.get("settings") or {}).get("_runtime") or {}
if runtime.get("startup_status") != "spawned":
    raise SystemExit(1)
status = ((runtime.get("kiss_tcp") or {}).get("status") or {})
if status.get("link_state") != "running":
    raise SystemExit(1)
if status.get("bearer") != "tcp":
    raise SystemExit(1)
if status.get("endpoint") != endpoint:
    raise SystemExit(1)
if status.get("device") is not None:
    raise SystemExit(1)
if status.get("mtu") != 512:
    raise SystemExit(1)
if status.get("preamble_ms") != 350:
    raise SystemExit(1)
if status.get("tx_tail_ms") != 20:
    raise SystemExit(1)
if status.get("persistence") != 64:
    raise SystemExit(1)
if status.get("slot_time_ms") != 20:
    raise SystemExit(1)
if status.get("kiss_flow_control") is not True:
    raise SystemExit(1)
if status.get("ax25") is not False:
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
if (fake_state.get("accepted_connections") or 0) < 2:
    raise SystemExit(1)
if fake_state.get("ready_response_sent") is not True:
    raise SystemExit(1)
if sorted(set(fake_state.get("init_commands_seen") or [])) != [1, 2, 3, 4, 15]:
    raise SystemExit(1)
if "kiss-fake-tcp" not in human:
    raise SystemExit(1)
if f"kiss state=running bearer=tcp endpoint={endpoint}" not in human:
    raise SystemExit(1)
if "flow=true" not in human or "ready=true" not in human:
    raise SystemExit(1)
if "cmd_rx=1" not in human or "ready_rx=1" not in human or "init_tx=5" not in human:
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[kiss-fake-tcp-smoke] pass"
      echo "[kiss-fake-tcp-smoke] report=${REPORT_PATH}"
      echo "[kiss-fake-tcp-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for healthy fake KISS TCP runtime status"
