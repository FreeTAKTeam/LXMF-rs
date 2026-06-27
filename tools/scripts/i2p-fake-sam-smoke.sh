#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

I2P_PEERS="${I2P_PEERS:-peer-one.b32.i2p}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"
if [[ -z "$I2P_PEERS" ]]; then
  I2P_PEERS="peer-one.b32.i2p"
fi
if [[ -z "$TIMEOUT_SECS" ]]; then
  TIMEOUT_SECS="60"
fi

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/i2p-fake-sam-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-i2p-fake-sam.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
FAKE_SAM_LOG="${RUN_DIR}/fake-sam.log"
FAKE_SAM_PORT_FILE="${RUN_DIR}/fake-sam.port"

: >"$RETICULUMD_LOG"
: >"$FAKE_SAM_LOG"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$SAM_ENDPOINT" "$I2P_PEERS" "$RPC_ADDR" "$RUN_DIR" "$RETICULUMD_LOG" "$FAKE_SAM_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    sam_endpoint,
    peers_raw,
    rpc_addr,
    run_dir,
    reticulumd_log,
    fake_sam_log,
    rnstatus_json,
    rnstatus_human,
) = sys.argv[1:12]
expected_peers = [item.strip() for item in peers_raw.split(",") if item.strip()]
report = {
    "status": status,
    "reason": reason or None,
    "sam_endpoint": sam_endpoint,
    "expected_outbound_peers": expected_peers,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "reticulumd_log": reticulumd_log,
    "fake_sam_log": fake_sam_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
}
json_path = pathlib.Path(rnstatus_json)
if json_path.exists():
    try:
        payload = json.loads(json_path.read_text(encoding="utf-8"))
        rows = payload.get("interfaces") or []
        i2p = next(
            (
                row
                for row in rows
                if row.get("type") == "i2p" and row.get("name") == "i2p-fake-sam"
            ),
            None,
        )
        if i2p:
            runtime = ((i2p.get("settings") or {}).get("_runtime") or {}).get("i2p") or {}
            tunnel = runtime.get("tunnel_status") or {}
            peer_rows = tunnel.get("peers") or []
            report["startup_status"] = ((i2p.get("settings") or {}).get("_runtime") or {}).get(
                "startup_status"
            )
            report["reachable_endpoint"] = runtime.get("reachable_endpoint")
            report["private_key_persisted"] = runtime.get("private_key_persisted")
            report["accept_state"] = tunnel.get("accept_state")
            report["configured_peer_count"] = tunnel.get("configured_peer_count")
            report["peer_rows"] = peer_rows
            report["connected_outbound_peers"] = [
                row.get("peer")
                for row in peer_rows
                if row.get("direction") == "outbound" and row.get("state") == "connected"
            ]
            report["recovered_outbound_peers"] = [
                row.get("peer")
                for row in peer_rows
                if row.get("direction") == "outbound"
                and row.get("state") == "connected"
                and (row.get("reconnect_attempts") or 0) >= 1
                and row.get("last_error") is None
            ]
            report["connected_incoming_peers"] = [
                row.get("peer")
                for row in peer_rows
                if row.get("direction") == "incoming" and row.get("state") == "connected"
            ]
    except Exception as exc:
        report["status_parse_error"] = str(exc)
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
  if [[ -n "${FAKE_SAM_PID:-}" ]]; then
    kill "$FAKE_SAM_PID" >/dev/null 2>&1 || true
    wait "$FAKE_SAM_PID" >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[i2p-fake-sam-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[i2p-fake-sam-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - "$FAKE_SAM_PORT_FILE" "$FAKE_SAM_LOG" <<'PY' &
import base64
import re
import socket
import socketserver
import sys
import threading

port_path, log_path = sys.argv[1:3]
lock = threading.Lock()
accepted_streams = 0
naming_lookups = {}


def log(message):
    with lock:
        with open(log_path, "a", encoding="utf-8") as handle:
            handle.write(message + "\n")
            handle.flush()


def fake_private_destination():
    private = bytearray(range(256)) + bytearray(range(244))
    private[385] = 0
    private[386] = 3
    return base64.b64encode(private).decode("ascii").replace("+", "-").replace("/", "~")


PRIVATE_DESTINATION = fake_private_destination()
PUBLIC_DESTINATION = "fake-public-destination"


def safe_peer_value(name):
    return "resolved-" + re.sub(r"[^A-Za-z0-9_.~-]", "-", name)


class SamHandler(socketserver.StreamRequestHandler):
    def readline_text(self):
        raw = self.rfile.readline()
        if not raw:
            return ""
        line = raw.decode("utf-8", errors="replace").strip()
        log(f"recv {line}")
        return line

    def write_line(self, line):
        log(f"send {line}")
        self.wfile.write((line + "\n").encode("utf-8"))
        self.wfile.flush()

    def drain(self):
        try:
            while self.request.recv(4096):
                pass
        except OSError:
            pass

    def handle(self):
        first = self.readline_text()
        if not first:
            return
        if first.startswith("HELLO VERSION"):
            self.write_line("HELLO REPLY RESULT=OK VERSION=3.3")
            command = self.readline_text()
        else:
            command = first
        if not command:
            return
        if command == "DEST GENERATE SIGNATURE_TYPE=7":
            self.write_line(f"DEST REPLY PUB={PUBLIC_DESTINATION} PRIV={PRIVATE_DESTINATION}")
        elif command.startswith("SESSION CREATE "):
            destination = "fake-session-destination"
            for token in command.split():
                if token.startswith("DESTINATION="):
                    destination = token.split("=", 1)[1]
                    break
            if destination == "TRANSIENT":
                destination = "fake-transient-destination"
            self.write_line(f"SESSION STATUS RESULT=OK DESTINATION={destination}")
            self.drain()
        elif command.startswith("NAMING LOOKUP NAME="):
            name = command.split("NAME=", 1)[1]
            with lock:
                lookup_count = naming_lookups.get(name, 0) + 1
                naming_lookups[name] = lookup_count
            if lookup_count == 1:
                self.write_line(
                    f"NAMING REPLY RESULT=KEY_NOT_FOUND NAME={name} MESSAGE=transient-lookup-failure"
                )
                return
            self.write_line(f"NAMING REPLY RESULT=OK NAME={name} VALUE={safe_peer_value(name)}")
        elif command.startswith("STREAM CONNECT "):
            self.write_line("STREAM STATUS RESULT=OK")
            self.drain()
        elif command.startswith("STREAM ACCEPT "):
            global accepted_streams
            with lock:
                accepted_streams += 1
                accept_index = accepted_streams
            if accept_index > 1:
                self.drain()
                return
            self.write_line("STREAM STATUS RESULT=OK")
            self.write_line("fake-remote-destination")
            self.drain()
        else:
            self.write_line("SESSION STATUS RESULT=I2P_ERROR MESSAGE=unsupported-command")


class ThreadingServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


with ThreadingServer(("127.0.0.1", 0), SamHandler) as server:
    host, port = server.server_address
    with open(port_path, "w", encoding="utf-8") as handle:
        handle.write(str(port))
    log(f"listening {host}:{port}")
    server.serve_forever()
PY
FAKE_SAM_PID=$!

for _ in {1..100}; do
  if [[ -s "$FAKE_SAM_PORT_FILE" ]]; then
    break
  fi
  sleep 0.05
done
if [[ ! -s "$FAKE_SAM_PORT_FILE" ]]; then
  fail "fake SAM did not publish a port"
fi
SAM_PORT="$(cat "$FAKE_SAM_PORT_FILE")"
SAM_ENDPOINT="127.0.0.1:${SAM_PORT}"

python3 - <<'PY' "$SAM_ENDPOINT" || fail "fake SAM endpoint ${SAM_ENDPOINT} did not complete HELLO"
import socket
import sys

host, port = sys.argv[1].rsplit(":", 1)
with socket.create_connection((host, int(port)), timeout=5) as sock:
    sock.settimeout(5)
    sock.sendall(b"HELLO VERSION MIN=3.0 MAX=3.3\n")
    response = b""
    while not response.endswith(b"\n"):
        chunk = sock.recv(1)
        if not chunk:
            break
        response += chunk
text = response.decode("utf-8", errors="replace")
if "HELLO REPLY" not in text or "RESULT=OK" not in text:
    raise SystemExit(f"unexpected SAM HELLO response: {text!r}")
PY

python3 - <<'PY' "$CONFIG_PATH" "$SAM_PORT" "$RUN_DIR" "$I2P_PEERS" || fail "failed to generate I2P fake-SAM config"
import json
import pathlib
import sys

config_path, sam_port, run_dir, peers_raw = sys.argv[1:5]
peers = [item.strip() for item in peers_raw.split(",") if item.strip()]
entries = [
    'type = "I2PInterface"',
    "enabled = true",
    'name = "i2p-fake-sam"',
    "connectable = true",
    'sam_host = "127.0.0.1"',
    f"sam_port = {int(sam_port)}",
    f"storagepath = {json.dumps(f'{run_dir}/i2p-state')}",
    "configured_bitrate = 256000",
    "reconnect_backoff_ms = 100",
]
if peers:
    entries.append("peers = [" + ", ".join(json.dumps(peer) for peer in peers) + "]")
lines = ["[[interfaces]]"]
lines.extend(entries)
pathlib.Path(config_path).write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet

"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$RPC_ADDR" \
  --rpc-unix "$RPC_UNIX" \
  --db "$DB_PATH" \
  --config "$CONFIG_PATH" \
  --strict-interface-startup >"$RETICULUMD_LOG" 2>&1 &
RET_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before fake-SAM I2P status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$SAM_ENDPOINT" "$I2P_PEERS"
import json
import sys

json_path, human_path, sam_endpoint, peers_raw = sys.argv[1:5]
expected_peers = [item.strip() for item in peers_raw.split(",") if item.strip()]
payload = json.load(open(json_path, "r", encoding="utf-8"))
rows = payload.get("interfaces") or []
i2p = next(
    (
        row
        for row in rows
        if row.get("type") == "i2p" and row.get("name") == "i2p-fake-sam"
    ),
    None,
)
if i2p is None:
    raise SystemExit(1)
runtime_root = (i2p.get("settings") or {}).get("_runtime") or {}
if runtime_root.get("startup_status") != "spawned":
    raise SystemExit(1)
runtime = runtime_root.get("i2p") or {}
tunnel = runtime.get("tunnel_status") or {}
reachable = runtime.get("reachable_endpoint")
if not isinstance(reachable, str) or not reachable.endswith(".b32.i2p"):
    raise SystemExit(1)
if runtime.get("private_key_persisted") is not True:
    raise SystemExit(1)
if tunnel.get("sam_endpoint") != sam_endpoint:
    raise SystemExit(1)
if tunnel.get("connectable") is not True:
    raise SystemExit(1)
if tunnel.get("accept_state") != "listening":
    raise SystemExit(1)
if tunnel.get("configured_peer_count") != len(expected_peers):
    raise SystemExit(1)
rows_by_peer = {
    row.get("peer"): row
    for row in tunnel.get("peers", [])
    if row.get("direction") == "outbound"
}
for peer in expected_peers:
    row = rows_by_peer.get(peer)
    if not row or row.get("state") != "connected" or not row.get("iface"):
        raise SystemExit(1)
    if (row.get("reconnect_attempts") or 0) < 1:
        raise SystemExit(1)
    if row.get("last_error") is not None:
        raise SystemExit(1)
incoming_rows = [
    row
    for row in tunnel.get("peers", [])
    if row.get("direction") == "incoming" and row.get("state") == "connected"
]
if not any(row.get("peer") == "fake-remote-destination" and row.get("iface") for row in incoming_rows):
    raise SystemExit(1)
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
if f"i2p sam={sam_endpoint} accept=listening" not in human:
    raise SystemExit(1)
if f"outbound={len(expected_peers)}" not in human:
    raise SystemExit(1)
if "incoming=1" not in human:
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[i2p-fake-sam-smoke] pass"
      echo "[i2p-fake-sam-smoke] report=${REPORT_PATH}"
      echo "[i2p-fake-sam-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for healthy fake-SAM I2P runtime status"
