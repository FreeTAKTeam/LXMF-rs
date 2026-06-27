#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/local-interface-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-local-interface.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
FAKE_LOG="${RUN_DIR}/fake-shared-instance.log"
FAKE_STATE="${RUN_DIR}/fake-shared-instance-state.json"

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
        rows = {}
        for expected in ["local-tcp-listener", "local-tcp-attach"]:
            row = next(
                (
                    item
                    for item in payload.get("interfaces", [])
                    if item.get("name") == expected
                ),
                None,
            )
            if row:
                runtime = (row.get("settings") or {}).get("_runtime") or {}
                rows[expected] = {
                    "type": row.get("type"),
                    "enabled": row.get("enabled"),
                    "host": row.get("host"),
                    "port": row.get("port"),
                    "startup_status": runtime.get("startup_status"),
                    "runtime_iface": runtime.get("iface") or runtime.get("runtime_iface"),
                }
        report["interfaces"] = rows
    except Exception as exc:
        report["status_parse_error"] = str(exc)
state_path = pathlib.Path(fake_state)
if state_path.exists():
    try:
        report["fake_shared_instance"] = json.loads(state_path.read_text(encoding="utf-8"))
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
    echo "[local-interface-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[local-interface-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

LOCAL_LISTENER_PORT="$(
  python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"

python3 - "$FAKE_LOG" "$FAKE_STATE" <<'PY' &
import json
import pathlib
import select
import socket
import sys
import time

log_path, state_path = sys.argv[1:3]

state = {
    "host": "127.0.0.1",
    "port": None,
    "accepted_connections": 0,
    "closed_connections": 0,
    "bytes_rx": 0,
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


listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", 0))
listener.listen()
listener.setblocking(False)
state["port"] = listener.getsockname()[1]
save_state()
log(f"fake shared LocalInterface listener on 127.0.0.1:{state['port']}")

clients = []
deadline = time.monotonic() + 300
while time.monotonic() < deadline:
    readable, _, _ = select.select([listener, *clients], [], [], 0.1)
    for sock in readable:
        if sock is listener:
            conn, addr = listener.accept()
            conn.setblocking(False)
            clients.append(conn)
            state["accepted_connections"] += 1
            log(f"accepted connection {state['accepted_connections']} from {addr[0]}:{addr[1]}")
            save_state()
            continue
        try:
            chunk = sock.recv(4096)
        except BlockingIOError:
            continue
        except OSError as exc:
            log(f"read error: {exc}")
            chunk = b""
        if chunk:
            state["bytes_rx"] += len(chunk)
            save_state()
            continue
        clients.remove(sock)
        try:
            sock.close()
        except OSError:
            pass
        state["closed_connections"] += 1
        save_state()

log("fake shared LocalInterface listener exiting")
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
    fail "timed out waiting for fake shared LocalInterface listener"
  fi
  if ! kill -0 "$FAKE_PID" >/dev/null 2>&1; then
    fail "fake shared LocalInterface listener exited before publishing port"
  fi
  sleep 0.1
done

FAKE_SHARED_PORT="$(
  python3 - <<'PY' "$FAKE_STATE"
import json
import sys
print(json.load(open(sys.argv[1], "r", encoding="utf-8"))["port"])
PY
)"

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "LocalInterface"
enabled = true
name = "local-tcp-listener"
shared_instance_type = "tcp"
host = "127.0.0.1"
port = ${LOCAL_LISTENER_PORT}
fixed_mtu = 262144
force_shared_instance_bitrate = 1000000

[[interfaces]]
type = "LocalClientInterface"
enabled = true
name = "local-tcp-attach"
shared_instance_type = "tcp"
host = "127.0.0.1"
port = ${FAKE_SHARED_PORT}
fixed_mtu = 262144
force_shared_instance_bitrate = 1000000
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
    fail "reticulumd exited before LocalInterface status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_STATE" "$LOCAL_LISTENER_PORT" "$FAKE_SHARED_PORT"
import json
import socket
import sys

json_path, human_path, fake_state_path, local_port_raw, fake_port_raw = sys.argv[1:6]
local_port = int(local_port_raw)
fake_port = int(fake_port_raw)
payload = json.load(open(json_path, "r", encoding="utf-8"))
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
fake_state = json.load(open(fake_state_path, "r", encoding="utf-8"))

interfaces = payload.get("interfaces", [])
listener = next(
    (
        row
        for row in interfaces
        if row.get("type") == "local" and row.get("name") == "local-tcp-listener"
    ),
    None,
)
attach = next(
    (
        row
        for row in interfaces
        if row.get("type") == "local_client" and row.get("name") == "local-tcp-attach"
    ),
    None,
)
if listener is None or attach is None:
    raise SystemExit(1)

listener_runtime = (listener.get("settings") or {}).get("_runtime") or {}
attach_runtime = (attach.get("settings") or {}).get("_runtime") or {}
if listener_runtime.get("startup_status") != "active":
    raise SystemExit(1)
if attach_runtime.get("startup_status") != "attached":
    raise SystemExit(1)
for row, expected_port in [(listener, local_port), (attach, fake_port)]:
    if row.get("enabled") is not True:
        raise SystemExit(1)
    if row.get("host") != "127.0.0.1":
        raise SystemExit(1)
    if row.get("port") != expected_port:
        raise SystemExit(1)
    settings = row.get("settings") or {}
    if settings.get("shared_instance_type") != "tcp":
        raise SystemExit(1)
    if settings.get("mtu") != 262144:
        raise SystemExit(1)
    if settings.get("bitrate") != 1000000:
        raise SystemExit(1)
    runtime = settings.get("_runtime") or {}
    runtime_iface = runtime.get("iface") or runtime.get("runtime_iface")
    if not isinstance(runtime_iface, str) or not runtime_iface:
        raise SystemExit(1)

if (fake_state.get("accepted_connections") or 0) < 2:
    raise SystemExit(1)
with socket.create_connection(("127.0.0.1", local_port), timeout=1.0):
    pass

if "local-tcp-listener" not in human or "local-tcp-attach" not in human:
    raise SystemExit(1)
for expected in [" local ", " local_client ", " active", " attached"]:
    if expected not in human:
        raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[local-interface-smoke] pass"
      echo "[local-interface-smoke] report=${REPORT_PATH}"
      echo "[local-interface-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for healthy LocalInterface runtime status"
