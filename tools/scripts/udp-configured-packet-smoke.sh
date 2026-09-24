#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"
TIMEOUT_SECS=30
LOG_DIR="$ROOT_DIR/target/udp-configured-packet-smoke"
REPORT_PATH="$LOG_DIR/report.json"
mkdir -p "$LOG_DIR"
RUN_DIR="$(mktemp -d "$LOG_DIR/run.XXXXXX")"
CONFIG_PATH="$RUN_DIR/reticulumd-udp.toml"
DB_PATH="$RUN_DIR/reticulum.db"
RPC_UNIX="$RUN_DIR/rpc.sock"
LOG_PATH="$RUN_DIR/reticulumd.log"
STATUS_JSON="$RUN_DIR/status.json"
ACTIVE_STATUS_JSON="$RUN_DIR/active-status.json"
PEER_REPORT="$RUN_DIR/peer.json"
RESTART_VERIFIED=false
RET_PID=""
PEER_PID=""

PORTS="$(python3 - <<'PY'
import socket
ports = []
for _ in range(2):
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind(("127.0.0.1", 0))
        ports.append(sock.getsockname()[1])
print(*ports)
PY
)"
read -r UDP_LISTEN_PORT UDP_FORWARD_PORT <<<"$PORTS"
RPC_ADDR="$(python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(f"127.0.0.1:{sock.getsockname()[1]}")
PY
)"

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "UDPInterface"
enabled = true
name = "udp-configured-trace"
listen_ip = "127.0.0.1"
listen_port = $UDP_LISTEN_PORT
forward_ip = "127.0.0.1"
forward_port = $UDP_FORWARD_PORT
EOF

cleanup() {
  if [[ -n "$RET_PID" ]]; then
    kill -INT "$RET_PID" >/dev/null 2>&1 || true
    wait "$RET_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "$PEER_PID" ]]; then
    kill "$PEER_PID" >/dev/null 2>&1 || true
    wait "$PEER_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

fail() {
  echo "[udp-configured-packet-smoke] ERROR: $1" >&2
  exit 1
}

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet
python3 tools/scripts/udp-reference-loopback-peer.py "$UDP_FORWARD_PORT" "$UDP_LISTEN_PORT" "$PEER_REPORT" \
  >"$RUN_DIR/peer.log" 2>&1 &
PEER_PID=$!

start_daemon() {
  "$ROOT_DIR/target/debug/reticulumd" --rpc "$RPC_ADDR" --rpc-unix "$RPC_UNIX" \
    --db "$DB_PATH" --config "$CONFIG_PATH" --announce-interval-secs 1 --strict-interface-startup \
    >"$LOG_PATH" 2>&1 &
  RET_PID=$!
}
stop_daemon() {
  kill -INT "$RET_PID" >/dev/null 2>&1 || fail "daemon did not accept shutdown signal"
  wait "$RET_PID" || fail "daemon shutdown failed"
  RET_PID=""
}
read_status() {
  "$ROOT_DIR/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$STATUS_JSON" 2>>"$LOG_PATH"
}

start_daemon
deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  kill -0 "$RET_PID" >/dev/null 2>&1 || fail "daemon exited during configured startup"
  if read_status 2>/dev/null && python3 - "$STATUS_JSON" "$PEER_REPORT" "$UDP_LISTEN_PORT" "$UDP_FORWARD_PORT" <<'PY'
import json, pathlib, sys
status_path, peer_path, listen_port, forward_port = sys.argv[1:5]
payload = json.loads(pathlib.Path(status_path).read_text(encoding="utf-8"))
row = next((r for r in payload.get("interfaces", []) if r.get("type") == "udp" and r.get("name") == "udp-configured-trace"), None)
runtime = ((row or {}).get("settings") or {}).get("_runtime") or {}
udp = ((runtime.get("udp") or {}).get("status") or {})
if runtime.get("startup_status") != "spawned" or udp.get("link_state") != "bound":
    raise SystemExit(1)
if udp.get("bind_addr") != f"127.0.0.1:{listen_port}" or udp.get("forward_addr") != f"127.0.0.1:{forward_port}":
    raise SystemExit(1)
if (udp.get("packets_tx") or 0) < 1 or (udp.get("packets_rx") or 0) < 1:
    raise SystemExit(1)
peer = pathlib.Path(peer_path)
if not peer.exists() or json.loads(peer.read_text(encoding="utf-8")).get("echoed_to_daemon") is not True:
    raise SystemExit(1)
PY
  then
    cp "$STATUS_JSON" "$ACTIVE_STATUS_JSON"
    break
  fi
  sleep 1
done
(( SECONDS < deadline )) || fail "no valid production UDP packet completed peer loopback"

stop_daemon
start_daemon
restart_deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < restart_deadline )); do
  kill -0 "$RET_PID" >/dev/null 2>&1 || fail "daemon exited during same-port restart"
  if read_status 2>/dev/null && python3 - "$STATUS_JSON" <<'PY'
import json, pathlib, sys
payload = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
row = next((r for r in payload.get("interfaces", []) if r.get("type") == "udp" and r.get("name") == "udp-configured-trace"), None)
runtime = ((row or {}).get("settings") or {}).get("_runtime") or {}
udp = ((runtime.get("udp") or {}).get("status") or {})
raise SystemExit(0 if runtime.get("startup_status") == "spawned" and udp.get("link_state") == "bound" else 1)
PY
  then
    RESTART_VERIFIED=true
    break
  fi
  sleep 1
done
[[ "$RESTART_VERIFIED" == true ]] || fail "same-port UDP restart failed"
stop_daemon
kill "$PEER_PID" >/dev/null 2>&1 || true
wait "$PEER_PID" >/dev/null 2>&1 || true
PEER_PID=""

python3 - "$REPORT_PATH" "$PEER_REPORT" "$STATUS_JSON" "$ACTIVE_STATUS_JSON" "$RESTART_VERIFIED" "$UDP_LISTEN_PORT" "$UDP_FORWARD_PORT" <<'PY'
import json, pathlib, sys
report_path, peer_path, status_path, active_status_path, restart, listen, forward = sys.argv[1:8]
payload = json.loads(pathlib.Path(status_path).read_text(encoding="utf-8"))
row = next(r for r in payload.get("interfaces", []) if r.get("type") == "udp" and r.get("name") == "udp-configured-trace")
runtime = (row.get("settings") or {}).get("_runtime") or {}
active_payload = json.loads(pathlib.Path(active_status_path).read_text(encoding="utf-8"))
active_row = next(r for r in active_payload.get("interfaces", []) if r.get("type") == "udp" and r.get("name") == "udp-configured-trace")
active_status = (((active_row.get("settings") or {}).get("_runtime") or {}).get("udp") or {}).get("status") or {}
report = {
    "status": "pass",
    "evidence_scope": "configured_udp_valid_packet_loopback_status_and_restart",
    "python_reference": "Reticulum 99de23c040d507e3fefca19e87b182302902725d RNS/Interfaces/UDPInterface.py",
    "configured_bind": f"127.0.0.1:{listen}",
    "configured_forward": f"127.0.0.1:{forward}",
    "startup_status_after_restart": runtime.get("startup_status"),
    "link_state_after_restart": ((runtime.get("udp") or {}).get("status") or {}).get("link_state"),
    "restart_same_ports": restart == "true",
    "packet_counters_before_restart": {
        "packets_rx": active_status.get("packets_rx"),
        "packets_tx": active_status.get("packets_tx"),
        "bytes_rx": active_status.get("bytes_rx"),
        "bytes_tx": active_status.get("bytes_tx"),
    },
    "peer_trace": json.loads(pathlib.Path(peer_path).read_text(encoding="utf-8")),
}
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
echo "[udp-configured-packet-smoke] pass"
echo "[udp-configured-packet-smoke] report=$REPORT_PATH"
