#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

RNODE_PORT="${RNODE_PORT:-}"
RNODE_BAUD_RATE="${RNODE_BAUD_RATE:-${RNODE_SPEED:-115200}}"
RNODE_REGION="${RNODE_REGION:-US915}"
RNODE_FREQUENCY="${RNODE_FREQUENCY:-915000000}"
RNODE_BANDWIDTH="${RNODE_BANDWIDTH:-125000}"
RNODE_SPREADING_FACTOR="${RNODE_SPREADING_FACTOR:-9}"
RNODE_CODING_RATE="${RNODE_CODING_RATE:-5}"
RNODE_TX_POWER="${RNODE_TX_POWER:-17}"
RNODE_BITRATE="${RNODE_BITRATE:-${RNODE_CONFIGURED_BITRATE:-1200}}"
RNODE_COMMAND_TIMEOUT_MS="${RNODE_COMMAND_TIMEOUT_MS:-1500}"
RNODE_BLE_ADAPTER="${RNODE_BLE_ADAPTER:-}"
RNODE_BLE_SCAN_TIMEOUT_MS="${RNODE_BLE_SCAN_TIMEOUT_MS:-2000}"
RNODE_BLE_CONNECT_TIMEOUT_MS="${RNODE_BLE_CONNECT_TIMEOUT_MS:-5000}"
RNODE_BLE_MAX_WRITE_LEN="${RNODE_BLE_MAX_WRITE_LEN:-20}"
TIMEOUT_SECS="${RNODE_TIMEOUT_SECS:-${TIMEOUT_SECS:-180}}"
if [[ -z "$RNODE_BAUD_RATE" ]]; then
  RNODE_BAUD_RATE="115200"
fi
if [[ -z "$TIMEOUT_SECS" ]]; then
  TIMEOUT_SECS="180"
fi

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/rnode-hil}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-rnode.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RNODE_PORT" "$RNODE_BAUD_RATE" "$RNODE_FREQUENCY" "$RNODE_BANDWIDTH" "$RNODE_SPREADING_FACTOR" "$RNODE_CODING_RATE" "$RNODE_TX_POWER" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON"
import json
import pathlib
import sys
from urllib.parse import urlparse

(
    report_path,
    status,
    reason,
    rnode_port,
    baud_rate,
    frequency,
    bandwidth,
    spreading_factor,
    coding_rate,
    tx_power,
    rpc_addr,
    run_dir,
    config_path,
    log_path,
    rnstatus_path,
) = sys.argv[1:16]
port_lower = rnode_port.lower()
if port_lower.startswith("tcp://"):
    transport_kind = "tcp"
elif port_lower.startswith("ble://"):
    transport_kind = "ble"
else:
    transport_kind = "serial"
expected_endpoint = rnode_port
if transport_kind == "tcp":
    parsed = urlparse(rnode_port)
    expected_endpoint = parsed.netloc
report = {
    "status": status,
    "rnode_port": rnode_port,
    "transport_kind": transport_kind,
    "expected_endpoint": expected_endpoint,
    "baud_rate": None if transport_kind in {"tcp", "ble"} else int(baud_rate),
    "expected_frequency_hz": int(frequency),
    "expected_bandwidth_hz": int(bandwidth),
    "expected_spreading_factor": int(spreading_factor),
    "expected_coding_rate": int(coding_rate),
    "expected_tx_power_dbm": int(tx_power),
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
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
        row = next(
            (
                item
                for item in rows
                if item.get("type") == "lora"
                and item.get("name") == "rnode-prepared-host"
            ),
            None,
        )
        if row:
            runtime = ((row.get("settings") or {}).get("_runtime") or {})
            status_root = (runtime.get("lora") or {}).get("rnode_status") or {}
            probe = status_root.get("probe_status") or {}
            radio = status_root.get("radio_status") or {}
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_iface"] = runtime.get("iface")
            report["endpoint"] = status_root.get("endpoint")
            report["bearer"] = status_root.get("bearer")
            report["online"] = status_root.get("online")
            report["detected"] = probe.get("detected")
            report["firmware_version"] = probe.get("firmware_version")
            report["platform"] = probe.get("platform")
            report["mcu"] = probe.get("mcu")
            report["radio_state"] = radio.get("radio_state")
            report["frequency_hz"] = radio.get("frequency_hz")
            report["bandwidth_hz"] = radio.get("bandwidth_hz")
            report["spreading_factor"] = radio.get("spreading_factor")
            report["coding_rate"] = radio.get("coding_rate")
            report["tx_power_dbm"] = radio.get("tx_power_dbm")
            report["reported_bitrate_bps"] = status_root.get("reported_bitrate_bps")
            report["hardware_errors"] = status_root.get("hardware_errors")
            report["last_command_error"] = status_root.get("last_command_error")
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
    echo "[rnode-prepared-host-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[rnode-prepared-host-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

if [[ -z "$RNODE_PORT" ]]; then
  fail "RNODE_PORT must name a serial device, tcp://host:port endpoint, or ble://peripheral endpoint"
fi

python3 - <<'PY' "$RNODE_BAUD_RATE" "$TIMEOUT_SECS" "$RNODE_FREQUENCY" "$RNODE_BANDWIDTH" "$RNODE_SPREADING_FACTOR" "$RNODE_CODING_RATE" "$RNODE_TX_POWER" "$RNODE_BITRATE" "$RNODE_COMMAND_TIMEOUT_MS" "$RNODE_BLE_SCAN_TIMEOUT_MS" "$RNODE_BLE_CONNECT_TIMEOUT_MS" "$RNODE_BLE_MAX_WRITE_LEN" || fail "RNode numeric environment is invalid"
import sys
(
    baud_rate,
    timeout_secs,
    frequency,
    bandwidth,
    spreading_factor,
    coding_rate,
    tx_power,
    bitrate,
    command_timeout_ms,
    ble_scan_timeout_ms,
    ble_connect_timeout_ms,
    ble_max_write_len,
) = (int(value) for value in sys.argv[1:13])
if (
    baud_rate <= 0
    or timeout_secs <= 0
    or bitrate <= 0
    or command_timeout_ms <= 0
    or ble_scan_timeout_ms <= 0
    or ble_connect_timeout_ms <= 0
    or ble_max_write_len <= 0
):
    raise SystemExit(1)
if not 137_000_000 <= frequency <= 3_000_000_000:
    raise SystemExit(1)
if not 7_800 <= bandwidth <= 1_625_000:
    raise SystemExit(1)
if not 5 <= spreading_factor <= 12:
    raise SystemExit(1)
if coding_rate not in {5, 6, 7, 8}:
    raise SystemExit(1)
if not 0 <= tx_power <= 37:
    raise SystemExit(1)
PY

if [[ "${RNODE_PORT,,}" == tcp://* ]]; then
  python3 - <<'PY' "$RNODE_PORT" || fail "RNode TCP endpoint did not accept a connection"
import socket
import sys
from urllib.parse import urlparse

parsed = urlparse(sys.argv[1])
if not parsed.hostname or not parsed.port:
    raise SystemExit("tcp endpoint must be tcp://host:port")
with socket.create_connection((parsed.hostname, parsed.port), timeout=5):
    pass
PY
elif [[ "${RNODE_PORT,,}" == ble://* ]]; then
  :
elif [[ ! -e "$RNODE_PORT" ]]; then
  fail "RNode serial device ${RNODE_PORT} does not exist"
fi

python3 - <<'PY' "$CONFIG_PATH" "$RNODE_PORT" "$RNODE_BAUD_RATE" "$RNODE_REGION" "$RNODE_FREQUENCY" "$RNODE_BANDWIDTH" "$RNODE_SPREADING_FACTOR" "$RNODE_CODING_RATE" "$RNODE_TX_POWER" "$RNODE_BITRATE" "$RNODE_COMMAND_TIMEOUT_MS" "$RNODE_BLE_ADAPTER" "$RNODE_BLE_SCAN_TIMEOUT_MS" "$RNODE_BLE_CONNECT_TIMEOUT_MS" "$RNODE_BLE_MAX_WRITE_LEN" || fail "failed to generate RNode config"
import json
import pathlib
import sys

(
    config_path,
    port,
    baud_rate,
    region,
    frequency,
    bandwidth,
    spreading_factor,
    coding_rate,
    tx_power,
    bitrate,
    command_timeout_ms,
    ble_adapter,
    ble_scan_timeout_ms,
    ble_connect_timeout_ms,
    ble_max_write_len,
) = sys.argv[1:16]
fields = [
    'type = "RNodeInterface"',
    "enabled = true",
    'name = "rnode-prepared-host"',
    f"port = {json.dumps(port)}",
]
port_lower = port.lower()
if not (port_lower.startswith("tcp://") or port_lower.startswith("ble://")):
    fields.append(f"baud_rate = {int(baud_rate)}")
if port_lower.startswith("ble://") and ble_adapter:
    fields.append(f"adapter = {json.dumps(ble_adapter)}")
fields.extend(
    [
        f"region = {json.dumps(region)}",
        f"frequency = {int(frequency)}",
        f"bandwidth = {int(bandwidth)}",
        f"spreadingfactor = {int(spreading_factor)}",
        f"codingrate = {int(coding_rate)}",
        f"txpower = {int(tx_power)}",
        f"bitrate = {int(bitrate)}",
        f"command_timeout_ms = {int(command_timeout_ms)}",
        f"scan_timeout_ms = {int(ble_scan_timeout_ms)}",
        f"ble_connect_timeout_ms = {int(ble_connect_timeout_ms)}",
        f"max_write_len = {int(ble_max_write_len)}",
        f"state_path = {json.dumps(str(pathlib.Path(config_path).parent / 'lora-state.json'))}",
    ]
)
lines = ["[[interfaces]]"]
lines.extend(fields)
pathlib.Path(config_path).write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

if [[ "${RNODE_PORT,,}" == ble://* ]]; then
  cargo build -p reticulumd --bin reticulumd --features rnode-ble --quiet
else
  cargo build -p reticulumd --bin reticulumd --quiet
fi
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
    fail "reticulumd exited before RNode status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNODE_PORT" "$RNODE_BAUD_RATE" "$RNODE_FREQUENCY" "$RNODE_BANDWIDTH" "$RNODE_SPREADING_FACTOR" "$RNODE_CODING_RATE" "$RNODE_TX_POWER"
import json
import sys
from urllib.parse import urlparse

(
    path,
    rnode_port,
    baud_rate,
    frequency,
    bandwidth,
    spreading_factor,
    coding_rate,
    tx_power,
) = sys.argv[1:9]
port_lower = rnode_port.lower()
if port_lower.startswith("tcp://"):
    transport_kind = "tcp"
elif port_lower.startswith("ble://"):
    transport_kind = "ble"
else:
    transport_kind = "serial"
expected_endpoint = rnode_port
if transport_kind == "tcp":
    parsed = urlparse(rnode_port)
    expected_endpoint = parsed.netloc
payload = json.load(open(path, "r", encoding="utf-8"))
rows = payload.get("interfaces") or []
row = next(
    (
        item
        for item in rows
        if item.get("type") == "lora"
        and item.get("name") == "rnode-prepared-host"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime_root = (row.get("settings") or {}).get("_runtime") or {}
if runtime_root.get("startup_status") != "spawned":
    raise SystemExit(1)
if not isinstance(runtime_root.get("iface"), str) or not runtime_root.get("iface"):
    raise SystemExit(1)
status = (runtime_root.get("lora") or {}).get("rnode_status") or {}
if status.get("bearer") != transport_kind:
    raise SystemExit(1)
if status.get("endpoint") != expected_endpoint:
    raise SystemExit(1)
if transport_kind == "serial" and status.get("baud_rate") != int(baud_rate):
    raise SystemExit(1)
if transport_kind in {"tcp", "ble"} and status.get("baud_rate") is not None:
    raise SystemExit(1)
probe = status.get("probe_status") or {}
firmware = probe.get("firmware_version") or {}
if probe.get("detected") is not True:
    raise SystemExit(1)
if not isinstance(firmware.get("label"), str):
    raise SystemExit(1)
if probe.get("platform") is None or probe.get("mcu") is None:
    raise SystemExit(1)
radio = status.get("radio_status") or {}
reported_frequency = radio.get("frequency_hz")
if not isinstance(reported_frequency, int) or abs(reported_frequency - int(frequency)) > 100:
    raise SystemExit(1)
for key, expected in [
    ("bandwidth_hz", int(bandwidth)),
    ("spreading_factor", int(spreading_factor)),
    ("coding_rate", int(coding_rate)),
    ("tx_power_dbm", int(tx_power)),
]:
    if radio.get(key) != expected:
        raise SystemExit(1)
if radio.get("radio_state") != 1:
    raise SystemExit(1)
if status.get("online") is not True:
    raise SystemExit(1)
if status.get("last_command_error") is not None:
    raise SystemExit(1)
if status.get("hardware_errors") not in (None, []):
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[rnode-prepared-host-smoke] pass"
      echo "[rnode-prepared-host-smoke] report=${REPORT_PATH}"
      echo "[rnode-prepared-host-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 2
done

fail "timed out waiting for healthy RNode runtime status"
