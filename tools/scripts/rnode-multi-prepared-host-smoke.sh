#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

RNODE_MULTI_PORT="${RNODE_MULTI_PORT:-}"
RNODE_MULTI_BAUD_RATE="${RNODE_MULTI_BAUD_RATE:-${RNODE_MULTI_SPEED:-115200}}"
RNODE_MULTI_VPORTS="${RNODE_MULTI_VPORTS:-0,1}"
RNODE_MULTI_REGION="${RNODE_MULTI_REGION:-US915}"
RNODE_MULTI_FREQUENCIES="${RNODE_MULTI_FREQUENCIES:-915000000}"
RNODE_MULTI_BANDWIDTHS="${RNODE_MULTI_BANDWIDTHS:-125000}"
RNODE_MULTI_SPREADING_FACTORS="${RNODE_MULTI_SPREADING_FACTORS:-9}"
RNODE_MULTI_CODING_RATES="${RNODE_MULTI_CODING_RATES:-5}"
RNODE_MULTI_TX_POWERS="${RNODE_MULTI_TX_POWERS:-17}"
RNODE_MULTI_OUTGOING="${RNODE_MULTI_OUTGOING:-true}"
TIMEOUT_SECS="${RNODE_MULTI_TIMEOUT_SECS:-${TIMEOUT_SECS:-180}}"
if [[ -z "$RNODE_MULTI_BAUD_RATE" ]]; then
  RNODE_MULTI_BAUD_RATE="115200"
fi
if [[ -z "$TIMEOUT_SECS" ]]; then
  TIMEOUT_SECS="180"
fi

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/rnode-multi-hil}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-rnode-multi.toml"
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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RNODE_MULTI_PORT" "$RNODE_MULTI_BAUD_RATE" "$RNODE_MULTI_VPORTS" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    rnode_multi_port,
    baud_rate,
    expected_vports_raw,
    rpc_addr,
    run_dir,
    config_path,
    log_path,
    rnstatus_path,
) = sys.argv[1:12]
expected_vports = [int(item.strip()) for item in expected_vports_raw.split(",") if item.strip()]
transport_kind = "tcp" if rnode_multi_port.lower().startswith("tcp://") else "serial"
report = {
    "status": status,
    "rnode_multi_port": rnode_multi_port,
    "transport_kind": transport_kind,
    "baud_rate": None if transport_kind == "tcp" else int(baud_rate),
    "expected_vports": expected_vports,
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
                if item.get("type") == "rnode_multi"
                and item.get("name") == "rnode-multi-prepared-host"
            ),
            None,
        )
        if row:
            runtime = ((row.get("settings") or {}).get("_runtime") or {})
            rnode = runtime.get("rnode_multi") or {}
            radio = rnode.get("radio_status") or {}
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_status"] = runtime.get("runtime_status")
            report["subinterface_count"] = rnode.get("subinterface_count")
            report["stream_state"] = radio.get("stream_state")
            report["last_error"] = radio.get("last_error")
            report["selected_vport"] = radio.get("selected_vport")
            report["runtime_vports"] = radio.get("vports")
            report["startup_probe"] = radio.get("startup_probe")
            report["runtime_subinterfaces"] = sorted((radio.get("subinterfaces") or {}).keys())
            report["radio_status"] = radio.get("subinterfaces")
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
    echo "[rnode-multi-prepared-host-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[rnode-multi-prepared-host-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

if [[ -z "$RNODE_MULTI_PORT" ]]; then
  fail "RNODE_MULTI_PORT must name a serial device or tcp://host:port endpoint"
fi

python3 - <<'PY' "$RNODE_MULTI_BAUD_RATE" "$TIMEOUT_SECS" "$RNODE_MULTI_VPORTS" || fail "RNodeMulti numeric environment is invalid"
import sys
baud_rate, timeout_secs, vports = sys.argv[1:4]
if int(baud_rate) <= 0 or int(timeout_secs) <= 0:
    raise SystemExit(1)
seen = set()
for raw in vports.split(","):
    value = int(raw.strip())
    if value < 0 or value > 255 or value in seen:
        raise SystemExit(1)
    seen.add(value)
if not seen:
    raise SystemExit(1)
PY

if [[ "${RNODE_MULTI_PORT,,}" == tcp://* ]]; then
  python3 - <<'PY' "$RNODE_MULTI_PORT" || fail "RNodeMulti TCP endpoint did not accept a connection"
import socket
import sys
from urllib.parse import urlparse

parsed = urlparse(sys.argv[1])
if not parsed.hostname or not parsed.port:
    raise SystemExit("tcp endpoint must be tcp://host:port")
with socket.create_connection((parsed.hostname, parsed.port), timeout=5):
    pass
PY
elif [[ ! -e "$RNODE_MULTI_PORT" ]]; then
  fail "RNodeMulti serial device ${RNODE_MULTI_PORT} does not exist"
fi

python3 - <<'PY' "$CONFIG_PATH" "$RNODE_MULTI_PORT" "$RNODE_MULTI_BAUD_RATE" "$RNODE_MULTI_VPORTS" "$RNODE_MULTI_REGION" "$RNODE_MULTI_FREQUENCIES" "$RNODE_MULTI_BANDWIDTHS" "$RNODE_MULTI_SPREADING_FACTORS" "$RNODE_MULTI_CODING_RATES" "$RNODE_MULTI_TX_POWERS" "$RNODE_MULTI_OUTGOING" || fail "failed to generate RNodeMulti config"
import pathlib
import sys

(
    config_path,
    port,
    baud_rate,
    vports_raw,
    region,
    freqs_raw,
    bandwidths_raw,
    sfs_raw,
    crs_raw,
    txs_raw,
    outgoing_raw,
) = sys.argv[1:12]

def parse_list(raw):
    return [item.strip() for item in raw.split(",") if item.strip()]

def expand(name, raw, count):
    values = parse_list(raw)
    if len(values) == 1:
        return values * count
    if len(values) != count:
        raise SystemExit(f"{name} must contain one value or {count} comma-separated values")
    return values

vports = [int(value) for value in parse_list(vports_raw)]
count = len(vports)
frequencies = expand("RNODE_MULTI_FREQUENCIES", freqs_raw, count)
bandwidths = expand("RNODE_MULTI_BANDWIDTHS", bandwidths_raw, count)
spreading_factors = expand("RNODE_MULTI_SPREADING_FACTORS", sfs_raw, count)
coding_rates = expand("RNODE_MULTI_CODING_RATES", crs_raw, count)
tx_powers = expand("RNODE_MULTI_TX_POWERS", txs_raw, count)
outgoing = expand("RNODE_MULTI_OUTGOING", outgoing_raw, count)

lines = [
    "[[interfaces]]",
    'type = "RNodeMultiInterface"',
    "enabled = true",
    'name = "rnode-multi-prepared-host"',
    f'port = "{port}"',
]
if not port.lower().startswith("tcp://"):
    lines.append(f"speed = {int(baud_rate)}")
lines.append("configured_bitrate = 1200")
for index, vport in enumerate(vports):
    enabled = "true" if outgoing[index].lower() in {"1", "true", "yes", "on"} else "false"
    coding_rate = coding_rates[index]
    coding_rate_value = coding_rate if coding_rate.isdigit() else f'"{coding_rate}"'
    radio_fields = [
        f'name = "rnode-multi-v{vport}"',
        f"vport = {vport}",
        f'region = "{region}"',
        f"frequency = {int(frequencies[index])}",
        f"bandwidth = {int(bandwidths[index])}",
        f"spreadingfactor = {int(spreading_factors[index])}",
        f"codingrate = {coding_rate_value}",
        f"txpower = {int(tx_powers[index])}",
        f"outgoing = {enabled}",
    ]
    lines.append(f"radio{index} = {{ {', '.join(radio_fields)} }}")
text = "\n".join(lines) + "\n"
pathlib.Path(config_path).write_text(text, encoding="utf-8")
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
    fail "reticulumd exited before RNodeMulti status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNODE_MULTI_VPORTS"
import json
import sys

path, vports_raw = sys.argv[1:3]
expected_vports = [int(item.strip()) for item in vports_raw.split(",") if item.strip()]
payload = json.load(open(path, "r", encoding="utf-8"))
rows = payload.get("interfaces") or []
row = next(
    (
        item
        for item in rows
        if item.get("type") == "rnode_multi"
        and item.get("name") == "rnode-multi-prepared-host"
    ),
    None,
)
if row is None:
    raise SystemExit(1)
runtime_root = (row.get("settings") or {}).get("_runtime") or {}
if runtime_root.get("startup_status") != "spawned":
    raise SystemExit(1)
runtime = runtime_root.get("rnode_multi") or {}
if runtime.get("subinterface_count") != len(expected_vports):
    raise SystemExit(1)
radio = runtime.get("radio_status") or {}
if radio.get("stream_state") != "running":
    raise SystemExit(1)
if radio.get("last_error") is not None:
    raise SystemExit(1)
if radio.get("vports") != expected_vports:
    raise SystemExit(1)
probe = radio.get("startup_probe") or {}
firmware = probe.get("firmware_version") or {}
if not probe.get("detected"):
    raise SystemExit(1)
if not firmware.get("label"):
    raise SystemExit(1)
if probe.get("platform") is None:
    raise SystemExit(1)
if probe.get("mcu") is None:
    raise SystemExit(1)
probe_interfaces = probe.get("interfaces") or {}
if sorted(probe_interfaces.keys()) != [str(vport) for vport in expected_vports]:
    raise SystemExit(1)
subinterfaces = radio.get("subinterfaces") or {}
if sorted(subinterfaces.keys()) != [str(vport) for vport in expected_vports]:
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[rnode-multi-prepared-host-smoke] pass"
      echo "[rnode-multi-prepared-host-smoke] report=${REPORT_PATH}"
      echo "[rnode-multi-prepared-host-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 2
done

fail "timed out waiting for healthy RNodeMulti runtime status"
