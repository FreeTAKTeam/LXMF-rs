#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

ALLOW_PARTIAL=false
AUDIT_EXISTING=false
RUN_LOCAL_SMOKES="${RIF_RUN_LOCAL_SMOKES:-auto}"
for arg in "$@"; do
  case "$arg" in
    --allow-partial)
      ALLOW_PARTIAL=true
      ;;
    --audit-existing)
      AUDIT_EXISTING=true
      ;;
    --run-local-smokes)
      RUN_LOCAL_SMOKES=true
      ;;
    *)
      echo "[reticulum-interface-hil-matrix] ERROR: unknown argument: $arg" >&2
      echo "usage: $0 [--allow-partial] [--audit-existing] [--run-local-smokes]" >&2
      exit 2
      ;;
  esac
done

if [[ "$RUN_LOCAL_SMOKES" == "auto" ]]; then
  if [[ "$ALLOW_PARTIAL" != true && "$AUDIT_EXISTING" != true ]]; then
    RUN_LOCAL_SMOKES=true
  else
    RUN_LOCAL_SMOKES=false
  fi
fi

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/reticulum-interface-hil-matrix}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
RNODE_MATRIX_DIR="${RNODE_MATRIX_DIR:-${ROOT_DIR}/target/rnode-hil/matrix}"
AUDIT_REPORT_PATH="${AUDIT_REPORT_PATH:-${LOG_DIR}/parity-audit-report.json}"
ARTIFACT_MANIFEST_PATH="${ARTIFACT_MANIFEST_PATH:-${LOG_DIR}/artifact-manifest.json}"
mkdir -p "$LOG_DIR" "$RNODE_MATRIX_DIR"

SERIAL_PORT="${RIF_RNODE_SERIAL_PORT:-${RNODE_SERIAL_PORT:-}}"
TCP_PORT="${RIF_RNODE_TCP_PORT:-${RNODE_TCP_PORT:-}}"
BLE_PORT="${RIF_RNODE_BLE_PORT:-${RNODE_BLE_PORT:-}}"

bearer_env_value() {
  local bearer_upper="$1"
  local key="$2"
  local fallback_var="$3"
  if [[ "$bearer_upper" == "BLE" && "$key" == BLE_* ]]; then
    local alias_var="RIF_RNODE_BLE_${key#BLE_}"
    local alias_value="${!alias_var-}"
    if [[ -n "$alias_value" ]]; then
      printf '%s' "$alias_value"
      return 0
    fi
  fi
  local override_var="RIF_RNODE_${bearer_upper}_${key}"
  local override_value="${!override_var-}"
  if [[ -n "$override_value" ]]; then
    printf '%s' "$override_value"
  else
    printf '%s' "${!fallback_var-}"
  fi
}

write_report() {
  local status="$1"
  local reason="${2:-}"
  RUN_LOCAL_SMOKES_EFFECTIVE="$RUN_LOCAL_SMOKES" python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$LOG_DIR" "$RNODE_MATRIX_DIR" "$AUDIT_REPORT_PATH" "$ARTIFACT_MANIFEST_PATH" "$SERIAL_PORT" "$TCP_PORT" "$BLE_PORT" "$AUDIT_EXISTING" "$ALLOW_PARTIAL"
import hashlib
import json
import os
import pathlib
import sys

(
    report_path,
    status,
    reason,
    log_dir,
    rnode_matrix_dir,
    audit_report_path,
    artifact_manifest_path,
    serial_port,
    tcp_port,
    ble_port,
    audit_existing,
    allow_partial,
) = sys.argv[1:13]
root = pathlib.Path.cwd()
matrix_dir = pathlib.Path(rnode_matrix_dir)


def read_json(path):
    value_path = pathlib.Path(path)
    if not value_path.exists():
        return None
    try:
        return json.loads(value_path.read_text(encoding="utf-8"))
    except Exception as exc:
        return {"parse_error": str(exc)}

def rel_path(path):
    value_path = pathlib.Path(path)
    if value_path.is_absolute() and root in value_path.parents:
        return str(value_path.relative_to(root))
    return str(value_path)


def artifact_entry(path, role):
    value_path = pathlib.Path(path)
    entry = {
        "role": role,
        "path": rel_path(value_path),
        "exists": value_path.exists(),
    }
    if value_path.exists():
        data = value_path.read_bytes()
        entry["sha256"] = hashlib.sha256(data).hexdigest()
        entry["bytes"] = len(data)
    return entry


reports = {}
for bearer in ["serial", "tcp", "ble"]:
    path = matrix_dir / f"{bearer}.report.json"
    payload = read_json(path)
    reports[bearer] = {
        "path": rel_path(path),
        "report": payload,
        "status": payload.get("status") if isinstance(payload, dict) else "missing",
        "evidence_scope": payload.get("evidence_scope") if isinstance(payload, dict) else None,
    }

override_keys = [
    "BAUD_RATE",
    "REGION",
    "FREQUENCY",
    "BANDWIDTH",
    "SPREADING_FACTOR",
    "CODING_RATE",
    "TX_POWER",
    "BITRATE",
    "COMMAND_TIMEOUT_MS",
    "MAX_PAYLOAD_BYTES",
    "BLE_ADAPTER",
    "BLE_SCAN_TIMEOUT_MS",
    "BLE_CONNECT_TIMEOUT_MS",
    "BLE_MAX_WRITE_LEN",
    "MANAGEMENT_TIMEOUT_SECS",
    "BLINK_PATTERN",
    "TIMEOUT_SECS",
]
configured_overrides = {}
for bearer in ["serial", "tcp", "ble"]:
    prefix = f"RIF_RNODE_{bearer.upper()}_"
    values = {}
    for key in override_keys:
        value = os.environ.get(prefix + key)
        if bearer == "ble" and key.startswith("BLE_"):
            value = os.environ.get(prefix + key.removeprefix("BLE_")) or value
        if value not in (None, ""):
            values[key] = value
    configured_overrides[bearer] = values

report = {
    "status": status,
    "evidence_scope": "reticulum_interfaces_384_385_hil_matrix",
    "product_boundary": (
        "This matrix runner collects serial, TCP/Wi-Fi, and BLE prepared-host "
        "RNode reports, then delegates full #384/#385 proof to the Reticulum "
        "interface parity audit. Software-only RNode evidence is recorded but "
        "does not replace the hardware matrix."
    ),
    "local_smoke_policy": (
        "Strict matrix runs refresh LocalInterface #384 smokes by default so "
        "clean HIL runners have the full evidence set before --require-full; "
        "partial and audit-existing modes skip them unless requested."
    ),
    "reason": reason or None,
    "configured_ports": {
        "serial": serial_port or None,
        "tcp": tcp_port or None,
        "ble": ble_port or None,
    },
    "supported_override_examples": [
        "RIF_RNODE_SERIAL_FREQUENCY",
        "RIF_RNODE_TCP_FREQUENCY",
        "RIF_RNODE_BLE_FREQUENCY",
        "RIF_RNODE_BLE_ADAPTER",
    ],
    "per_bearer_overrides": configured_overrides,
    "audit_existing": audit_existing == "true",
    "allow_partial": allow_partial == "true",
    "run_local_smokes": os.environ.get("RUN_LOCAL_SMOKES_EFFECTIVE"),
    "log_dir": log_dir,
    "rnode_matrix_dir": rel_path(matrix_dir),
    "expected_rnode_reports": [
        "serial.report.json",
        "tcp.report.json",
        "ble.report.json",
    ],
    "evidence_requirement": "prepared-host RNode reports",
    "required_hardware_identity_fields": [
        "report_schema",
        "captured_at_utc",
        "captured_by_host",
        "script",
        "endpoint",
        "transport_kind",
        "bearer",
        "detected",
        "firmware_version.label",
        "platform",
        "mcu",
    ],
    "rnode_reports": reports,
    "audit_report_path": audit_report_path,
    "artifact_manifest_path": artifact_manifest_path,
    "audit_report": read_json(audit_report_path),
}
pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
artifacts = [
    artifact_entry(report_path, "matrix_report"),
    artifact_entry(audit_report_path, "parity_audit_report"),
]
for bearer in ["serial", "tcp", "ble"]:
    artifacts.append(artifact_entry(matrix_dir / f"{bearer}.report.json", f"rnode_{bearer}_report"))
manifest = {
    "schema": "reticulum_interface_hil_matrix_artifacts.v1",
    "evidence_scope": "reticulum_interfaces_384_385_hil_matrix",
    "matrix_report": rel_path(report_path),
    "artifacts": artifacts,
}
pathlib.Path(artifact_manifest_path).write_text(
    json.dumps(manifest, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY
}

fail() {
  local msg="$1"
  echo "[reticulum-interface-hil-matrix] ERROR: ${msg}" >&2
  write_report "fail" "$msg"
  exit 1
}

run_local_smokes() {
  echo "[reticulum-interface-hil-matrix] running LocalInterface #384 smokes"
  TIMEOUT_SECS="${TIMEOUT_SECS:-45}" ./tools/scripts/local-interface-smoke.sh
  TIMEOUT_SECS="${TIMEOUT_SECS:-45}" ./tools/scripts/local-interface-unix-smoke.sh
  TIMEOUT_SECS="${TIMEOUT_SECS:-45}" ./tools/scripts/local-interface-python-shared-smoke.sh
}

run_rnode_bearer() {
  local bearer="$1"
  local port="$2"
  local bearer_upper="${bearer^^}"
  local report="${RNODE_MATRIX_DIR}/${bearer}.report.json"
  local bearer_log_dir="${LOG_DIR}/rnode-${bearer}"
  mkdir -p "$bearer_log_dir"
  echo "[reticulum-interface-hil-matrix] running RNode ${bearer} prepared-host smoke"
  LOG_DIR="$bearer_log_dir" \
    REPORT_PATH="$report" \
    RNODE_PORT="$port" \
    RNODE_BAUD_RATE="$(bearer_env_value "$bearer_upper" BAUD_RATE RNODE_BAUD_RATE)" \
    RNODE_REGION="$(bearer_env_value "$bearer_upper" REGION RNODE_REGION)" \
    RNODE_FREQUENCY="$(bearer_env_value "$bearer_upper" FREQUENCY RNODE_FREQUENCY)" \
    RNODE_BANDWIDTH="$(bearer_env_value "$bearer_upper" BANDWIDTH RNODE_BANDWIDTH)" \
    RNODE_SPREADING_FACTOR="$(bearer_env_value "$bearer_upper" SPREADING_FACTOR RNODE_SPREADING_FACTOR)" \
    RNODE_CODING_RATE="$(bearer_env_value "$bearer_upper" CODING_RATE RNODE_CODING_RATE)" \
    RNODE_TX_POWER="$(bearer_env_value "$bearer_upper" TX_POWER RNODE_TX_POWER)" \
    RNODE_BITRATE="$(bearer_env_value "$bearer_upper" BITRATE RNODE_BITRATE)" \
    RNODE_COMMAND_TIMEOUT_MS="$(bearer_env_value "$bearer_upper" COMMAND_TIMEOUT_MS RNODE_COMMAND_TIMEOUT_MS)" \
    RNODE_MAX_PAYLOAD_BYTES="$(bearer_env_value "$bearer_upper" MAX_PAYLOAD_BYTES RNODE_MAX_PAYLOAD_BYTES)" \
    RNODE_BLE_ADAPTER="$(bearer_env_value "$bearer_upper" BLE_ADAPTER RNODE_BLE_ADAPTER)" \
    RNODE_BLE_SCAN_TIMEOUT_MS="$(bearer_env_value "$bearer_upper" BLE_SCAN_TIMEOUT_MS RNODE_BLE_SCAN_TIMEOUT_MS)" \
    RNODE_BLE_CONNECT_TIMEOUT_MS="$(bearer_env_value "$bearer_upper" BLE_CONNECT_TIMEOUT_MS RNODE_BLE_CONNECT_TIMEOUT_MS)" \
    RNODE_BLE_MAX_WRITE_LEN="$(bearer_env_value "$bearer_upper" BLE_MAX_WRITE_LEN RNODE_BLE_MAX_WRITE_LEN)" \
    RNODE_MANAGEMENT_TIMEOUT_SECS="$(bearer_env_value "$bearer_upper" MANAGEMENT_TIMEOUT_SECS RNODE_MANAGEMENT_TIMEOUT_SECS)" \
    RNODE_BLINK_PATTERN="$(bearer_env_value "$bearer_upper" BLINK_PATTERN RNODE_BLINK_PATTERN)" \
    RNODE_TIMEOUT_SECS="$(bearer_env_value "$bearer_upper" TIMEOUT_SECS RNODE_TIMEOUT_SECS)" \
    ./tools/scripts/rnode-prepared-host-smoke.sh
}

missing=()
[[ -n "$SERIAL_PORT" ]] || missing+=("RIF_RNODE_SERIAL_PORT")
[[ -n "$TCP_PORT" ]] || missing+=("RIF_RNODE_TCP_PORT")
[[ -n "$BLE_PORT" ]] || missing+=("RIF_RNODE_BLE_PORT")
if [[ ${#missing[@]} -gt 0 && "$ALLOW_PARTIAL" != true && "$AUDIT_EXISTING" != true ]]; then
  fail "missing required endpoint env vars for strict matrix: ${missing[*]}"
fi

if [[ "$RUN_LOCAL_SMOKES" == true && "$AUDIT_EXISTING" != true ]]; then
  run_local_smokes
fi

if [[ -n "$SERIAL_PORT" && "$AUDIT_EXISTING" != true ]]; then
  run_rnode_bearer "serial" "$SERIAL_PORT"
fi
if [[ -n "$TCP_PORT" && "$AUDIT_EXISTING" != true ]]; then
  run_rnode_bearer "tcp" "$TCP_PORT"
fi
if [[ -n "$BLE_PORT" && "$AUDIT_EXISTING" != true ]]; then
  run_rnode_bearer "ble" "$BLE_PORT"
fi

rnode_reports=()
for bearer in serial tcp ble; do
  report="${RNODE_MATRIX_DIR}/${bearer}.report.json"
  if [[ -f "$report" ]]; then
    rnode_reports+=("$report")
  fi
done

if [[ ${#rnode_reports[@]} -eq 0 ]]; then
  fail "no RNode prepared-host reports were collected"
fi

rnode_hil_reports="$(
  IFS=:
  echo "${rnode_reports[*]}"
)"

if [[ "$ALLOW_PARTIAL" == true ]]; then
  RNODE_HIL_REPORTS="$rnode_hil_reports" \
    REPORT_PATH="$AUDIT_REPORT_PATH" \
    ./tools/scripts/reticulum-interface-parity-audit.sh
  write_report "partial"
  echo "[reticulum-interface-hil-matrix] partial"
else
  RNODE_HIL_REPORTS="$rnode_hil_reports" \
    REPORT_PATH="$AUDIT_REPORT_PATH" \
    ./tools/scripts/reticulum-interface-parity-audit.sh --require-full
  write_report "pass"
  echo "[reticulum-interface-hil-matrix] pass"
fi
echo "[reticulum-interface-hil-matrix] report=${REPORT_PATH}"
echo "[reticulum-interface-hil-matrix] audit_report=${AUDIT_REPORT_PATH}"
