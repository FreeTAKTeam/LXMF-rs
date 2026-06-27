#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

SAM_HOST="${SAM_HOST:-127.0.0.1}"
SAM_PORT="${SAM_PORT:-7656}"
I2P_PEERS="${I2P_PEERS:-}"
TIMEOUT_SECS="${TIMEOUT_SECS:-180}"
if [[ -z "$SAM_HOST" ]]; then
  SAM_HOST="127.0.0.1"
fi
if [[ -z "$SAM_PORT" ]]; then
  SAM_PORT="7656"
fi
if [[ -z "$TIMEOUT_SECS" ]]; then
  TIMEOUT_SECS="180"
fi
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/i2p-hil}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-i2p.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
SAM_ENDPOINT="${SAM_HOST}:${SAM_PORT}"

: >"$RETICULUMD_LOG"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$SAM_ENDPOINT" "$I2P_PEERS" "$RPC_ADDR" "$RUN_DIR" "$RETICULUMD_LOG" "$RNSTATUS_JSON"
import json
import pathlib
import sys

report_path, status, reason, sam_endpoint, peers_raw, rpc_addr, run_dir, log_path, rnstatus_path = sys.argv[1:10]
expected_peers = [item.strip() for item in peers_raw.split(",") if item.strip()]
report = {
    "status": status,
    "sam_endpoint": sam_endpoint,
    "expected_outbound_peers": expected_peers,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "reticulumd_log": log_path,
    "rnstatus_json": rnstatus_path,
}
if reason:
    report["reason"] = reason
status_path = pathlib.Path(rnstatus_path)
if status_path.exists():
    try:
        payload = json.loads(status_path.read_text(encoding="utf-8"))
        rows = payload.get("interfaces") or []
        i2p = next((row for row in rows if row.get("type") == "i2p"), None)
        if i2p:
            runtime = ((i2p.get("settings") or {}).get("_runtime") or {}).get("i2p") or {}
            tunnel = runtime.get("tunnel_status") or {}
            report["reachable_endpoint"] = runtime.get("reachable_endpoint")
            report["private_key_persisted"] = runtime.get("private_key_persisted")
            report["accept_state"] = tunnel.get("accept_state")
            report["configured_peer_count"] = tunnel.get("configured_peer_count")
            peer_rows = tunnel.get("peers") or []
            report["peer_rows"] = peer_rows
            report["connected_outbound_peers"] = [
                row.get("peer")
                for row in peer_rows
                if row.get("direction") == "outbound" and row.get("state") == "connected"
            ]
    except Exception as exc:  # best-effort artifact enrichment
        report["status_parse_error"] = str(exc)
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

cleanup() {
  local status=$?
  if [[ -n "${RET_PID:-}" ]]; then
    kill "$RET_PID" >/dev/null 2>&1 || true
    wait "$RET_PID" >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[i2p-prepared-host-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[i2p-prepared-host-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - <<'PY' "$SAM_HOST" "$SAM_PORT" || fail "SAM endpoint ${SAM_ENDPOINT} did not complete HELLO"
import socket
import sys

host, port = sys.argv[1], int(sys.argv[2])
with socket.create_connection((host, port), timeout=5) as sock:
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

python3 - <<'PY' "$CONFIG_PATH" "$SAM_HOST" "$SAM_PORT" "$RUN_DIR" "$I2P_PEERS" || fail "failed to generate I2P config"
import json
import pathlib
import sys

config_path, sam_host, sam_port, run_dir, peers_raw = sys.argv[1:6]
peers = [item.strip() for item in peers_raw.split(",") if item.strip()]
entries = [
    'type = "I2PInterface"',
    "enabled = true",
    'name = "i2p-prepared-host"',
    "connectable = true",
    f"sam_host = {json.dumps(sam_host)}",
    f"sam_port = {int(sam_port)}",
    f"storagepath = {json.dumps(f'{run_dir}/i2p-state')}",
    "configured_bitrate = 256000",
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
    fail "reticulumd exited before I2P status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$SAM_ENDPOINT" "$I2P_PEERS"
import json
import sys

path, sam_endpoint, peers_raw = sys.argv[1:4]
expected_peers = [item.strip() for item in peers_raw.split(",") if item.strip()]
payload = json.load(open(path, "r", encoding="utf-8"))
rows = payload.get("interfaces") or []
i2p = next((row for row in rows if row.get("type") == "i2p" and row.get("name") == "i2p-prepared-host"), None)
if not i2p:
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
if expected_peers:
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
PY
    then
      write_report "pass"
      echo "[i2p-prepared-host-smoke] pass"
      echo "[i2p-prepared-host-smoke] report=${REPORT_PATH}"
      echo "[i2p-prepared-host-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 2
done

fail "timed out waiting for healthy I2P connectable runtime status"
