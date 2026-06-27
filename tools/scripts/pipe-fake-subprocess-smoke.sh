#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/pipe-fake-subprocess-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-pipe.toml"
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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN"
import json
import pathlib
import sys

(
    report_path,
    status,
    reason,
    rpc_addr,
    run_dir,
    config_path,
    reticulumd_log,
    rnstatus_json,
    rnstatus_human,
) = sys.argv[1:10]
report = {
    "status": status,
    "reason": reason or None,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
    "reticulumd_log": reticulumd_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
}
json_path = pathlib.Path(rnstatus_json)
if json_path.exists():
    try:
        payload = json.loads(json_path.read_text(encoding="utf-8"))
        pipe = next(
            (
                row
                for row in payload.get("interfaces", [])
                if row.get("type") == "pipe" and row.get("name") == "pipe-fake-subprocess"
            ),
            None,
        )
        if pipe:
            runtime_root = (pipe.get("settings") or {}).get("_runtime") or {}
            pipe_runtime = runtime_root.get("pipe") or {}
            pipe_status = pipe_runtime.get("status") or {}
            report["startup_status"] = runtime_root.get("startup_status")
            report["runtime_iface"] = runtime_root.get("runtime_iface") or runtime_root.get("iface")
            report["process_state"] = pipe_status.get("process_state")
            report["pipe_is_open"] = pipe_status.get("pipe_is_open")
            report["respawn_attempts"] = pipe_status.get("respawn_attempts")
            report["last_error"] = pipe_status.get("last_error")
            report["command"] = pipe_status.get("command")
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
    echo "[pipe-fake-subprocess-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[pipe-fake-subprocess-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - <<'PY' "$CONFIG_PATH" || fail "failed to generate PipeInterface config"
import pathlib
import sys

config_path = sys.argv[1]
pathlib.Path(config_path).write_text(
    "\n".join(
        [
            "[[interfaces]]",
            'type = "PipeInterface"',
            "enabled = true",
            'name = "pipe-fake-subprocess"',
            'command = "cat"',
            "respawn_delay = 0.1",
            "configured_bitrate = 256000",
        ]
    )
    + "\n",
    encoding="utf-8",
)
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
    fail "reticulumd exited before PipeInterface status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN"
import json
import sys

json_path, human_path = sys.argv[1:3]
payload = json.load(open(json_path, "r", encoding="utf-8"))
pipe = next(
    (
        row
        for row in payload.get("interfaces", [])
        if row.get("type") == "pipe" and row.get("name") == "pipe-fake-subprocess"
    ),
    None,
)
if pipe is None:
    raise SystemExit(1)
runtime_root = (pipe.get("settings") or {}).get("_runtime") or {}
if runtime_root.get("startup_status") != "spawned":
    raise SystemExit(1)
runtime_iface = runtime_root.get("runtime_iface") or runtime_root.get("iface")
if not isinstance(runtime_iface, str) or not runtime_iface:
    raise SystemExit(1)
status = ((runtime_root.get("pipe") or {}).get("status") or {})
if status.get("command") != "cat":
    raise SystemExit(1)
if status.get("process_state") != "running":
    raise SystemExit(1)
if status.get("pipe_is_open") is not True:
    raise SystemExit(1)
if status.get("respawn_attempts") != 0:
    raise SystemExit(1)
if status.get("last_error") is not None:
    raise SystemExit(1)
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
if "pipe-fake-subprocess" not in human:
    raise SystemExit(1)
if "pipe state=running open=true respawns=0" not in human:
    raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[pipe-fake-subprocess-smoke] pass"
      echo "[pipe-fake-subprocess-smoke] report=${REPORT_PATH}"
      echo "[pipe-fake-subprocess-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for healthy PipeInterface runtime status"
