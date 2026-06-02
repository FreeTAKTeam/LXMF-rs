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
    EXTERNAL_CLIENT_ROOT="${MESHCHATX_ROOT}"
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
    EXTERNAL_CLIENT_ROOT="${SIDEBAND_ROOT}"
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
    EXTERNAL_CLIENT_ROOT="${COLUMBA_ROOT}"
    LOG_DIR="${LOG_DIR}" REPORT_PATH="${REPORT_PATH}" \
      bash tools/scripts/columba-reticulumd-smoke.sh
    ;;
esac

python3 - <<'PY' "${CLIENT}" "${REPORT_PATH}" "${GATE_SUMMARY_PATH}" "${EXTERNAL_CLIENT_ROOT}"
import json
import os
import subprocess
import sys

client, report_path, summary_path, client_root = sys.argv[1:5]

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

def optional_artifact(path):
    if isinstance(path, str) and path and os.path.exists(path):
        return path
    return None

def git_value(args):
    try:
        return subprocess.check_output(
            ["git", "-C", client_root, *args],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return None

def git_dirty():
    status = git_value(["status", "--porcelain"])
    if status is None:
        return None
    return bool(status)

def client_git_metadata():
    head = git_value(["rev-parse", "HEAD"])
    if head is None:
        return None
    return {
        "head": head,
        "describe": git_value(["describe", "--tags", "--always", "--dirty"]),
        "dirty": git_dirty(),
    }

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
    tmp_root = require_artifact(artifacts.get("tmp_root"))
    config_artifacts = [
        require_artifact(os.path.join(tmp_root, "mesh-reticulum", "config")),
        require_artifact(os.path.join(tmp_root, "mesh-config.json")),
    ]
else:
    external_hash = require_text("external_client_hash")
    reticulumd_log = require_artifact(report.get("reticulumd_log"))
    external_log_key = f"{client}_log"
    external_log = require_artifact(report.get(external_log_key))
    for key in ("daemon_to_external_content", "external_to_daemon_content"):
        if not isinstance(report.get(key), str) or not report[key]:
            raise SystemExit(f"{client} report missing {key}")
    tmp_root = require_artifact(report.get("tmp_root"))
    if client == "sideband":
        config_artifacts = [
            require_artifact(os.path.join(tmp_root, "sideband-reticulum", "config")),
            require_artifact(os.path.join(tmp_root, "control", "state.json")),
        ]
    else:
        config_artifacts = [
            require_artifact(os.path.join(tmp_root, "control", "state.json")),
            optional_artifact(os.path.join(tmp_root, "columba")),
        ]
        config_artifacts = [path for path in config_artifacts if path is not None]

summary = {
    "status": "pass",
    "client": client,
    "report_path": report_path,
    "external_client": {
        "root": client_root,
        "git": client_git_metadata(),
    },
    "daemon_hash": require_text("daemon_hash"),
    "external_client_hash": external_hash,
    "artifacts": {
        "tmp_root": tmp_root,
        "reticulumd_log": reticulumd_log,
        "external_client_log": external_log,
        "client_config": config_artifacts,
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
