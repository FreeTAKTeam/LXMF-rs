#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

SAM_HOST="${SAM_HOST:-127.0.0.1}"
SAM_PORT="${SAM_PORT:-7656}"
TIMEOUT_SECS="${TIMEOUT_SECS:-360}"
I2P_PAIR_SOAK_SECS="${I2P_PAIR_SOAK_SECS:-${SOAK_SECS:-0}}"
I2P_PAIR_SOAK_POLL_SECS="${I2P_PAIR_SOAK_POLL_SECS:-5}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/i2p-hil-pair}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
SAM_ENDPOINT="${SAM_HOST}:${SAM_PORT}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
ACCEPTOR_DIR="${RUN_DIR}/acceptor"
DIALER_DIR="${RUN_DIR}/dialer"
mkdir -p "$ACCEPTOR_DIR" "$DIALER_DIR"

ACCEPTOR_CONFIG="${ACCEPTOR_DIR}/reticulumd-i2p.toml"
DIALER_CONFIG="${DIALER_DIR}/reticulumd-i2p.toml"
ACCEPTOR_DB="${ACCEPTOR_DIR}/reticulum.db"
DIALER_DB="${DIALER_DIR}/reticulum.db"
ACCEPTOR_RPC_UNIX="${ACCEPTOR_DIR}/rpc.sock"
DIALER_RPC_UNIX="${DIALER_DIR}/rpc.sock"
ACCEPTOR_LOG="${ACCEPTOR_DIR}/reticulumd.log"
DIALER_LOG="${DIALER_DIR}/reticulumd.log"
ACCEPTOR_STATUS="${ACCEPTOR_DIR}/rnstatus.json"
DIALER_STATUS="${DIALER_DIR}/rnstatus.json"
SOAK_SAMPLES="${RUN_DIR}/soak-samples.jsonl"
ACCEPTOR_ENDPOINT=""

: >"$ACCEPTOR_LOG"
: >"$DIALER_LOG"
: >"$SOAK_SAMPLES"

pick_rpc_addr() {
  python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(f"127.0.0.1:{sock.getsockname()[1]}")
PY
}

ACCEPTOR_RPC_ADDR="${ACCEPTOR_RPC_ADDR:-$(pick_rpc_addr)}"
DIALER_RPC_ADDR="${DIALER_RPC_ADDR:-$(pick_rpc_addr)}"

write_report() {
  local status="$1"
  local reason="${2:-}"
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$SAM_ENDPOINT" "$RUN_DIR" "$ACCEPTOR_RPC_ADDR" "$DIALER_RPC_ADDR" "$ACCEPTOR_LOG" "$DIALER_LOG" "$ACCEPTOR_STATUS" "$DIALER_STATUS" "$I2P_PAIR_SOAK_SECS" "$SOAK_SAMPLES"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    sam_endpoint,
    run_dir,
    acceptor_rpc,
    dialer_rpc,
    acceptor_log,
    dialer_log,
    acceptor_status,
    dialer_status,
    soak_secs,
    soak_samples_path,
) = sys.argv[1:14]

def read_i2p(path):
    status_path = pathlib.Path(path)
    if not status_path.exists():
        return {}
    try:
        payload = json.loads(status_path.read_text(encoding="utf-8"))
    except Exception as exc:
        return {"status_parse_error": str(exc)}
    rows = payload.get("interfaces") or []
    i2p = next((row for row in rows if row.get("type") == "i2p"), None)
    if not i2p:
        return {}
    runtime = ((i2p.get("settings") or {}).get("_runtime") or {}).get("i2p") or {}
    tunnel = runtime.get("tunnel_status") or {}
    peer_rows = tunnel.get("peers") or []
    return {
        "interface_name": i2p.get("name"),
        "reachable_endpoint": runtime.get("reachable_endpoint"),
        "private_key_persisted": runtime.get("private_key_persisted"),
        "accept_state": tunnel.get("accept_state"),
        "configured_peer_count": tunnel.get("configured_peer_count"),
        "peer_rows": peer_rows,
        "connected_outbound_peers": [
            row.get("peer")
            for row in peer_rows
            if row.get("direction") == "outbound" and row.get("state") == "connected"
        ],
    }

report = {
    "status": status,
    "evidence_scope": (
        "sam_connectable_with_outbound_peers_real_pair_soak"
        if int(soak_secs) > 0 and status == "pass"
        else "sam_connectable_with_outbound_peers_real_pair"
    ),
    "product_boundary": (
        "Single real-SAM router pair evidence proves a live prepared-host "
        "destination can be dialed by a second local daemon; broader public "
        "I2P peer-set and long-running production evidence remain separate."
    ),
    "sam_endpoint": sam_endpoint,
    "run_dir": run_dir,
    "acceptor_rpc_addr": acceptor_rpc,
    "dialer_rpc_addr": dialer_rpc,
    "acceptor_log": acceptor_log,
    "dialer_log": dialer_log,
    "acceptor_rnstatus_json": acceptor_status,
    "dialer_rnstatus_json": dialer_status,
    "soak_requested_secs": int(soak_secs),
    "soak_samples_jsonl": soak_samples_path,
    "acceptor": read_i2p(acceptor_status),
    "dialer": read_i2p(dialer_status),
}
samples_path = pathlib.Path(soak_samples_path)
if samples_path.exists():
    samples = []
    for line in samples_path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            samples.append(json.loads(line))
    report["soak_sample_count"] = len(samples)
    report["soak_samples"] = samples
if reason:
    report["reason"] = reason
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

if ! python3 - "$I2P_PAIR_SOAK_SECS" "$I2P_PAIR_SOAK_POLL_SECS" <<'PY'
import sys
soak, poll = (int(value) for value in sys.argv[1:3])
if soak < 0 or poll <= 0:
    raise SystemExit(1)
PY
then
  echo "[i2p-prepared-host-pair-smoke] ERROR: I2P pair soak timing environment is invalid" >&2
  exit 1
fi

cleanup() {
  local status=$?
  if [[ -n "${DIALER_PID:-}" ]]; then
    kill "$DIALER_PID" >/dev/null 2>&1 || true
    wait "$DIALER_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${ACCEPTOR_PID:-}" ]]; then
    kill "$ACCEPTOR_PID" >/dev/null 2>&1 || true
    wait "$ACCEPTOR_PID" >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[i2p-prepared-host-pair-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[i2p-prepared-host-pair-smoke] ERROR: ${msg}" >&2
  write_report "fail" "$msg"
  exit 1
}

pair_status_ok() {
  python3 - <<'PY' "$ACCEPTOR_STATUS" "$DIALER_STATUS" "$SAM_ENDPOINT" "$ACCEPTOR_ENDPOINT"
import json
import sys

acceptor_path, dialer_path, sam_endpoint, peer = sys.argv[1:5]
acceptor_payload = json.load(open(acceptor_path, "r", encoding="utf-8"))
dialer_payload = json.load(open(dialer_path, "r", encoding="utf-8"))

acceptor_rows = acceptor_payload.get("interfaces") or []
acceptor_i2p = next((row for row in acceptor_rows if row.get("type") == "i2p" and row.get("name") == "i2p-pair-acceptor"), None)
if not acceptor_i2p:
    raise SystemExit(1)
acceptor_runtime = ((acceptor_i2p.get("settings") or {}).get("_runtime") or {}).get("i2p") or {}
acceptor_tunnel = acceptor_runtime.get("tunnel_status") or {}
if acceptor_tunnel.get("sam_endpoint") != sam_endpoint:
    raise SystemExit(1)
if acceptor_tunnel.get("accept_state") != "listening":
    raise SystemExit(1)
if not any(
    row.get("direction") == "incoming" and row.get("state") == "connected" and row.get("iface")
    for row in acceptor_tunnel.get("peers", [])
):
    raise SystemExit(1)

dialer_rows = dialer_payload.get("interfaces") or []
dialer_i2p = next((row for row in dialer_rows if row.get("type") == "i2p" and row.get("name") == "i2p-pair-dialer"), None)
if not dialer_i2p:
    raise SystemExit(1)
runtime_root = (dialer_i2p.get("settings") or {}).get("_runtime") or {}
if runtime_root.get("startup_status") != "spawned":
    raise SystemExit(1)
runtime = runtime_root.get("i2p") or {}
tunnel = runtime.get("tunnel_status") or {}
if tunnel.get("sam_endpoint") != sam_endpoint:
    raise SystemExit(1)
if tunnel.get("connectable") is not True or tunnel.get("accept_state") != "listening":
    raise SystemExit(1)
if tunnel.get("configured_peer_count") != 1:
    raise SystemExit(1)
rows_by_peer = {
    row.get("peer"): row
    for row in tunnel.get("peers", [])
    if row.get("direction") == "outbound"
}
row = rows_by_peer.get(peer)
if not row or row.get("state") != "connected" or not row.get("iface"):
    raise SystemExit(1)
PY
}

append_soak_sample() {
  local elapsed="$1"
  python3 - <<'PY' "$SOAK_SAMPLES" "$elapsed" "$ACCEPTOR_STATUS" "$DIALER_STATUS"
import json
import pathlib
import sys

samples_path, elapsed, acceptor_path, dialer_path = sys.argv[1:5]

def read_i2p(path):
    payload = json.load(open(path, "r", encoding="utf-8"))
    row = next((item for item in payload.get("interfaces", []) if item.get("type") == "i2p"), None)
    runtime = ((row or {}).get("settings") or {}).get("_runtime") or {}
    i2p = runtime.get("i2p") or {}
    tunnel = i2p.get("tunnel_status") or {}
    return {
        "startup_status": runtime.get("startup_status"),
        "reachable_endpoint": i2p.get("reachable_endpoint"),
        "accept_state": tunnel.get("accept_state"),
        "configured_peer_count": tunnel.get("configured_peer_count"),
        "peers": tunnel.get("peers") or [],
    }

sample = {
    "elapsed_secs": int(elapsed),
    "acceptor": read_i2p(acceptor_path),
    "dialer": read_i2p(dialer_path),
}
with pathlib.Path(samples_path).open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(sample, sort_keys=True) + "\n")
PY
}

run_pair_soak() {
  if (( I2P_PAIR_SOAK_SECS <= 0 )); then
    return 0
  fi

  local start=$SECONDS
  append_soak_sample 0
  while (( SECONDS - start < I2P_PAIR_SOAK_SECS )); do
    sleep "$I2P_PAIR_SOAK_POLL_SECS"
    if ! kill -0 "$ACCEPTOR_PID" >/dev/null 2>&1; then
      fail "acceptor reticulumd exited during I2P pair soak"
    fi
    if ! kill -0 "$DIALER_PID" >/dev/null 2>&1; then
      fail "dialer reticulumd exited during I2P pair soak"
    fi
    "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$ACCEPTOR_RPC_ADDR" --json >"$ACCEPTOR_STATUS" 2>>"$ACCEPTOR_LOG" || fail "failed to refresh acceptor status during I2P pair soak"
    "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$DIALER_RPC_ADDR" --json >"$DIALER_STATUS" 2>>"$DIALER_LOG" || fail "failed to refresh dialer status during I2P pair soak"
    pair_status_ok || fail "I2P pair connection state regressed during soak"
    append_soak_sample "$((SECONDS - start))"
  done
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

write_config() {
  local config_path="$1"
  local run_dir="$2"
  local name="$3"
  local peer="${4:-}"
  python3 - <<'PY' "$config_path" "$run_dir" "$name" "$SAM_HOST" "$SAM_PORT" "$peer"
import json
import pathlib
import sys

config_path, run_dir, name, sam_host, sam_port, peer = sys.argv[1:7]
entries = [
    'type = "I2PInterface"',
    "enabled = true",
    f"name = {json.dumps(name)}",
    "connectable = true",
    f"sam_host = {json.dumps(sam_host)}",
    f"sam_port = {int(sam_port)}",
    f"storagepath = {json.dumps(f'{run_dir}/i2p-state')}",
    "configured_bitrate = 256000",
]
if peer:
    entries.append(f"peers = [{json.dumps(peer)}]")
lines = ["[[interfaces]]"]
lines.extend(entries)
pathlib.Path(config_path).write_text("\n".join(lines) + "\n", encoding="utf-8")
PY
}

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet

write_config "$ACCEPTOR_CONFIG" "$ACCEPTOR_DIR" "i2p-pair-acceptor"
"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$ACCEPTOR_RPC_ADDR" \
  --rpc-unix "$ACCEPTOR_RPC_UNIX" \
  --db "$ACCEPTOR_DB" \
  --config "$ACCEPTOR_CONFIG" \
  --strict-interface-startup >"$ACCEPTOR_LOG" 2>&1 &
ACCEPTOR_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  if ! kill -0 "$ACCEPTOR_PID" >/dev/null 2>&1; then
    fail "acceptor reticulumd exited before I2P status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$ACCEPTOR_RPC_ADDR" --json >"$ACCEPTOR_STATUS" 2>>"$ACCEPTOR_LOG"; then
    ACCEPTOR_ENDPOINT="$(python3 - <<'PY' "$ACCEPTOR_STATUS" "$SAM_ENDPOINT" || true
import json
import sys

path, sam_endpoint = sys.argv[1:3]
payload = json.load(open(path, "r", encoding="utf-8"))
rows = payload.get("interfaces") or []
i2p = next((row for row in rows if row.get("type") == "i2p" and row.get("name") == "i2p-pair-acceptor"), None)
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
print(reachable)
PY
)"
    if [[ -n "$ACCEPTOR_ENDPOINT" ]]; then
      break
    fi
  fi
  sleep 2
done

if [[ -z "$ACCEPTOR_ENDPOINT" ]]; then
  fail "timed out waiting for acceptor I2P connectable runtime status"
fi

write_config "$DIALER_CONFIG" "$DIALER_DIR" "i2p-pair-dialer" "$ACCEPTOR_ENDPOINT"
"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$DIALER_RPC_ADDR" \
  --rpc-unix "$DIALER_RPC_UNIX" \
  --db "$DIALER_DB" \
  --config "$DIALER_CONFIG" \
  --strict-interface-startup >"$DIALER_LOG" 2>&1 &
DIALER_PID=$!

while (( SECONDS < deadline )); do
  if ! kill -0 "$ACCEPTOR_PID" >/dev/null 2>&1; then
    fail "acceptor reticulumd exited while waiting for dialer peer connection"
  fi
  if ! kill -0 "$DIALER_PID" >/dev/null 2>&1; then
    fail "dialer reticulumd exited before outbound I2P peer connected"
  fi
  "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$ACCEPTOR_RPC_ADDR" --json >"$ACCEPTOR_STATUS" 2>>"$ACCEPTOR_LOG" || true
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$DIALER_RPC_ADDR" --json >"$DIALER_STATUS" 2>>"$DIALER_LOG"; then
    if pair_status_ok; then
      run_pair_soak
      write_report "pass"
      echo "[i2p-prepared-host-pair-smoke] pass"
      echo "[i2p-prepared-host-pair-smoke] report=${REPORT_PATH}"
      echo "[i2p-prepared-host-pair-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 2
done

fail "timed out waiting for dialer outbound I2P peer to connect"
