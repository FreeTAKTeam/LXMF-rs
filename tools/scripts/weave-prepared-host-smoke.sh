#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

WEAVE_PORT="${WEAVE_PORT:-${WEAVE_DEVICE:-}}"
WEAVE_BAUD_RATE="${WEAVE_BAUD_RATE:-${WEAVE_SPEED:-3000000}}"
WEAVE_MTU="${WEAVE_MTU:-1024}"
WEAVE_CONFIGURED_BITRATE="${WEAVE_CONFIGURED_BITRATE:-250000}"
WEAVE_REQUIRE_CONNECTED="${WEAVE_REQUIRE_CONNECTED:-true}"
WEAVE_REMOTE_DISPLAY_CONTROL="${WEAVE_REMOTE_DISPLAY_CONTROL:-false}"
TIMEOUT_SECS="${WEAVE_TIMEOUT_SECS:-${TIMEOUT_SECS:-180}}"
if [[ -z "$WEAVE_BAUD_RATE" ]]; then
  WEAVE_BAUD_RATE="3000000"
fi
if [[ -z "$WEAVE_MTU" ]]; then
  WEAVE_MTU="1024"
fi
if [[ -z "$WEAVE_CONFIGURED_BITRATE" ]]; then
  WEAVE_CONFIGURED_BITRATE="250000"
fi
if [[ -z "$TIMEOUT_SECS" ]]; then
  TIMEOUT_SECS="180"
fi
if [[ -z "$WEAVE_REQUIRE_CONNECTED" ]]; then
  WEAVE_REQUIRE_CONNECTED="true"
fi
if [[ -z "$WEAVE_REMOTE_DISPLAY_CONTROL" ]]; then
  WEAVE_REMOTE_DISPLAY_CONTROL="false"
fi
REMOTE_DISPLAY_CONTROL_RESULT="not_requested"

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/weave-hil}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-weave.toml"
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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$WEAVE_PORT" "$WEAVE_BAUD_RATE" "$WEAVE_MTU" "$WEAVE_CONFIGURED_BITRATE" "$WEAVE_REQUIRE_CONNECTED" "$WEAVE_REMOTE_DISPLAY_CONTROL" "$REMOTE_DISPLAY_CONTROL_RESULT" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    weave_port,
    baud_rate,
    mtu,
    configured_bitrate,
    require_connected,
    remote_display_control,
    remote_display_control_result,
    rpc_addr,
    run_dir,
    config_path,
    log_path,
    rnstatus_path,
) = sys.argv[1:16]
report = {
    "status": status,
    "weave_port": weave_port,
    "baud_rate": int(baud_rate),
    "mtu": int(mtu),
    "configured_bitrate": int(configured_bitrate),
    "require_connected": require_connected.lower() in {"1", "true", "yes", "on"},
    "remote_display_control_requested": remote_display_control.lower() in {"1", "true", "yes", "on"},
    "remote_display_control_result": remote_display_control_result,
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
                if item.get("type") == "weave"
                and item.get("name") == "weave-prepared-host"
            ),
            None,
        )
        if row:
            runtime = ((row.get("settings") or {}).get("_runtime") or {})
            status_root = (runtime.get("weave") or {}).get("status") or {}
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_iface"] = runtime.get("iface")
            report["runtime_status"] = runtime.get("runtime_status")
            report["link_state"] = status_root.get("link_state")
            report["wdcl_connected"] = status_root.get("wdcl_connected")
            report["last_error"] = status_root.get("last_error")
            report["remote_switch_id"] = status_root.get("remote_switch_id")
            report["local_endpoint_id"] = status_root.get("local_endpoint_id")
            report["endpoint_count"] = status_root.get("endpoint_count")
            report["bytes_rx"] = status_root.get("bytes_rx")
            report["bytes_tx"] = status_root.get("bytes_tx")
            report["frames_rx"] = status_root.get("frames_rx")
            report["frames_tx"] = status_root.get("frames_tx")
            report["invalid_frames"] = status_root.get("invalid_frames")
            report["last_log_event"] = status_root.get("last_log_event")
            report["log_events"] = status_root.get("log_events")
            report["display"] = status_root.get("display")
            report["device_stats"] = status_root.get("device_stats")
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
    echo "[weave-prepared-host-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[weave-prepared-host-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

if [[ -z "$WEAVE_PORT" ]]; then
  fail "WEAVE_PORT or WEAVE_DEVICE must name a Weave serial device"
fi

python3 - <<'PY' "$WEAVE_BAUD_RATE" "$WEAVE_MTU" "$WEAVE_CONFIGURED_BITRATE" "$TIMEOUT_SECS" || fail "Weave numeric environment is invalid"
import sys
baud_rate, mtu, configured_bitrate, timeout_secs = (int(value) for value in sys.argv[1:5])
if baud_rate <= 0 or configured_bitrate <= 0 or timeout_secs <= 0:
    raise SystemExit(1)
if not 256 <= mtu <= 32768:
    raise SystemExit(1)
PY

if [[ ! -e "$WEAVE_PORT" ]]; then
  fail "Weave serial device ${WEAVE_PORT} does not exist"
fi

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "WeaveInterface"
enabled = true
name = "weave-prepared-host"
port = "${WEAVE_PORT}"
speed = ${WEAVE_BAUD_RATE}
mtu = ${WEAVE_MTU}
configured_bitrate = ${WEAVE_CONFIGURED_BITRATE}
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet
if [[ "$WEAVE_REMOTE_DISPLAY_CONTROL" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
  cargo build -p rns-tools --bin weaveconf-rs --quiet
fi

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
    fail "reticulumd exited before Weave status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$WEAVE_PORT" "$WEAVE_BAUD_RATE" "$WEAVE_MTU" "$WEAVE_REQUIRE_CONNECTED"
import json
import sys

path, expected_device, expected_baud_rate, expected_mtu, require_connected_raw = sys.argv[1:6]
require_connected = require_connected_raw.lower() in {"1", "true", "yes", "on"}
payload = json.load(open(path, "r", encoding="utf-8"))
rows = payload.get("interfaces") or []
row = next(
    (
        item
        for item in rows
        if item.get("type") == "weave"
        and item.get("name") == "weave-prepared-host"
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
status = (runtime_root.get("weave") or {}).get("status") or {}
if status.get("device") != expected_device:
    raise SystemExit(1)
if status.get("baud_rate") != int(expected_baud_rate):
    raise SystemExit(1)
if status.get("mtu") != int(expected_mtu):
    raise SystemExit(1)
if status.get("last_error") is not None:
    raise SystemExit(1)
if (status.get("frames_tx") or 0) < 1 or (status.get("bytes_tx") or 0) < 1:
    raise SystemExit(1)
link_state = status.get("link_state")
if require_connected:
    if link_state != "connected":
        raise SystemExit(1)
    if status.get("wdcl_connected") is not True:
        raise SystemExit(1)
    if not isinstance(status.get("remote_switch_id"), str):
        raise SystemExit(1)
else:
    if link_state not in {"discovering", "connected"}:
        raise SystemExit(1)
PY
    then
      if [[ "$WEAVE_REMOTE_DISPLAY_CONTROL" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
        if [[ ! "$WEAVE_REQUIRE_CONNECTED" =~ ^(1|true|TRUE|yes|YES|on|ON)$ ]]; then
          fail "WEAVE_REMOTE_DISPLAY_CONTROL requires WEAVE_REQUIRE_CONNECTED=true"
        fi
        REMOTE_DISPLAY_CONTROL_RESULT="attempted"
        "${ROOT_DIR}/target/debug/weaveconf-rs" \
          --rpc "$RPC_ADDR" \
          enable-remote-display \
          --interface weave-prepared-host >>"$RETICULUMD_LOG" 2>&1 \
          || fail "weaveconf-rs enable-remote-display failed"
        "${ROOT_DIR}/target/debug/weaveconf-rs" \
          --rpc "$RPC_ADDR" \
          disable-remote-display \
          --interface weave-prepared-host >>"$RETICULUMD_LOG" 2>&1 \
          || fail "weaveconf-rs disable-remote-display failed"
        REMOTE_DISPLAY_CONTROL_RESULT="enable_disable_ok"
        "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
          || fail "rnstatus-rs refresh after Weave remote-display control failed"
      fi
      write_report "pass"
      echo "[weave-prepared-host-smoke] pass"
      echo "[weave-prepared-host-smoke] report=${REPORT_PATH}"
      echo "[weave-prepared-host-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 2
done

fail "timed out waiting for healthy Weave runtime status"
