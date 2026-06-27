#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

case "$(uname -s)" in
  Linux|Android) ;;
  *)
    echo "[local-interface-unix-smoke] ERROR: abstract Unix shared instances require Linux/Android" >&2
    exit 1
    ;;
esac

TIMEOUT_SECS="${TIMEOUT_SECS:-30}"
LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/local-interface-unix-smoke}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
mkdir -p "$LOG_DIR"

# Boundary phrase preserved for contract tests: not multi-process Python shared-instance interop evidence.
RUN_DIR="$(mktemp -d "${LOG_DIR}/run.XXXXXX")"
CONFIG_PATH="${RUN_DIR}/reticulumd-local-interface-unix.toml"
DB_PATH="${RUN_DIR}/reticulum.db"
RPC_UNIX="${RUN_DIR}/rpc.sock"
RETICULUMD_LOG="${RUN_DIR}/reticulumd.log"
RNSTATUS_JSON="${RUN_DIR}/rnstatus.json"
RNSTATUS_HUMAN="${RUN_DIR}/rnstatus.txt"
FAKE_LOG="${RUN_DIR}/fake-unix-shared-instance.log"
FAKE_STATE="${RUN_DIR}/fake-unix-shared-instance-state.json"
FILESYSTEM_SOCKET="${RUN_DIR}/local-filesystem.sock"
ABSTRACT_LISTENER_INSTANCE="codex-listener-$$"
ABSTRACT_ATTACH_INSTANCE="codex-attach-$$"

: >"$RETICULUMD_LOG"
: >"$FAKE_LOG"

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
  python3 - <<'PY' "$REPORT_PATH" "$status" "$reason" "$RPC_ADDR" "$RUN_DIR" "$CONFIG_PATH" "$RETICULUMD_LOG" "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_LOG" "$FAKE_STATE" "$FILESYSTEM_SOCKET" "$ABSTRACT_LISTENER_INSTANCE" "$ABSTRACT_ATTACH_INSTANCE"
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
    fake_log,
    fake_state,
    filesystem_socket,
    abstract_listener_instance,
    abstract_attach_instance,
) = sys.argv[1:15]
report = {
    "status": status,
    "evidence_scope": "software_unix_shared_instance_local",
    "product_boundary": (
        "This proves filesystem Unix listener startup, Linux abstract Unix "
        "listener startup, and Linux abstract Unix client attach against a "
        "local fake shared instance; it is not multi-process Python "
        "shared-instance interop evidence."
    ),
    "reason": reason or None,
    "rpc_addr": rpc_addr,
    "run_dir": run_dir,
    "config_path": config_path,
    "reticulumd_log": reticulumd_log,
    "rnstatus_json": rnstatus_json,
    "rnstatus_human": rnstatus_human,
    "fake_log": fake_log,
    "fake_state": fake_state,
    "filesystem_socket": filesystem_socket,
    "abstract_listener_socket_path": f"@rns/{abstract_listener_instance}",
    "abstract_attach_socket_path": f"@rns/{abstract_attach_instance}",
}
json_path = pathlib.Path(rnstatus_json)
if json_path.exists():
    try:
        payload = json.loads(json_path.read_text(encoding="utf-8"))
        rows = {}
        for expected in [
            "local-unix-filesystem-listener",
            "local-unix-abstract-listener",
            "local-unix-abstract-attach",
        ]:
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
                    "shared_instance_type": (row.get("settings") or {}).get("shared_instance_type"),
                    "startup_status": runtime.get("startup_status"),
                    "runtime_iface": runtime.get("iface") or runtime.get("runtime_iface"),
                }
        report["interfaces"] = rows
    except Exception as exc:
        report["status_parse_error"] = str(exc)
state_path = pathlib.Path(fake_state)
if state_path.exists():
    try:
        report["fake_shared_instance"] = json.loads(state_path.read_text(encoding="utf-8"))
    except Exception as exc:
        report["fake_state_parse_error"] = str(exc)
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
  if [[ -n "${FAKE_PID:-}" ]]; then
    kill "$FAKE_PID" >/dev/null 2>&1 || true
    wait "$FAKE_PID" >/dev/null 2>&1 || true
  fi
  rm -f "$FILESYSTEM_SOCKET"
  if [[ $status -ne 0 ]]; then
    echo "[local-interface-unix-smoke] failed; logs=${RUN_DIR}" >&2
  fi
}
trap cleanup EXIT

fail() {
  local msg="$1"
  echo "[local-interface-unix-smoke] ERROR: ${msg}" | tee -a "$RETICULUMD_LOG" >&2
  write_report "fail" "$msg"
  exit 1
}

python3 - "$FAKE_LOG" "$FAKE_STATE" "$ABSTRACT_ATTACH_INSTANCE" <<'PY' &
import json
import pathlib
import select
import socket
import sys
import time

log_path, state_path, instance_name = sys.argv[1:4]
abstract_name = f"\0rns/{instance_name}"

state = {
    "socket_path": f"@rns/{instance_name}",
    "accepted_connections": 0,
    "closed_connections": 0,
    "bytes_rx": 0,
    "listening": False,
}


def save_state():
    pathlib.Path(state_path).write_text(
        json.dumps(state, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def log(message):
    with open(log_path, "a", encoding="utf-8") as handle:
        handle.write(message + "\n")
        handle.flush()


listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
listener.bind(abstract_name)
listener.listen()
listener.setblocking(False)
state["listening"] = True
save_state()
log(f"fake abstract LocalInterface listener on @rns/{instance_name}")

clients = []
deadline = time.monotonic() + 300
while time.monotonic() < deadline:
    readable, _, _ = select.select([listener, *clients], [], [], 0.1)
    for sock in readable:
        if sock is listener:
            conn, _ = listener.accept()
            conn.setblocking(False)
            clients.append(conn)
            state["accepted_connections"] += 1
            save_state()
            log(f"accepted abstract connection {state['accepted_connections']}")
            continue
        try:
            chunk = sock.recv(4096)
        except BlockingIOError:
            continue
        except OSError as exc:
            log(f"read error: {exc}")
            chunk = b""
        if chunk:
            state["bytes_rx"] += len(chunk)
            save_state()
            continue
        clients.remove(sock)
        try:
            sock.close()
        except OSError:
            pass
        state["closed_connections"] += 1
        save_state()

log("fake abstract LocalInterface listener exiting")
save_state()
PY
FAKE_PID=$!

deadline=$((SECONDS + TIMEOUT_SECS))
while true; do
  if [[ -s "$FAKE_STATE" ]] && python3 - <<'PY' "$FAKE_STATE"
import json
import sys
state = json.load(open(sys.argv[1], "r", encoding="utf-8"))
if state.get("listening") is True:
    raise SystemExit(0)
raise SystemExit(1)
PY
  then
    break
  fi
  if (( SECONDS >= deadline )); then
    fail "timed out waiting for fake abstract LocalInterface listener"
  fi
  if ! kill -0 "$FAKE_PID" >/dev/null 2>&1; then
    fail "fake abstract LocalInterface listener exited before publishing state"
  fi
  sleep 0.1
done

cat >"$CONFIG_PATH" <<EOF
[[interfaces]]
type = "LocalInterface"
enabled = true
name = "local-unix-filesystem-listener"
shared_instance_type = "unix"
socket_path = "${FILESYSTEM_SOCKET}"
fixed_mtu = 262144
force_shared_instance_bitrate = 1000000

[[interfaces]]
type = "LocalInterface"
enabled = true
name = "local-unix-abstract-listener"
shared_instance_type = "unix"
instance_name = "${ABSTRACT_LISTENER_INSTANCE}"
fixed_mtu = 262144
force_shared_instance_bitrate = 1000000

[[interfaces]]
type = "LocalClientInterface"
enabled = true
name = "local-unix-abstract-attach"
shared_instance_type = "unix"
socket_path = "@rns/${ABSTRACT_ATTACH_INSTANCE}"
fixed_mtu = 262144
force_shared_instance_bitrate = 1000000
EOF

cargo build -p reticulumd --bin reticulumd --quiet
cargo build -p rns-tools --bin rnstatus-rs --quiet

"${ROOT_DIR}/target/debug/reticulumd" \
  --rpc "$RPC_ADDR" \
  --rpc-unix "$RPC_UNIX" \
  --db "$DB_PATH" \
  --config "$CONFIG_PATH" \
  --strict-interface-startup >"$RETICULUMD_LOG" 2>&1 &
RET_PID=$!

while (( SECONDS < deadline )); do
  if ! kill -0 "$RET_PID" >/dev/null 2>&1; then
    fail "reticulumd exited before LocalInterface Unix status became healthy"
  fi
  if "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" --json >"$RNSTATUS_JSON" 2>>"$RETICULUMD_LOG" \
    && "${ROOT_DIR}/target/debug/rnstatus-rs" --rpc "$RPC_ADDR" >"$RNSTATUS_HUMAN" 2>>"$RETICULUMD_LOG"; then
    if python3 - <<'PY' "$RNSTATUS_JSON" "$RNSTATUS_HUMAN" "$FAKE_STATE" "$FILESYSTEM_SOCKET" "$ABSTRACT_LISTENER_INSTANCE" "$ABSTRACT_ATTACH_INSTANCE"
import json
import socket
import sys

json_path, human_path, fake_state_path, filesystem_socket, listener_instance, attach_instance = sys.argv[1:7]
payload = json.load(open(json_path, "r", encoding="utf-8"))
human = open(human_path, "r", encoding="utf-8", errors="replace").read()
fake_state = json.load(open(fake_state_path, "r", encoding="utf-8"))

interfaces = payload.get("interfaces", [])
expected = {
    "local-unix-filesystem-listener": ("local", "active", filesystem_socket),
    "local-unix-abstract-listener": ("local", "active", f"@rns/{listener_instance}"),
    "local-unix-abstract-attach": ("local_client", "attached", f"@rns/{attach_instance}"),
}
for name, (kind, startup_status, socket_path) in expected.items():
    row = next(
        (
            item
            for item in interfaces
            if item.get("type") == kind and item.get("name") == name
        ),
        None,
    )
    if row is None:
        raise SystemExit(1)
    if row.get("enabled") is not True:
        raise SystemExit(1)
    settings = row.get("settings") or {}
    if settings.get("shared_instance_type") != "unix":
        raise SystemExit(1)
    if settings.get("socket_path") != socket_path:
        raise SystemExit(1)
    if settings.get("mtu") != 262144:
        raise SystemExit(1)
    if settings.get("bitrate") != 1000000:
        raise SystemExit(1)
    runtime = settings.get("_runtime") or {}
    if runtime.get("startup_status") != startup_status:
        raise SystemExit(1)
    runtime_iface = runtime.get("iface") or runtime.get("runtime_iface")
    if not isinstance(runtime_iface, str) or not runtime_iface:
        raise SystemExit(1)

if (fake_state.get("accepted_connections") or 0) < 2:
    raise SystemExit(1)

with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
    sock.settimeout(1.0)
    sock.connect(filesystem_socket)

with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
    sock.settimeout(1.0)
    sock.connect(f"\0rns/{listener_instance}")

for token in [
    "local-unix-filesystem-listener",
    "local-unix-abstract-listener",
    "local-unix-abstract-attach",
    " local ",
    " local_client ",
    " active",
    " attached",
]:
    if token not in human:
        raise SystemExit(1)
PY
    then
      write_report "pass"
      echo "[local-interface-unix-smoke] pass"
      echo "[local-interface-unix-smoke] report=${REPORT_PATH}"
      echo "[local-interface-unix-smoke] logs=${RUN_DIR}"
      exit 0
    fi
  fi
  sleep 1
done

fail "timed out waiting for healthy LocalInterface Unix runtime status"
