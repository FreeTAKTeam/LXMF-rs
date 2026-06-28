#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

case "$(uname -s)" in
  Linux|Android) ;;
  *)
    echo "[local-interface-python-shared-smoke] ERROR: Python abstract Unix shared instance evidence requires Linux/Android" >&2
    exit 1
    ;;
esac

RETICULUM_PY_REPO="${RETICULUM_PY_REPO:-${ROOT_DIR}/.tmp/python-refs/Reticulum}"
PYTHON_BIN="${LXMF_PYTHON_BIN:-${PYTHON_BIN:-python3}}"
TIMEOUT_SECS="${TIMEOUT_SECS:-45}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/local-interface-python-shared-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

# Boundary phrase preserved for contract tests: does not prove broad application-level shared-instance traffic parity.
RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
RUST_CONFIG_PATH="${RUN_DIR}/reticulumd-local-python-shared.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
PY_TCP_CONFIG_DIR="${RUN_DIR}/python-tcp"
PY_UNIX_CONFIG_DIR="${RUN_DIR}/python-unix"
PY_TCP_TRAFFIC_CONFIG_DIR="${RUN_DIR}/python-tcp-traffic"
PY_UNIX_TRAFFIC_CONFIG_DIR="${RUN_DIR}/python-unix-traffic"
PY_TCP_LOG="${RUN_DIR}/python-shared-tcp.log"
PY_UNIX_LOG="${RUN_DIR}/python-shared-unix.log"
PY_TCP_TRAFFIC_LOG="${RUN_DIR}/python-traffic-tcp.log"
PY_UNIX_TRAFFIC_LOG="${RUN_DIR}/python-traffic-unix.log"
PY_TCP_STATE="${RUN_DIR}/python-shared-tcp-state.json"
PY_UNIX_STATE="${RUN_DIR}/python-shared-unix-state.json"
PY_TCP_TRAFFIC_STATE="${RUN_DIR}/python-traffic-tcp-state.json"
PY_UNIX_TRAFFIC_STATE="${RUN_DIR}/python-traffic-unix-state.json"
PY_TCP_PORT_FILE="${RUN_DIR}/python-shared-tcp.port"
PY_UNIX_INSTANCE="codex-py-shared-$$"
mkdir -p "$PY_TCP_CONFIG_DIR" "$PY_UNIX_CONFIG_DIR" "$PY_TCP_TRAFFIC_CONFIG_DIR" "$PY_UNIX_TRAFFIC_CONFIG_DIR"
: >"$RETICULUMD_LOG"
: >"$PY_TCP_LOG"
: >"$PY_UNIX_LOG"
: >"$PY_TCP_TRAFFIC_LOG"
: >"$PY_UNIX_TRAFFIC_LOG"

if [[ ! -f "${RETICULUM_PY_REPO}/RNS/Reticulum.py" ]]; then
  echo "[local-interface-python-shared-smoke] ERROR: RETICULUM_PY_REPO does not point to a Reticulum checkout: ${RETICULUM_PY_REPO}" >&2
  exit 1
fi

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

PY_TCP_PORT="$(
  python3 - <<'PY'
import socket
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
)"
printf '%s\n' "$PY_TCP_PORT" >"$PY_TCP_PORT_FILE"

cat >"${PY_TCP_CONFIG_DIR}/config" <<EOF
[reticulum]
share_instance = yes
shared_instance_type = tcp
shared_instance_port = ${PY_TCP_PORT}
force_shared_instance_bitrate = 1000000
enable_transport = no

[logging]
loglevel = 7
EOF

cat >"${PY_UNIX_CONFIG_DIR}/config" <<EOF
[reticulum]
share_instance = yes
shared_instance_type = unix
instance_name = ${PY_UNIX_INSTANCE}
force_shared_instance_bitrate = 1000000
enable_transport = no

[logging]
loglevel = 7
EOF

cat >"${PY_TCP_TRAFFIC_CONFIG_DIR}/config" <<EOF
[reticulum]
share_instance = yes
shared_instance_type = tcp
shared_instance_port = ${PY_TCP_PORT}
force_shared_instance_bitrate = 1000000
enable_transport = no

[logging]
loglevel = 7
EOF

cat >"${PY_UNIX_TRAFFIC_CONFIG_DIR}/config" <<EOF
[reticulum]
share_instance = yes
shared_instance_type = unix
instance_name = ${PY_UNIX_INSTANCE}
force_shared_instance_bitrate = 1000000
enable_transport = no

[logging]
loglevel = 7
EOF

write_report() {
  local status="$1"
  local reason="${2:-}"
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RPC_ADDR" "$RUN_DIR" "$RUST_CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$RETICULUM_PY_REPO" "$PY_TCP_CONFIG_DIR" "$PY_UNIX_CONFIG_DIR" "$PY_TCP_TRAFFIC_CONFIG_DIR" "$PY_UNIX_TRAFFIC_CONFIG_DIR" "$PY_TCP_LOG" "$PY_UNIX_LOG" "$PY_TCP_TRAFFIC_LOG" "$PY_UNIX_TRAFFIC_LOG" "$PY_TCP_STATE" "$PY_UNIX_STATE" "$PY_TCP_TRAFFIC_STATE" "$PY_UNIX_TRAFFIC_STATE" "$PY_TCP_PORT" "$PY_UNIX_INSTANCE"
import json
import pathlib
import subprocess
import sys

(
    report_path,
    status,
    reason,
    rpc_addr,
    run_dir,
    rust_config_path,
    reticulumd_log,
    rnstatus_json,
    rnstatus_human,
    reticulum_py_repo,
    py_tcp_config_dir,
    py_unix_config_dir,
    py_tcp_traffic_config_dir,
    py_unix_traffic_config_dir,
    py_tcp_log,
    py_unix_log,
    py_tcp_traffic_log,
    py_unix_traffic_log,
    py_tcp_state,
    py_unix_state,
    py_tcp_traffic_state,
    py_unix_traffic_state,
    py_tcp_port,
    py_unix_instance,
) = sys.argv[1:25]

def read_json(path):
    value_path = pathlib.Path(path)
    if not value_path.exists():
        return None
    try:
        return json.loads(value_path.read_text(encoding="utf-8"))
    except Exception as exc:
        return {"parse_error": str(exc)}

def read_interfaces(path):
    payload = read_json(path)
    rows = {}
    if not isinstance(payload, dict):
        return rows
    for expected in ["local-python-tcp-attach", "local-python-unix-attach"]:
        row = next(
            (
                item
                for item in payload.get("interfaces", [])
                if item.get("name") == expected
            ),
            None,
        )
        if row:
            runtime = (row.get("settings") or {}).get("_runtime") or {}
            rows[expected] = {
                "type": row.get("type"),
                "enabled": row.get("enabled"),
                "socket_path": (row.get("settings") or {}).get("socket_path"),
                "host": row.get("host"),
                "port": row.get("port"),
                "shared_instance_type": (row.get("settings") or {}).get("shared_instance_type"),
                "startup_status": runtime.get("startup_status"),
                "runtime_iface": runtime.get("iface") or runtime.get("runtime_iface"),
            }
    return rows

try:
    py_revision = subprocess.check_output(
        ["git", "-C", reticulum_py_repo, "rev-parse", "HEAD"],
        text=True,
        stderr=subprocess.DEVNULL,
    ).strip()
except Exception:
    py_revision = None

report = {
    "status": status,
    "evidence_scope": "python_shared_instance_tcp_unix_attach_and_announce_forward",
    "product_boundary": (
        "This proves reticulumd LocalClientInterface attaches to real pinned "
        "Python Reticulum shared instances over TCP and Linux abstract Unix "
        "sockets, and that Python-origin announces move across the shared "
        "instance fanout toward attached local clients; it does not prove broad "
        "application-level shared-instance traffic parity."
    ),
    "reason": reason or None,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "rust_config_path": rust_config_path,
    "reticulumd_log": reticulumd_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
    "reticulum_py_repo": reticulum_py_repo,
    "python_rns_revision": py_revision,
    "python_tcp_config_dir": py_tcp_config_dir,
    "python_unix_config_dir": py_unix_config_dir,
    "python_tcp_traffic_config_dir": py_tcp_traffic_config_dir,
    "python_unix_traffic_config_dir": py_unix_traffic_config_dir,
    "python_tcp_log": py_tcp_log,
    "python_unix_log": py_unix_log,
    "python_tcp_traffic_log": py_tcp_traffic_log,
    "python_unix_traffic_log": py_unix_traffic_log,
    "python_tcp_state": read_json(py_tcp_state),
    "python_unix_state": read_json(py_unix_state),
    "python_tcp_traffic_state": read_json(py_tcp_traffic_state),
    "python_unix_traffic_state": read_json(py_unix_traffic_state),
    "python_tcp_port": int(py_tcp_port),
    "python_unix_socket_path": f"@rns/{py_unix_instance}",
    "interfaces": read_interfaces(rnstatus_json),
}
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
  if [[ -n "${PY_TCP_PID:-}" ]]; then
    kill "$PY_TCP_PID" >/dev/null 2>&1 || true
    wait "$PY_TCP_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${PY_UNIX_PID:-}" ]]; then
    kill "$PY_UNIX_PID" >/dev/null 2>&1 || true
    wait "$PY_UNIX_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${PY_TCP_TRAFFIC_PID:-}" ]]; then
    kill "$PY_TCP_TRAFFIC_PID" >/dev/null 2>&1 || true
    wait "$PY_TCP_TRAFFIC_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${PY_UNIX_TRAFFIC_PID:-}" ]]; then
    kill "$PY_UNIX_TRAFFIC_PID" >/dev/null 2>&1 || true
    wait "$PY_UNIX_TRAFFIC_PID" >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[local-interface-python-shared-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[local-interface-python-shared-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

start_python_shared() {
  local config_dir="$1"
  local state_path="$2"
  local log_path="$3"
  PYTHONPATH="$RETICULUM_PY_REPO${PYTHONPATH:+:${PYTHONPATH}}" "$PYTHON_BIN" - "$config_dir" "$state_path" >"$log_path" 2>&1 <<'PY' &
import json
import pathlib
import sys
import time

config_dir, state_path = sys.argv[1:3]

def write_state(payload):
    pathlib.Path(state_path).write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

try:
    import RNS
    reticulum = RNS.Reticulum(configdir=config_dir, loglevel=7)
    while True:
        local_clients = getattr(RNS.Transport, "local_client_interfaces", [])
        local_client_stats = []
        for item in local_clients:
            local_client_stats.append(
                {
                    "name": str(item),
                    "online": getattr(item, "online", None),
                    "rxb": getattr(item, "rxb", None),
                    "txb": getattr(item, "txb", None),
                    "is_connected_to_shared_instance": getattr(item, "is_connected_to_shared_instance", None),
                }
            )
        shared = reticulum.shared_instance_interface
        write_state(
            {
                "ready": True,
                "is_shared_instance": reticulum.is_shared_instance,
                "is_connected_to_shared_instance": reticulum.is_connected_to_shared_instance,
                "shared_instance_type": reticulum.shared_instance_type,
                "use_af_unix": reticulum.use_af_unix,
                "local_interface_port": reticulum.local_interface_port,
                "local_socket_path": reticulum.local_socket_path,
                "shared_interface": str(shared) if shared is not None else None,
                "local_client_count": len(local_clients),
                "local_clients": [str(item) for item in local_clients],
                "local_client_stats": local_client_stats,
                "local_client_rxb_total": sum((getattr(item, "rxb", 0) or 0) for item in local_clients),
                "local_client_txb_total": sum((getattr(item, "txb", 0) or 0) for item in local_clients),
            }
        )
        time.sleep(0.25)
except BaseException as exc:
    write_state({"ready": False, "error": repr(exc)})
    raise
PY
  echo $!
}

start_python_traffic() {
  local config_dir="$1"
  local state_path="$2"
  local log_path="$3"
  local label="$4"
  PYTHONPATH="$RETICULUM_PY_REPO${PYTHONPATH:+:${PYTHONPATH}}" "$PYTHON_BIN" - "$config_dir" "$state_path" "$label" >"$log_path" 2>&1 <<'PY' &
import json
import pathlib
import sys
import time

config_dir, state_path, label = sys.argv[1:4]

def write_state(payload):
    pathlib.Path(state_path).write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

try:
    import RNS
    reticulum = RNS.Reticulum(configdir=config_dir, loglevel=7, require_shared_instance=True)
    identity = RNS.Identity()
    destination = RNS.Destination(
        identity,
        RNS.Destination.IN,
        RNS.Destination.SINGLE,
        "codex",
        "local",
        "shared",
        "traffic",
    )
    app_data = f"codex-local-python-shared-traffic:{label}".encode("utf-8")
    announced = 0
    for _ in range(3):
        destination.announce(app_data=app_data)
        announced += 1
        write_state(
            {
                "ready": True,
                "label": label,
                "is_connected_to_shared_instance": reticulum.is_connected_to_shared_instance,
                "announced_count": announced,
                "destination_hash": RNS.hexrep(destination.hash, delimit=False),
            }
        )
        time.sleep(0.5)
    while True:
        write_state(
            {
                "ready": True,
                "label": label,
                "is_connected_to_shared_instance": reticulum.is_connected_to_shared_instance,
                "announced_count": announced,
                "destination_hash": RNS.hexrep(destination.hash, delimit=False),
            }
        )
        time.sleep(0.5)
except BaseException as exc:
    write_state({"ready": False, "label": label, "error": repr(exc)})
    raise
PY
  echo $!
}

PY_TCP_PID="$(start_python_shared "$PY_TCP_CONFIG_DIR" "$PY_TCP_STATE" "$PY_TCP_LOG")"
PY_UNIX_PID="$(start_python_shared "$PY_UNIX_CONFIG_DIR" "$PY_UNIX_STATE" "$PY_UNIX_LOG")"

deadline=$((SECONDS + TIMEOUT_SECS))
wait_for_python_ready() {
  local state_path="$1"
  local pid="$2"
  local label="$3"
  while (( SECONDS < deadline )); do
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      fail "Python ${label} shared instance exited before ready"
    fi
    if [[ -s "$state_path" ]] && python3 - <<'PY' "$state_path"
import json
import sys
state = json.load(open(sys.argv[1], "r", encoding="utf-8"))
if state.get("ready") is True and state.get("is_shared_instance") is True:
    raise SystemExit(0)
raise SystemExit(1)
PY
    then
      return 0
    fi
    sleep 0.2
  done
  fail "timed out waiting for Python ${label} shared instance"
}

wait_for_python_ready "$PY_TCP_STATE" "$PY_TCP_PID" "TCP"
wait_for_python_ready "$PY_UNIX_STATE" "$PY_UNIX_PID" "Unix"

cat >"$RUST_CONFIG_PATH" <<EOF
[[interfaces]]
type = "LocalClientInterface"
enabled = true
name = "local-python-tcp-attach"
shared_instance_type = "tcp"
host = "127.0.0.1"
port = ${PY_TCP_PORT}
fixed_mtu = 262144
force_shared_instance_bitrate = 1000000

[[interfaces]]
type = "LocalClientInterface"
enabled = true
name = "local-python-unix-attach"
shared_instance_type = "unix"
socket_path = "@rns/${PY_UNIX_INSTANCE}"
fixed_mtu = 262144
force_shared_instance_bitrate = 1000000
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet

"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$RPC_ADDR" \
  --rpc-unix "$RPC_UNIX" \
  --db "$DB_PATH" \
  --config "$RUST_CONFIG_PATH" \
  --strict-interface-startup >"$RETICULUMD_LOG" 2>&1 &
RET_PID=$!
TRAFFIC_STARTED=false

while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before Python shared-instance attach status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if [[ "$TRAFFIC_STARTED" == false ]] && python3 - <<'PY' "$RNSTATUS_JSON" "$PY_TCP_STATE" "$PY_UNIX_STATE"
import json
import sys

json_path, tcp_state_path, unix_state_path = sys.argv[1:4]
payload = json.load(open(json_path, "r", encoding="utf-8"))
states = [
    json.load(open(tcp_state_path, "r", encoding="utf-8")),
    json.load(open(unix_state_path, "r", encoding="utf-8")),
]
for state in states:
    if (state.get("local_client_count") or 0) < 1:
        raise SystemExit(1)
for name in ["local-python-tcp-attach", "local-python-unix-attach"]:
    row = next(
        (
            item
            for item in payload.get("interfaces", [])
            if item.get("type") == "local_client" and item.get("name") == name
        ),
        None,
    )
    if row is None:
        raise SystemExit(1)
    runtime = ((row.get("settings") or {}).get("_runtime") or {})
    if runtime.get("startup_status") != "attached":
        raise SystemExit(1)
PY
    then
      PY_TCP_TRAFFIC_PID="$(start_python_traffic "$PY_TCP_TRAFFIC_CONFIG_DIR" "$PY_TCP_TRAFFIC_STATE" "$PY_TCP_TRAFFIC_LOG" "tcp")"
      PY_UNIX_TRAFFIC_PID="$(start_python_traffic "$PY_UNIX_TRAFFIC_CONFIG_DIR" "$PY_UNIX_TRAFFIC_STATE" "$PY_UNIX_TRAFFIC_LOG" "unix")"
      TRAFFIC_STARTED=true
    fi

    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$PY_TCP_STATE" "$PY_UNIX_STATE" "$PY_TCP_TRAFFIC_STATE" "$PY_UNIX_TRAFFIC_STATE" "$PY_TCP_PORT" "$PY_UNIX_INSTANCE"
import json
import pathlib
import sys

(
    json_path,
    human_path,
    tcp_state_path,
    unix_state_path,
    tcp_traffic_state_path,
    unix_traffic_state_path,
    tcp_port_raw,
    unix_instance,
) = sys.argv[1:9]
for path in [
    json_path,
    human_path,
    tcp_state_path,
    unix_state_path,
    tcp_traffic_state_path,
    unix_traffic_state_path,
]:
    if not pathlib.Path(path).exists():
        raise SystemExit(1)
tcp_port = int(tcp_port_raw)
payload = json.load(open(json_path, "r", encoding="utf-8"))
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
tcp_state = json.load(open(tcp_state_path, "r", encoding="utf-8"))
unix_state = json.load(open(unix_state_path, "r", encoding="utf-8"))
tcp_traffic_state = json.load(open(tcp_traffic_state_path, "r", encoding="utf-8"))
unix_traffic_state = json.load(open(unix_traffic_state_path, "r", encoding="utf-8"))

interfaces = payload.get("interfaces", [])
expected = {
    "local-python-tcp-attach": ("tcp", "127.0.0.1", tcp_port, None),
    "local-python-unix-attach": ("unix", None, None, f"@rns/{unix_instance}"),
}
for name, (shared_type, host, port, socket_path) in expected.items():
    row = next(
        (
            item
            for item in interfaces
            if item.get("type") == "local_client" and item.get("name") == name
        ),
        None,
    )
    if row is None or row.get("enabled") is not True:
        raise SystemExit(1)
    settings = row.get("settings") or {}
    if settings.get("shared_instance_type") != shared_type:
        raise SystemExit(1)
    if settings.get("mtu") != 262144 or settings.get("bitrate") != 1000000:
        raise SystemExit(1)
    if host is not None and row.get("host") != host:
        raise SystemExit(1)
    if port is not None and row.get("port") != port:
        raise SystemExit(1)
    if socket_path is not None and settings.get("socket_path") != socket_path:
        raise SystemExit(1)
    runtime = settings.get("_runtime") or {}
    if runtime.get("startup_status") != "attached":
        raise SystemExit(1)
    runtime_iface = runtime.get("iface") or runtime.get("runtime_iface")
    if not isinstance(runtime_iface, str) or not runtime_iface:
        raise SystemExit(1)

for state in [tcp_state, unix_state]:
    if (state.get("local_client_count") or 0) < 2:
        raise SystemExit(1)
    if (state.get("local_client_rxb_total") or 0) <= 0:
        raise SystemExit(1)
    if (state.get("local_client_txb_total") or 0) <= 0:
        raise SystemExit(1)
for state in [tcp_traffic_state, unix_traffic_state]:
    if state.get("ready") is not True:
        raise SystemExit(1)
    if state.get("is_connected_to_shared_instance") is not True:
        raise SystemExit(1)
    if (state.get("announced_count") or 0) < 3:
        raise SystemExit(1)
for token in [
    "local-python-tcp-attach",
    "local-python-unix-attach",
    " local_client ",
    " attached",
]:
    if token not in human:
        raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[local-interface-python-shared-smoke] pass"
      echo "[local-interface-python-shared-smoke] report=${REPORT_PATH}"
      echo "[local-interface-python-shared-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for healthy Python shared-instance attach status"
