#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/udp-loopback-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-udp-loopback.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
FAKE_STATE="${RUN_DIR}/udp-loopback-state.json"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_STATE"
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
    fake_state,
) = sys.argv[1:11]
report = {
    "status": status,
    "reason": reason or None,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
    "reticulumd_log": reticulumd_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
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
                if item.get("type") == "udp" and item.get("name") == "udp-loopback"
            ),
            None,
        )
        if row:
            runtime = (row.get("settings") or {}).get("_runtime") or {}
            status_root = ((runtime.get("udp") or {}).get("status") or {})
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_iface"] = runtime.get("iface") or runtime.get("runtime_iface")
            for key in [
                "link_state",
                "role",
                "bind_addr",
                "forward_addr",
                "packets_rx",
                "packets_tx",
                "bytes_rx",
                "bytes_tx",
                "decode_errors",
                "rx_queue_errors",
                "socket_errors",
                "tx_errors",
                "dropped_direct",
                "last_error",
            ]:
                report[key] = status_root.get(key)
    except Exception as exc:
        report["status_parse_error"] = str(exc)
state_path = pathlib.Path(fake_state)
if state_path.exists():
    try:
        report["loopback_probe"] = json.loads(state_path.read_text(encoding="utf-8"))
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
  if [[ $status -ne 0 ]]; then
    echo "[udp-loopback-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[udp-loopback-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

PORTS="$(
  python3 - <<'PY'
import socket

ports = []
for _ in range(2):
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind(("127.0.0.1", 0))
        ports.append(sock.getsockname()[1])
print(" ".join(str(port) for port in ports))
PY
)"
read -r UDP_LISTEN_PORT UDP_FORWARD_PORT <<<"$PORTS"

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "UDPInterface"
enabled = true
name = "udp-loopback"
listen_ip = "127.0.0.1"
listen_port = ${UDP_LISTEN_PORT}
forward_ip = "127.0.0.1"
forward_port = ${UDP_FORWARD_PORT}
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

deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before UDP loopback status became bound"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$UDP_LISTEN_PORT" "$UDP_FORWARD_PORT"
import json
import sys

json_path, human_path, listen_port, forward_port = sys.argv[1:5]
payload = json.load(open(json_path, "r", encoding="utf-8"))
row = next(
    (
        item
        for item in payload.get("interfaces", [])
        if item.get("type") == "udp" and item.get("name") == "udp-loopback"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime = (row.get("settings") or {}).get("_runtime") or {}
if runtime.get("startup_status") != "spawned":
    raise SystemExit(1)
status = ((runtime.get("udp") or {}).get("status") or {})
if status.get("link_state") != "bound":
    raise SystemExit(1)
if status.get("role") != "peer":
    raise SystemExit(1)
if status.get("bind_addr") != f"127.0.0.1:{listen_port}":
    raise SystemExit(1)
if status.get("forward_addr") != f"127.0.0.1:{forward_port}":
    raise SystemExit(1)
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
if "udp-loopback" not in human:
    raise SystemExit(1)
if f"udp state=bound role=peer bind=127.0.0.1:{listen_port}" not in human:
    raise SystemExit(1)
if f"forward=127.0.0.1:{forward_port}" not in human:
    raise SystemExit(1)
PY
    then
      break
    fi
  fi
  sleep 1
done

if (( SECONDS >= deadline )); then
  fail "timed out waiting for bound UDP loopback runtime status"
fi

python3 - <<'PY' "$UDP_LISTEN_PORT" "$FAKE_STATE"
import json
import pathlib
import socket
import sys

listen_port, state_path = sys.argv[1:3]
payload = b"not-a-reticulum-packet"
with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
    sock.sendto(payload, ("127.0.0.1", int(listen_port)))
pathlib.Path(state_path).write_text(
    json.dumps(
        {
            "sent_malformed_datagram": True,
            "target": f"127.0.0.1:{listen_port}",
            "payload_hex": payload.hex(),
            "payload_len": len(payload),
        },
        indent=2,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY

deadline=$((SECONDS + TIMEOUT_SECS))
while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before UDP malformed datagram evidence appeared"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_STATE"
import json
import sys

json_path, human_path, state_path = sys.argv[1:4]
payload = json.load(open(json_path, "r", encoding="utf-8"))
probe = json.load(open(state_path, "r", encoding="utf-8"))
row = next(
    (
        item
        for item in payload.get("interfaces", [])
        if item.get("type") == "udp" and item.get("name") == "udp-loopback"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime = (row.get("settings") or {}).get("_runtime") or {}
status = ((runtime.get("udp") or {}).get("status") or {})
if runtime.get("startup_status") != "spawned":
    raise SystemExit(1)
if status.get("link_state") != "bound":
    raise SystemExit(1)
if (status.get("bytes_rx") or 0) < probe.get("payload_len", 0):
    raise SystemExit(1)
if (status.get("decode_errors") or 0) < 1:
    raise SystemExit(1)
if status.get("last_error") != "couldn't decode packet":
    raise SystemExit(1)
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
if "decode_errors=1" not in human:
    raise SystemExit(1)
if "err=couldn't decode packet" not in human:
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[udp-loopback-smoke] pass"
      echo "[udp-loopback-smoke] report=${REPORT_PATH}"
      echo "[udp-loopback-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for UDP malformed datagram runtime counters"
