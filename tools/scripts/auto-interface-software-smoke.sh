#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/auto-interface-software-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
TRANSPORT_TEST_LOG="${RUN_DIR}/rns_transport_auto_tests.log"
RETICULUMD_TEST_LOG="${RUN_DIR}/reticulumd_auto_tests.log"

write_report() {
  local status="$1"
  local reason="${2:-}"
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RUN_DIR" "$TRANSPORT_TEST_LOG" "$RETICULUMD_TEST_LOG"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    run_dir,
    transport_test_log,
    reticulumd_test_log,
) = sys.argv[1:7]
report = {
    "status": status,
    "evidence_scope": "software_auto_interface_runtime",
    "product_boundary": (
        "This proves AutoInterface protocol helpers, discovery state, final-init "
        "gating, peer-data routing, carrier runtime status, and daemon lifecycle "
        "planning through software-only Rust tests; Linux namespace churn, real "
        "Wi-Fi/Ethernet churn, hardware/radio discovery, public-network soak, "
        "and external-client evidence remain outside this smoke."
    ),
    "run_dir": run_dir,
    "commands": [
        {
            "name": "transport_auto_interface_protocol_helpers",
            "command": "cargo test -p reticulum-rs-transport auto --lib",
            "log": transport_test_log,
        },
        {
            "name": "reticulumd_auto_interface_runtime",
            "command": "cargo test -p reticulumd auto_ --bin reticulumd",
            "log": reticulumd_test_log,
        },
    ],
    "covered_behaviors": [
        "Python-compatible AutoInterface multicast address and peering token helpers",
        "adopted-device filtering and link-local address selection",
        "discovery state, authenticated peer acceptance, local echo tracking, and invalid-token rejection",
        "Python final-init gating for discovery and peer-data datagrams",
        "peer lifecycle jobs, reverse announces, multicast echo timeout, and carrier events",
        "peer-data duplicate suppression, known-peer admission, and forwarding outcomes",
        "daemon runtime JSON peer-data admitted, duplicate, unknown, delivered, decode-failed, rx-closed, and last forwarding status",
        "daemon discovery/data listener planning, supervisor restart, and stop-task tracking",
        "daemon outbound peer-data route registration, removal, and refresh behavior",
        "daemon runtime JSON for zero-initial startup, adopted interface churn, and last peer-job status",
        "boundary excludes Linux namespace churn, real Wi-Fi/Ethernet churn, hardware/radio discovery, public-network soak, and external-client evidence",
    ],
}
if reason:
    report["reason"] = reason
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
}

fail() {
  local msg="$1"
  echo "[auto-interface-software-smoke] ERROR: ${msg}" >&2
  write_report "fail" "$msg"
  exit 1
}

if ! cargo test -p reticulum-rs-transport auto --lib >"$TRANSPORT_TEST_LOG" 2>&1; then
  fail "reticulum-rs-transport AutoInterface software regressions failed; see ${TRANSPORT_TEST_LOG}"
fi

if ! cargo test -p reticulumd auto_ --bin reticulumd >"$RETICULUMD_TEST_LOG" 2>&1; then
  fail "reticulumd AutoInterface runtime regressions failed; see ${RETICULUMD_TEST_LOG}"
fi

write_report "pass"
echo "[auto-interface-software-smoke] pass"
echo "[auto-interface-software-smoke] report=${REPORT_PATH}"
echo "[auto-interface-software-smoke] logs=${RUN_DIR}"
