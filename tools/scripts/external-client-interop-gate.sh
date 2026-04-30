#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

usage() {
  cat >&2 <<'EOF'
Usage: tools/scripts/external-client-interop-gate.sh <meshchatx|sideband|columba> [client-root]

Runs one external-client interoperability proof and emits a stable gate summary.
The selected external client must already exist as a local source checkout.

Environment overrides:
  MESHCHATX_ROOT, SIDEBAND_ROOT, COLUMBA_ROOT
  LOG_DIR, REPORT_PATH, GATE_SUMMARY_PATH
  RPC_ADDR, TRANSPORT_ADDR
EOF
}

CLIENT="${1:-}"
CLIENT_ROOT="${2:-}"

if [[ -z "${CLIENT}" ]]; then
  usage
  exit 2
fi

case "${CLIENT}" in
  meshchatx | sideband | columba) ;;
  *)
    usage
    echo "unknown external client '${CLIENT}'" >&2
    exit 2
    ;;
esac

LOG_DIR="${LOG_DIR:-${REPO_ROOT}/target/interop/external-client-gate/${CLIENT}}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
GATE_SUMMARY_PATH="${GATE_SUMMARY_PATH:-${LOG_DIR}/gate-summary.json}"
mkdir -p "${LOG_DIR}"

if [[ "${REPORT_PATH}" != */* ]]; then
  REPORT_PATH="./${REPORT_PATH}"
fi
if [[ "${GATE_SUMMARY_PATH}" != */* ]]; then
  GATE_SUMMARY_PATH="./${GATE_SUMMARY_PATH}"
fi

case "${CLIENT}" in
  meshchatx)
    MESHCHATX_ROOT="${CLIENT_ROOT:-${MESHCHATX_ROOT:-${REPO_ROOT}/../MeshChatX}}"
    if [[ ! -d "${MESHCHATX_ROOT}" ]]; then
      echo "MeshChatX checkout not found at ${MESHCHATX_ROOT}" >&2
      echo "Pass a checkout path as the second argument or set MESHCHATX_ROOT." >&2
      exit 1
    fi
    export MESHCHATX_ROOT
    LOG_DIR="${LOG_DIR}" REPORT_PATH="${REPORT_PATH}" \
      bash tools/scripts/meshchatx-reticulumd-smoke.sh
    ;;
  sideband)
    SIDEBAND_ROOT="${CLIENT_ROOT:-${SIDEBAND_ROOT:-${REPO_ROOT}/../Sideband}}"
    if [[ ! -d "${SIDEBAND_ROOT}" ]]; then
      echo "Sideband checkout not found at ${SIDEBAND_ROOT}" >&2
      echo "Pass a checkout path as the second argument or set SIDEBAND_ROOT." >&2
      exit 1
    fi
    export SIDEBAND_ROOT
    LOG_DIR="${LOG_DIR}" REPORT_PATH="${REPORT_PATH}" \
      bash tools/scripts/sideband-reticulumd-smoke.sh
    ;;
  columba)
    COLUMBA_ROOT="${CLIENT_ROOT:-${COLUMBA_ROOT:-${REPO_ROOT}/../columba}}"
    if [[ ! -d "${COLUMBA_ROOT}" ]]; then
      echo "Columba checkout not found at ${COLUMBA_ROOT}" >&2
      echo "Pass a checkout path as the second argument or set COLUMBA_ROOT." >&2
      exit 1
    fi
    export COLUMBA_ROOT
    LOG_DIR="${LOG_DIR}" REPORT_PATH="${REPORT_PATH}" \
      bash tools/scripts/columba-reticulumd-smoke.sh
    ;;
esac

python3 - <<'PY' "${CLIENT}" "${REPORT_PATH}" "${GATE_SUMMARY_PATH}"
import json
import os
import sys

client, report_path, summary_path = sys.argv[1:4]

with open(report_path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

def require_text(key):
    value = report.get(key)
    if not isinstance(value, str) or not value:
        raise SystemExit(f"interop report missing non-empty '{key}'")
    return value

def require_artifact(path):
    if not isinstance(path, str) or not path:
        raise SystemExit("interop report contains an empty artifact path")
    if not os.path.exists(path):
        raise SystemExit(f"interop artifact does not exist: {path}")
    return path

if client == "meshchatx":
    external_hash = require_text("meshchatx_hash")
    artifacts = report.get("artifacts")
    if not isinstance(artifacts, dict):
        raise SystemExit("meshchatx report missing artifacts object")
    reticulumd_log = require_artifact(artifacts.get("reticulumd_log"))
    external_log = require_artifact(artifacts.get("meshchatx_log"))
    proof = report.get("proof")
    if not isinstance(proof, dict):
        raise SystemExit("meshchatx report missing proof object")
    for key in ("daemon_to_meshchatx", "meshchatx_to_daemon"):
        if not isinstance(proof.get(key), str) or not proof[key]:
            raise SystemExit(f"meshchatx report missing proof.{key}")
else:
    external_hash = require_text("external_client_hash")
    reticulumd_log = require_artifact(report.get("reticulumd_log"))
    external_log_key = f"{client}_log"
    external_log = require_artifact(report.get(external_log_key))
    for key in ("daemon_to_external_content", "external_to_daemon_content"):
        if not isinstance(report.get(key), str) or not report[key]:
            raise SystemExit(f"{client} report missing {key}")

summary = {
    "status": "pass",
    "client": client,
    "report_path": report_path,
    "daemon_hash": require_text("daemon_hash"),
    "external_client_hash": external_hash,
    "artifacts": {
        "reticulumd_log": reticulumd_log,
        "external_client_log": external_log,
    },
}

summary_dir = os.path.dirname(summary_path)
if summary_dir:
    os.makedirs(summary_dir, exist_ok=True)
with open(summary_path, "w", encoding="utf-8") as handle:
    json.dump(summary, handle, indent=2, sort_keys=True)
PY

echo "[external-client-interop-gate] pass"
echo "[external-client-interop-gate] report=${REPORT_PATH}"
echo "[external-client-interop-gate] summary=${GATE_SUMMARY_PATH}"
