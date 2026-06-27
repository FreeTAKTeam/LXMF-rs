#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

AUTO_IFACE_NAME="${AUTO_IFACE_NAME:-auto-churn-prepared-host}"
AUTO_CHURN_NETNS="${AUTO_CHURN_NETNS:-lxmf-auto-churn-$$}"
AUTO_CHURN_DEVICE="${AUTO_CHURN_DEVICE:-lxauto0}"
AUTO_CHURN_INITIAL_ADDR="${AUTO_CHURN_INITIAL_ADDR:-fe80::1200}"
AUTO_CHURN_REPLACEMENT_ADDR="${AUTO_CHURN_REPLACEMENT_ADDR:-fe80::1201}"
AUTO_CHURN_GROUP_ID="${AUTO_CHURN_GROUP_ID:-reticulum}"
AUTO_CHURN_TIMEOUT_SECS="${AUTO_CHURN_TIMEOUT_SECS:-${TIMEOUT_SECS:-90}}"
AUTO_CHURN_POLL_SECS="${AUTO_CHURN_POLL_SECS:-2}"
IP="${IP:-ip}"

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/auto-interface-hil}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-auto-interface.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
PHASE_DIR="${RUN_DIR}/phases"
mkdir -p "$PHASE_DIR"
: >"$RETICULUMD_LOG"

if [[ -z "${RPC_ADDR:-}" ]]; then
  RPC_ADDR="127.0.0.1:$(
    python3 - <<'PY'
import random
print(random.randint(36000, 48000))
PY
  )"
fi

SUDO=()
if [[ "$(id -u)" -ne 0 ]]; then
  if ! command -v sudo >/dev/null 2>&1; then
    echo "[auto-interface-prepared-host-smoke] ERROR: root or sudo is required for network namespace setup" >&2
    exit 1
  fi
  SUDO=(sudo -n)
fi

run_ip() {
  "${SUDO[@]}" "$IP" "$@"
}

run_ns() {
  run_ip netns exec "$AUTO_CHURN_NETNS" "$@"
}

write_report() {
  local status="$1"
  local reason="${2:-}"
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$AUTO_IFACE_NAME" "$AUTO_CHURN_NETNS" "$AUTO_CHURN_DEVICE" "$AUTO_CHURN_INITIAL_ADDR" "$AUTO_CHURN_REPLACEMENT_ADDR" "$RPC_ADDR" "$RUN_DIR" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$PHASE_DIR"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    iface_name,
    netns,
    device,
    initial_addr,
    replacement_addr,
    rpc_addr,
    run_dir,
    log_path,
    rnstatus_path,
    phase_dir,
) = sys.argv[1:14]
report = {
    "status": status,
    "interface_name": iface_name,
    "netns": netns,
    "device": device,
    "initial_link_local_address": initial_addr,
    "replacement_link_local_address": replacement_addr,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "reticulumd_log": log_path,
    "rnstatus_json": rnstatus_path,
    "phase_snapshots": sorted(str(path) for path in pathlib.Path(phase_dir).glob("*.json")),
}
if reason:
    report["reason"] = reason
status_path = pathlib.Path(rnstatus_path)
if status_path.exists():
    try:
        payload = json.loads(status_path.read_text(encoding="utf-8"))
        row = next(
            (
                item
                for item in payload.get("interfaces", [])
                if item.get("type") == "auto" and item.get("name") == iface_name
            ),
            None,
        )
        if row:
            runtime = ((row.get("settings") or {}).get("_runtime") or {})
            carrier = ((runtime.get("auto") or {}).get("carrier_runtime") or {})
            report["startup_status"] = runtime.get("startup_status")
            report["runtime_status"] = runtime.get("runtime_status")
            report["adopted_device_count"] = carrier.get("adopted_device_count")
            report["adopted_add_count"] = carrier.get("adopted_add_count")
            report["adopted_remove_count"] = carrier.get("adopted_remove_count")
            report["link_local_replacement_count"] = carrier.get("link_local_replacement_count")
            report["last_adopted_change"] = carrier.get("last_adopted_change")
            report["adopted_devices"] = carrier.get("adopted_devices")
            report["link_local_update"] = carrier.get("link_local_update")
    except Exception as exc:
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
  run_ip netns del "$AUTO_CHURN_NETNS" >/dev/null 2>&1 || true
  if [[ $status -ne 0 ]]; then
    echo "[auto-interface-prepared-host-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[auto-interface-prepared-host-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

command -v "$IP" >/dev/null 2>&1 || fail "iproute2 ip command is required"
if [[ ${#SUDO[@]} -gt 0 ]]; then
  "${SUDO[@]}" true >/dev/null 2>&1 || fail "passwordless sudo is required when not running as root"
fi

python3 - <<'PY' "$AUTO_CHURN_TIMEOUT_SECS" "$AUTO_CHURN_POLL_SECS" || fail "AutoInterface churn timing environment is invalid"
import sys
timeout, poll = sys.argv[1:3]
if int(timeout) <= 0 or int(poll) <= 0:
    raise SystemExit(1)
PY

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "AutoInterface"
enabled = true
name = "${AUTO_IFACE_NAME}"
group_id = "${AUTO_CHURN_GROUP_ID}"
devices = ["${AUTO_CHURN_DEVICE}"]
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet

run_ip netns add "$AUTO_CHURN_NETNS"
run_ip -n "$AUTO_CHURN_NETNS" link set lo up

run_ns env RUST_LOG="${RUST_LOG:-reticulumd=debug,info}" \
  "${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$RPC_ADDR" \
  --rpc-unix "$RPC_UNIX" \
  --db "$DB_PATH" \
  --config "$CONFIG_PATH" \
  --strict-interface-startup >"$RETICULUMD_LOG" 2>&1 &
RET_PID=$!

status_phase_ok() {
  local phase="$1"
  local snapshot="${PHASE_DIR}/${phase}.json"
  run_ns "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" || return 1
  cp "$RNSTATUS_JSON" "$snapshot"
  python3 - <<'PY' "$RNSTATUS_JSON" "$phase" "$AUTO_IFACE_NAME" "$AUTO_CHURN_DEVICE" "$AUTO_CHURN_INITIAL_ADDR" "$AUTO_CHURN_REPLACEMENT_ADDR"
import json
import sys

path, phase, iface_name, device, initial_addr, replacement_addr = sys.argv[1:7]
payload = json.load(open(path, "r", encoding="utf-8"))
row = next(
    (
        item
        for item in payload.get("interfaces", [])
        if item.get("type") == "auto" and item.get("name") == iface_name
    ),
    None,
)
if not row:
    raise SystemExit(1)
runtime = (row.get("settings") or {}).get("_runtime") or {}
if runtime.get("startup_status") != "spawned" or runtime.get("runtime_status") != "running":
    raise SystemExit(1)
carrier = ((runtime.get("auto") or {}).get("carrier_runtime") or {})
devices = carrier.get("adopted_devices") or []
addresses_by_name = {
    item.get("ifname"): item.get("link_local_address")
    for item in devices
    if isinstance(item, dict)
}
last = carrier.get("last_adopted_change") or {}
if phase == "zero_initial":
    if carrier.get("adopted_device_count") != 0:
        raise SystemExit(1)
elif phase == "added":
    if carrier.get("adopted_add_count", 0) < 1:
        raise SystemExit(1)
    if addresses_by_name.get(device) != initial_addr:
        raise SystemExit(1)
elif phase == "replaced":
    if carrier.get("link_local_replacement_count", 0) < 1:
        raise SystemExit(1)
    if addresses_by_name.get(device) != replacement_addr:
        raise SystemExit(1)
    if last.get("event") != "link_local_changed":
        raise SystemExit(1)
elif phase == "removed":
    if carrier.get("adopted_remove_count", 0) < 1:
        raise SystemExit(1)
    if carrier.get("adopted_device_count") != 0:
        raise SystemExit(1)
    if last.get("event") != "removed":
        raise SystemExit(1)
else:
    raise SystemExit(1)
PY
}

wait_for_phase() {
  local phase="$1"
  local deadline=$((SECONDS + AUTO_CHURN_TIMEOUT_SECS))
  while (( SECONDS < deadline )); do
    if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
      fail "reticulumd exited before AutoInterface phase ${phase} was observed"
    fi
    if status_phase_ok "$phase"; then
      echo "[auto-interface-prepared-host-smoke] observed ${phase}"
      return 0
    fi
    sleep "$AUTO_CHURN_POLL_SECS"
  done
  fail "timed out waiting for AutoInterface phase ${phase}"
}

wait_for_phase zero_initial

run_ip -n "$AUTO_CHURN_NETNS" link add "$AUTO_CHURN_DEVICE" type dummy
run_ip -n "$AUTO_CHURN_NETNS" link set dev "$AUTO_CHURN_DEVICE" addrgenmode none
run_ip -n "$AUTO_CHURN_NETNS" addr add "${AUTO_CHURN_INITIAL_ADDR}/64" dev "$AUTO_CHURN_DEVICE"
run_ip -n "$AUTO_CHURN_NETNS" link set "$AUTO_CHURN_DEVICE" up
wait_for_phase added

run_ip -n "$AUTO_CHURN_NETNS" addr del "${AUTO_CHURN_INITIAL_ADDR}/64" dev "$AUTO_CHURN_DEVICE"
run_ip -n "$AUTO_CHURN_NETNS" addr add "${AUTO_CHURN_REPLACEMENT_ADDR}/64" dev "$AUTO_CHURN_DEVICE"
wait_for_phase replaced

run_ip -n "$AUTO_CHURN_NETNS" link del "$AUTO_CHURN_DEVICE"
wait_for_phase removed

write_report "pass"
echo "[auto-interface-prepared-host-smoke] pass"
echo "[auto-interface-prepared-host-smoke] report=${REPORT_PATH}"
echo "[auto-interface-prepared-host-smoke] logs=${RUN_DIR}"
