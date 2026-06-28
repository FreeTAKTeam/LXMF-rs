#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-60}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/rnode-fake-tcp-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
FAKE_LOG="${RUN_DIR}/fake-rnode.log"
FAKE_STATE="${RUN_DIR}/fake-rnode-state.json"
FAKE_PORT_FILE="${RUN_DIR}/fake-rnode.port"
PREPARED_LOG_DIR="${RUN_DIR}/prepared-host"
PREPARED_REPORT="${RUN_DIR}/prepared-host-report.json"

: >"$FAKE_LOG"

write_report() {
  local status="$1"
  local reason="${2:-}"
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RUN_DIR" "$PREPARED_REPORT" "$FAKE_LOG" "$FAKE_STATE"
import json
import pathlib
import sys

report_path, status, reason, run_dir, prepared_report, fake_log, fake_state = sys.argv[1:8]
report = {
    "status": status,
    "evidence_scope": "software_fake_tcp_rnode_prepared_host_management",
    "product_boundary": (
        "This proves the ordinary TCP RNode daemon, status, and safe-management "
        "prepared-host path against a local fake KISS TCP peer; it is not prepared hardware "
        "or broad RNode production parity."
    ),
    "reason": reason or None,
    "run_dir": run_dir,
    "prepared_host_report": prepared_report,
    "fake_log": fake_log,
    "fake_state": fake_state,
    "expected_prepared_host_artifacts": [
        "rnodeconf-query-radio-state.json",
        "rnodeconf-blink.json",
        "rnstatus-post-management.json",
    ],
}
prepared_path = pathlib.Path(prepared_report)
if prepared_path.exists():
    try:
        report["prepared_host"] = json.loads(prepared_path.read_text(encoding="utf-8"))
    except Exception as exc:
        report["prepared_host_parse_error"] = str(exc)
state_path = pathlib.Path(fake_state)
if state_path.exists():
    try:
        report["fake_peer"] = json.loads(state_path.read_text(encoding="utf-8"))
    except Exception as exc:
        report["fake_state_parse_error"] = str(exc)
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

cleanup() {
  local status=$?
  if [[ -n "${FAKE_PID:-}" ]]; then
    kill "$FAKE_PID" >/dev/null 2>&1 || true
    wait "$FAKE_PID" >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[rnode-fake-tcp-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[rnode-fake-tcp-smoke] ERROR: ${msg}" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - "$FAKE_PORT_FILE" "$FAKE_LOG" "$FAKE_STATE" <<'PY' &
import json
import socketserver
import sys
import threading

port_path, log_path, state_path = sys.argv[1:4]

FEND = 0xC0
FESC = 0xDB
TFEND = 0xDC
TFESC = 0xDD
CMD_FREQUENCY = 0x01
CMD_BANDWIDTH = 0x02
CMD_TXPOWER = 0x03
CMD_SF = 0x04
CMD_CR = 0x05
CMD_RADIO_STATE = 0x06
CMD_DETECT = 0x08
CMD_LEAVE = 0x0A
CMD_BLINK = 0x30
CMD_FB_EXT = 0x41
CMD_PLATFORM = 0x48
CMD_MCU = 0x49
CMD_FW_VERSION = 0x50
DETECT_RESP = 0x46
PLATFORM_ESP32 = 0x80
RADIO_STATE_ON = 0x01
RADIO_STATE_ASK = 0xFF

lock = threading.Lock()
state = {
    "connections": 0,
    "frames": [],
    "probe_responses": [],
    "radio_query_seen": False,
    "management_blink_seen": False,
    "shutdown_seen": False,
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


def decode_frames(buffer):
    frames = []
    current = bytearray()
    in_frame = False
    escape = False
    for value in buffer:
        if value == FEND:
            if in_frame and current:
                frames.append(bytes(current))
            current = bytearray()
            in_frame = True
            escape = False
            continue
        if not in_frame:
            continue
        if escape:
            if value == TFEND:
                current.append(FEND)
            elif value == TFESC:
                current.append(FESC)
            else:
                current.append(value)
            escape = False
            continue
        if value == FESC:
            escape = True
        else:
            current.append(value)
    return frames


def command_response(command, payload):
    if command == CMD_DETECT:
        return [DETECT_RESP]
    if command == CMD_FW_VERSION:
        return [1, 74]
    if command == CMD_PLATFORM:
        return [PLATFORM_ESP32]
    if command == CMD_MCU:
        return [0x01]
    if command in {CMD_FREQUENCY, CMD_BANDWIDTH, CMD_TXPOWER, CMD_SF, CMD_CR}:
        return payload
    if command == CMD_RADIO_STATE:
        if payload == [RADIO_STATE_ASK]:
            return [RADIO_STATE_ON]
        return payload
    return None


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        with lock:
            state["connections"] += 1
            save_state()
        log(f"connection {self.client_address}")
        self.request.settimeout(0.5)
        buffer = bytearray()
        while True:
            try:
                chunk = self.request.recv(4096)
            except TimeoutError:
                continue
            except OSError:
                return
            if not chunk:
                return
            buffer.extend(chunk)
            frames = decode_frames(buffer)
            if buffer and buffer[-1] == FEND:
                buffer = bytearray()
            for frame in frames:
                if not frame:
                    continue
                command = frame[0]
                payload = list(frame[1:])
                log(f"frame command=0x{command:02x} payload={payload}")
                response = command_response(command, payload)
                with lock:
                    if command == CMD_RADIO_STATE and payload == [RADIO_STATE_ASK]:
                        state["radio_query_seen"] = True
                    if command == CMD_BLINK and payload == [3]:
                        state["management_blink_seen"] = True
                    if command == CMD_LEAVE:
                        state["shutdown_seen"] = True
                    state["frames"].append({"command": command, "payload": payload})
                    save_state()
                if response is not None:
                    with lock:
                        state["probe_responses"].append({"command": command, "payload": response})
                        save_state()
                    self.request.sendall(encode_frame(command, response))
                    log(f"response command=0x{command:02x} payload={response}")


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True


server = Server(("127.0.0.1", 0), Handler)
with open(port_path, "w", encoding="utf-8") as handle:
    handle.write(str(server.server_address[1]))
save_state()
server.serve_forever()
PY
FAKE_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while [[ ! -s "$FAKE_PORT_FILE" ]]; do
  if (( SECONDS >= deadline )); then
    fail "timed out waiting for fake RNode TCP server port"
  fi
  sleep 0.1
done
FAKE_PORT="$(cat "$FAKE_PORT_FILE")"

if ! LOG_DIR="$PREPARED_LOG_DIR" \
  REPORT_PATH="$PREPARED_REPORT" \
  RNODE_PORT="tcp://127.0.0.1:${FAKE_PORT}" \
  RNODE_FREQUENCY=915000000 \
  RNODE_BANDWIDTH=125000 \
  RNODE_SPREADING_FACTOR=9 \
  RNODE_CODING_RATE=5 \
  RNODE_TX_POWER=17 \
  RNODE_COMMAND_TIMEOUT_MS=1500 \
  RNODE_MANAGEMENT_TIMEOUT_SECS=10 \
  RNODE_BLINK_PATTERN=3 \
  RNODE_TIMEOUT_SECS="$TIMEOUT_SECS" \
  ./tools/scripts/rnode-prepared-host-smoke.sh; then
  fail "prepared-host smoke failed against fake TCP RNode"
fi

python3 - <<'PY' "$PREPARED_REPORT" "$FAKE_STATE" || fail "fake TCP RNode evidence was invalid"
import json
import sys

prepared_path, state_path = sys.argv[1:3]
prepared = json.load(open(prepared_path, "r", encoding="utf-8"))
state = json.load(open(state_path, "r", encoding="utf-8"))
if prepared.get("status") != "pass":
    raise SystemExit(1)
if prepared.get("evidence_scope") != "prepared_host_tcp_rnode":
    raise SystemExit(1)
if prepared.get("transport_kind") != "tcp":
    raise SystemExit(1)
if prepared.get("online") is not True or prepared.get("radio_state") != 1:
    raise SystemExit(1)
management = prepared.get("management_commands") or []
commands = {item.get("command"): item for item in management}
if commands.get("radio_state_query", {}).get("queued") is not True:
    raise SystemExit(1)
if commands.get("blink", {}).get("queued") is not True:
    raise SystemExit(1)
post = prepared.get("post_management_status") or {}
if post.get("online") is not True or post.get("radio_state") != 1:
    raise SystemExit(1)
if post.get("last_command_error") is not None:
    raise SystemExit(1)
if post.get("hardware_errors") not in (None, []):
    raise SystemExit(1)
if state.get("radio_query_seen") is not True:
    raise SystemExit(1)
if state.get("management_blink_seen") is not True:
    raise SystemExit(1)
frames = state.get("frames") or []
expected = [
    (0x08, [0x73]),
    (0x50, [0x00]),
    (0x48, [0x00]),
    (0x49, [0x00]),
    (0x01, [0x36, 0x89, 0xCA, 0xC0]),
    (0x02, [0x00, 0x01, 0xE8, 0x48]),
    (0x03, [17]),
    (0x04, [9]),
    (0x05, [5]),
    (0x06, [1]),
    (0x06, [0xFF]),
    (0x30, [3]),
]
seen = [(frame.get("command"), frame.get("payload")) for frame in frames]
cursor = 0
for wanted in expected:
    try:
        index = seen.index(wanted, cursor)
    except ValueError as exc:
        raise SystemExit(1) from exc
    cursor = index + 1
PY

write_report "pass"
echo "[rnode-fake-tcp-smoke] pass"
echo "[rnode-fake-tcp-smoke] report=${REPORT_PATH}"
echo "[rnode-fake-tcp-smoke] logs=${RUN_DIR}"
