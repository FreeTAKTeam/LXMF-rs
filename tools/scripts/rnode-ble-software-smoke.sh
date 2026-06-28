#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/rnode-ble-software-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
RNODE_BLE_TEST_LOG="${RUN_DIR}/rnode_ble_test.log"
CLOSED_QUEUE_TEST_LOG="${RUN_DIR}/closed_tx_queue_test.log"
RNODECONF_CLI_TEST_LOG="${RUN_DIR}/rnodeconf_cli_test.log"
RETICULUMD_BLE_BRIDGE_TEST_LOG="${RUN_DIR}/reticulumd_ble_bridge_test.log"

write_report() {
  local status="$1"
  local reason="${2:-}"
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RUN_DIR" "$RNODE_BLE_TEST_LOG" "$CLOSED_QUEUE_TEST_LOG" "$RNODECONF_CLI_TEST_LOG" "$RETICULUMD_BLE_BRIDGE_TEST_LOG"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    run_dir,
    rnode_ble_log,
    closed_queue_log,
    rnodeconf_cli_log,
    reticulumd_ble_bridge_log,
) = sys.argv[1:9]
report = {
    "status": status,
    "evidence_scope": "software_rnode_ble_fallback_management",
    "product_boundary": (
        "This proves host-side RNode BLE fallback, Nordic UART framing/chunking, "
        "command-monitor status, management dispatch, rnodeconf-rs extended command-to-RPC "
        "coverage, CLI management guards, and shared closed-queue cleanup through software "
        "regressions only; BLE hardware, firmware, radio, and management operation evidence "
        "still requires prepared-host devices."
    ),
    "run_dir": run_dir,
    "commands": [
        {
            "name": "rnode_ble_feature_regressions",
            "command": "cargo test -p reticulum-rs-transport --features rnode-ble --test rnode_ble",
            "log": rnode_ble_log,
        },
        {
            "name": "shared_closed_tx_queue_cleanup",
            "command": "cargo test -p reticulum-rs-transport closed_tx_queue_stops_and_cleans_up_iface",
            "log": closed_queue_log,
        },
        {
            "name": "rnodeconf_extended_management_cli_matrix",
            "command": "cargo test -p rns-tools --test rnodeconf_cli",
            "log": rnodeconf_cli_log,
        },
        {
            "name": "reticulumd_native_rnode_ble_management_bridge",
            "command": "cargo test -p reticulumd --features rnode-ble bridge_dispatches_native_rnode_ble_management_commands",
            "log": reticulumd_ble_bridge_log,
        },
    ],
    "covered_behaviors": [
        "Python Nordic UART profile defaults",
        "configured RNode BLE identifier and alias matching",
        "configured Android peripheral exclusion during fallback scan",
        "RNode BLE command-monitor startup, degraded fallback, and runtime status JSON",
        "RNode BLE packet, shutdown, and management-frame chunking",
        "RNode BLE management handle queueing",
        "reticulumd daemon RnodeBle management bridge dispatch",
        "rnodeconf-rs extended management command-to-RPC matrix",
        "persistent and destructive RNode management CLI guard enforcement",
        "shared transport cleanup of closed TX queues",
    ],
}
if reason:
    report["reason"] = reason
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

fail() {
  local msg="$1"
  echo "[rnode-ble-software-smoke] ERROR: ${msg}" >&2
  write_report "fail" "$msg"
  exit 1
}

if ! cargo test -p reticulum-rs-transport --features rnode-ble --test rnode_ble >"$RNODE_BLE_TEST_LOG" 2>&1; then
  fail "RNode BLE feature regressions failed; see ${RNODE_BLE_TEST_LOG}"
fi

if ! cargo test -p reticulum-rs-transport closed_tx_queue_stops_and_cleans_up_iface >"$CLOSED_QUEUE_TEST_LOG" 2>&1; then
  fail "shared closed TX queue cleanup regression failed; see ${CLOSED_QUEUE_TEST_LOG}"
fi

if ! cargo test -p rns-tools --test rnodeconf_cli >"$RNODECONF_CLI_TEST_LOG" 2>&1; then
  fail "rnodeconf-rs extended management CLI matrix failed; see ${RNODECONF_CLI_TEST_LOG}"
fi

if ! cargo test -p reticulumd --features rnode-ble bridge_dispatches_native_rnode_ble_management_commands >"$RETICULUMD_BLE_BRIDGE_TEST_LOG" 2>&1; then
  fail "reticulumd native RNode BLE management bridge failed; see ${RETICULUMD_BLE_BRIDGE_TEST_LOG}"
fi

write_report "pass"
echo "[rnode-ble-software-smoke] pass"
echo "[rnode-ble-software-smoke] report=${REPORT_PATH}"
echo "[rnode-ble-software-smoke] logs=${RUN_DIR}"
