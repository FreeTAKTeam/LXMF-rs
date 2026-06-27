#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

VRN76_PERIPHERAL_ID="${VRN76_PERIPHERAL_ID:-${VRN76_DEVICE_NAME_FILTER:-}}"
VRN76_ADAPTER="${VRN76_ADAPTER:-}"
VRN76_MTU="${VRN76_MTU:-564}"
VRN76_MAX_WRITE_LEN="${VRN76_MAX_WRITE_LEN:-512}"
VRN76_FRAME_MODE="${VRN76_FRAME_MODE:-benshi_tnc_data}"
VRN76_KISS_FLOW_CONTROL="${VRN76_KISS_FLOW_CONTROL:-false}"
VRN76_PREAMBLE_MS="${VRN76_PREAMBLE_MS:-350}"
VRN76_TX_TAIL_MS="${VRN76_TX_TAIL_MS:-20}"
VRN76_PERSISTENCE="${VRN76_PERSISTENCE:-64}"
VRN76_SLOT_TIME_MS="${VRN76_SLOT_TIME_MS:-20}"
VRN76_SCAN_TIMEOUT_MS="${VRN76_SCAN_TIMEOUT_MS:-10000}"
VRN76_CONNECT_TIMEOUT_MS="${VRN76_CONNECT_TIMEOUT_MS:-3000}"
VRN76_RECONNECT_BACKOFF_MS="${VRN76_RECONNECT_BACKOFF_MS:-500}"
VRN76_MAX_RECONNECT_BACKOFF_MS="${VRN76_MAX_RECONNECT_BACKOFF_MS:-5000}"
TIMEOUT_SECS="${VRN76_TIMEOUT_SECS:-${TIMEOUT_SECS:-180}}"
if [[ -z "$TIMEOUT_SECS" ]]; then
  TIMEOUT_SECS="180"
fi

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/vrn76-hil}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-vrn76-kiss-ble.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$VRN76_PERIPHERAL_ID" "$VRN76_ADAPTER" "$VRN76_MTU" "$VRN76_MAX_WRITE_LEN" "$VRN76_FRAME_MODE" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    peripheral_id,
    adapter,
    mtu,
    max_write_len,
    frame_mode,
    rpc_addr,
    run_dir,
    config_path,
    log_path,
    rnstatus_json,
    rnstatus_human,
) = sys.argv[1:15]
report = {
    "status": status,
    "reason": reason or None,
    "peripheral_id": peripheral_id,
    "adapter": adapter or None,
    "expected_mtu": int(mtu),
    "expected_max_write_len": int(max_write_len),
    "frame_mode": frame_mode,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
    "reticulumd_log": log_path,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
}
json_path = pathlib.Path(rnstatus_json)
if json_path.exists():
    try:
        payload = json.loads(json_path.read_text(encoding="utf-8"))
        rows = payload.get("interfaces") or []
        row = next(
            (
                item
                for item in rows
                if item.get("type") == "vrn76_kiss_ble"
                and item.get("name") == "vrn76-prepared-host"
            ),
            None,
        )
        if row:
            runtime = (row.get("settings") or {}).get("_runtime") or {}
            status_root = ((runtime.get("vrn76") or {}).get("status") or {})
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_iface"] = runtime.get("iface")
            report["connected"] = status_root.get("connected")
            report["subscribed"] = status_root.get("subscribed")
            report["interface_ready"] = status_root.get("interface_ready")
            report["startup_write_failures"] = status_root.get("startup_write_failures")
            report["pending_payloads"] = status_root.get("pending_payloads")
            report["pending_writes"] = status_root.get("pending_writes")
            report["pending_packets"] = status_root.get("pending_packets")
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
  if [[ $status -ne 0 ]]; then
    echo "[vrn76-kiss-ble-prepared-host-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[vrn76-kiss-ble-prepared-host-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

if [[ -z "$VRN76_PERIPHERAL_ID" ]]; then
  fail "VRN76_PERIPHERAL_ID must name a prepared VR-N76 peripheral name, address, or platform id"
fi

python3 - <<'PY' "$VRN76_MTU" "$VRN76_MAX_WRITE_LEN" "$VRN76_PREAMBLE_MS" "$VRN76_TX_TAIL_MS" "$VRN76_PERSISTENCE" "$VRN76_SLOT_TIME_MS" "$VRN76_SCAN_TIMEOUT_MS" "$VRN76_CONNECT_TIMEOUT_MS" "$VRN76_RECONNECT_BACKOFF_MS" "$VRN76_MAX_RECONNECT_BACKOFF_MS" "$TIMEOUT_SECS" "$VRN76_FRAME_MODE" "$VRN76_KISS_FLOW_CONTROL" || fail "VR-N76 environment is invalid"
import sys

(
    mtu,
    max_write_len,
    preamble_ms,
    tx_tail_ms,
    persistence,
    slot_time_ms,
    scan_timeout_ms,
    connect_timeout_ms,
    reconnect_backoff_ms,
    max_reconnect_backoff_ms,
    timeout_secs,
    frame_mode,
    flow_control,
) = sys.argv[1:14]
numeric = [
    int(mtu),
    int(max_write_len),
    int(preamble_ms),
    int(tx_tail_ms),
    int(persistence),
    int(slot_time_ms),
    int(scan_timeout_ms),
    int(connect_timeout_ms),
    int(reconnect_backoff_ms),
    int(max_reconnect_backoff_ms),
    int(timeout_secs),
]
if any(value <= 0 for value in numeric):
    raise SystemExit(1)
if numeric[0] < 64 or numeric[0] > 65535:
    raise SystemExit(1)
if numeric[1] < 16:
    raise SystemExit(1)
if frame_mode not in {"benshi_tnc_data", "benshi", "raw_kiss", "raw"}:
    raise SystemExit(1)
if flow_control.lower() not in {"1", "0", "true", "false", "yes", "no", "on", "off"}:
    raise SystemExit(1)
PY

python3 - <<'PY' "$CONFIG_PATH" "$VRN76_PERIPHERAL_ID" "$VRN76_ADAPTER" "$VRN76_MTU" "$VRN76_MAX_WRITE_LEN" "$VRN76_FRAME_MODE" "$VRN76_KISS_FLOW_CONTROL" "$VRN76_PREAMBLE_MS" "$VRN76_TX_TAIL_MS" "$VRN76_PERSISTENCE" "$VRN76_SLOT_TIME_MS" "$VRN76_SCAN_TIMEOUT_MS" "$VRN76_CONNECT_TIMEOUT_MS" "$VRN76_RECONNECT_BACKOFF_MS" "$VRN76_MAX_RECONNECT_BACKOFF_MS" || fail "failed to generate VR-N76 config"
import json
import pathlib
import sys

(
    config_path,
    peripheral_id,
    adapter,
    mtu,
    max_write_len,
    frame_mode,
    flow_control,
    preamble_ms,
    tx_tail_ms,
    persistence,
    slot_time_ms,
    scan_timeout_ms,
    connect_timeout_ms,
    reconnect_backoff_ms,
    max_reconnect_backoff_ms,
) = sys.argv[1:16]
flow_enabled = flow_control.lower() in {"1", "true", "yes", "on"}
lines = [
    "[[interfaces]]",
    'type = "vrn76_kiss_ble"',
    "enabled = true",
    'name = "vrn76-prepared-host"',
    f"peripheral_id = {json.dumps(peripheral_id)}",
    f"mtu = {int(mtu)}",
    f"max_write_len = {int(max_write_len)}",
    f"frame_mode = {json.dumps(frame_mode)}",
    f"kiss_flow_control = {str(flow_enabled).lower()}",
    f"preamble_ms = {int(preamble_ms)}",
    f"tx_tail_ms = {int(tx_tail_ms)}",
    f"persistence = {int(persistence)}",
    f"slot_time_ms = {int(slot_time_ms)}",
    f"scan_timeout_ms = {int(scan_timeout_ms)}",
    f"connect_timeout_ms = {int(connect_timeout_ms)}",
    f"reconnect_backoff_ms = {int(reconnect_backoff_ms)}",
    f"max_reconnect_backoff_ms = {int(max_reconnect_backoff_ms)}",
]
if adapter.strip():
    lines.insert(5, f"adapter = {json.dumps(adapter.strip())}")
pathlib.Path(config_path).write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

cargo build -p reticulumd --bin reticulumd --features vrn76-kiss-ble --quiet
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
    fail "reticulumd exited before VR-N76 status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN"
import json
import sys

json_path, human_path = sys.argv[1:3]
payload = json.load(open(json_path, "r", encoding="utf-8"))
row = next(
    (
        item
        for item in payload.get("interfaces") or []
        if item.get("type") == "vrn76_kiss_ble"
        and item.get("name") == "vrn76-prepared-host"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime = (row.get("settings") or {}).get("_runtime") or {}
if runtime.get("startup_status") != "spawned":
    raise SystemExit(1)
if not isinstance(runtime.get("iface"), str) or not runtime.get("iface"):
    raise SystemExit(1)
status = ((runtime.get("vrn76") or {}).get("status") or {})
for key in ["connected", "subscribed", "interface_ready"]:
    if status.get(key) is not True:
        raise SystemExit(1)
for key in ["startup_write_failures", "pending_payloads", "pending_writes", "pending_packets"]:
    if not isinstance(status.get(key), int) or status.get(key) < 0:
        raise SystemExit(1)
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
if "vrn76-prepared-host" not in human:
    raise SystemExit(1)
if "vrn76 connected=true subscribed=true ready=true" not in human:
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[vrn76-kiss-ble-prepared-host-smoke] pass"
      echo "[vrn76-kiss-ble-prepared-host-smoke] report=${REPORT_PATH}"
      echo "[vrn76-kiss-ble-prepared-host-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 2
done

fail "timed out waiting for healthy VR-N76 runtime status"
