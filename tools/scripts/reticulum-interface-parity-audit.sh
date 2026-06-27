#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

REQUIRE_FULL=false
for arg in "$@"; do
  case "$arg" in
    --require-full)
      REQUIRE_FULL=true
      ;;
    *)
      echo "[reticulum-interface-parity-audit] ERROR: unknown argument: $arg" >&2
      echo "usage: $0 [--require-full]" >&2
      exit 2
      ;;
  esac
done

LOG_DIR="${LOG_DIR:-${ROOT_DIR}/target/reticulum-interface-parity-audit}"
REPORT_PATH="${REPORT_PATH:-${LOG_DIR}/report.json}"
ARTIFACT_MANIFEST="${RNODE_HIL_ARTIFACT_MANIFEST:-${RIF_HIL_ARTIFACT_MANIFEST:-${RIF_ARTIFACT_MANIFEST:-}}}"
mkdir -p "$LOG_DIR"

python3 - <<'PY' "$REPORT_PATH" "$REQUIRE_FULL" "${RNODE_HIL_REPORTS:-}" "$ARTIFACT_MANIFEST"
import glob
import hashlib
import json
import pathlib
import sys

report_path, require_full, rnode_hil_reports, artifact_manifest_path = sys.argv[1:5]
root = pathlib.Path.cwd()


def display_path(path):
    value = pathlib.Path(path)
    if value.is_absolute() and root in value.parents:
        return str(value.relative_to(root))
    return str(value)


def resolve_path(path):
    value = pathlib.Path(path)
    if value.is_absolute():
        return value
    return root / value


def read_json(relative_path):
    path = root / relative_path
    if not path.exists():
        return None, f"missing {relative_path}"
    try:
        return json.loads(path.read_text(encoding="utf-8")), None
    except Exception as exc:
        return None, f"invalid json in {relative_path}: {exc}"


def passed_report(relative_path, expected_scope=None):
    payload, error = read_json(relative_path)
    if error:
        return {
            "id": relative_path,
            "status": "missing",
            "path": relative_path,
            "reason": error,
        }
    reasons = []
    if payload.get("status") != "pass":
        reasons.append(f"status is {payload.get('status')!r}")
    if expected_scope is not None and payload.get("evidence_scope") != expected_scope:
        reasons.append(f"evidence_scope is {payload.get('evidence_scope')!r}")
    return {
        "id": relative_path,
        "status": "pass" if not reasons else "fail",
        "path": relative_path,
        "evidence_scope": payload.get("evidence_scope"),
        "reason": "; ".join(reasons) if reasons else None,
    }


def get_nested(payload, path):
    current = payload
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def artifact_manifest_check():
    if not artifact_manifest_path:
        return {
            "status": "not_configured",
            "path": None,
            "report_paths": [],
            "reason": None,
        }

    path = resolve_path(artifact_manifest_path)
    reasons = []
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        return {
            "status": "fail",
            "path": display_path(path),
            "report_paths": [],
            "reason": f"invalid artifact manifest: {exc}",
        }

    if payload.get("schema") != "reticulum_interface_hil_matrix_artifacts.v1":
        reasons.append(f"schema is {payload.get('schema')!r}")

    artifacts = payload.get("artifacts")
    if not isinstance(artifacts, list):
        artifacts = []
        reasons.append("artifacts is not a list")

    required_roles = {
        "rnode_serial_report": "serial",
        "rnode_tcp_report": "tcp",
        "rnode_ble_report": "ble",
    }
    by_role = {item.get("role"): item for item in artifacts if isinstance(item, dict)}
    checks = []
    report_paths = []
    for role, bearer in required_roles.items():
        item = by_role.get(role)
        if not item:
            reasons.append(f"{role} missing from artifact manifest")
            checks.append({"role": role, "bearer": bearer, "status": "missing"})
            continue

        artifact_path = item.get("path")
        if not artifact_path:
            reasons.append(f"{role} path missing")
            checks.append({"role": role, "bearer": bearer, "status": "fail", "reason": "path missing"})
            continue

        resolved = resolve_path(artifact_path)
        report_paths.append(resolved)
        check = {
            "role": role,
            "bearer": bearer,
            "path": display_path(resolved),
        }
        if not resolved.exists():
            reasons.append(f"{role} path does not exist: {display_path(resolved)}")
            check.update({"status": "missing", "reason": "path does not exist"})
            checks.append(check)
            continue

        expected_sha = item.get("sha256")
        actual_sha = hashlib.sha256(resolved.read_bytes()).hexdigest()
        check["sha256"] = actual_sha
        check["expected_sha256"] = expected_sha
        if expected_sha != actual_sha:
            reasons.append(
                f"sha256 mismatch for {role}: expected {expected_sha!r}, got {actual_sha!r}"
            )
            check.update({"status": "fail", "reason": "sha256 mismatch"})
        else:
            check["status"] = "pass"
        checks.append(check)

    return {
        "status": "pass" if not reasons else "fail",
        "path": display_path(path),
        "schema": payload.get("schema"),
        "evidence_scope": payload.get("evidence_scope"),
        "report_paths": [display_path(path) for path in report_paths],
        "checks": checks,
        "reason": "; ".join(reasons) if reasons else None,
    }


def local_python_shared_check():
    relative_path = "target/local-interface-python-shared-smoke/report.json"
    payload, error = read_json(relative_path)
    if error:
        return {
            "id": "local_python_shared_instance",
            "status": "missing",
            "path": relative_path,
            "reason": error,
        }
    reasons = []
    if payload.get("status") != "pass":
        reasons.append(f"status is {payload.get('status')!r}")
    if payload.get("evidence_scope") != "python_shared_instance_tcp_unix_attach_and_announce_forward":
        reasons.append(f"evidence_scope is {payload.get('evidence_scope')!r}")
    if not payload.get("python_rns_revision"):
        reasons.append("python_rns_revision missing")
    interfaces = payload.get("interfaces") or {}
    for name in ["local-python-tcp-attach", "local-python-unix-attach"]:
        startup = get_nested(interfaces, [name, "startup_status"])
        if startup != "attached":
            reasons.append(f"{name} startup_status is {startup!r}")
    for state_name in ["python_tcp_state", "python_unix_state"]:
        state = payload.get(state_name) or {}
        if (state.get("local_client_count") or 0) < 2:
            reasons.append(f"{state_name}.local_client_count < 2")
        if (state.get("local_client_rxb_total") or 0) <= 0:
            reasons.append(f"{state_name}.local_client_rxb_total <= 0")
        if (state.get("local_client_txb_total") or 0) <= 0:
            reasons.append(f"{state_name}.local_client_txb_total <= 0")
    for state_name in ["python_tcp_traffic_state", "python_unix_traffic_state"]:
        state = payload.get(state_name) or {}
        if (state.get("announced_count") or 0) < 3:
            reasons.append(f"{state_name}.announced_count < 3")
    return {
        "id": "local_python_shared_instance",
        "status": "pass" if not reasons else "fail",
        "path": relative_path,
        "evidence_scope": payload.get("evidence_scope"),
        "python_rns_revision": payload.get("python_rns_revision"),
        "reason": "; ".join(reasons) if reasons else None,
    }


def rnode_fake_tcp_check():
    relative_path = "target/rnode-fake-tcp-smoke/report.json"
    payload, error = read_json(relative_path)
    if error:
        return {
            "id": "rnode_fake_tcp_prepared_path",
            "status": "missing",
            "path": relative_path,
            "reason": error,
        }
    reasons = []
    if payload.get("status") != "pass":
        reasons.append(f"status is {payload.get('status')!r}")
    if payload.get("evidence_scope") != "software_fake_tcp_rnode_prepared_host_management":
        reasons.append(f"evidence_scope is {payload.get('evidence_scope')!r}")
    fake_peer = payload.get("fake_peer") or {}
    if fake_peer.get("radio_query_seen") is not True:
        reasons.append("fake peer did not see CMD_RADIO_STATE ask")
    if fake_peer.get("management_blink_seen") is not True:
        reasons.append("fake peer did not see CMD_BLINK management frame")
    prepared = payload.get("prepared_host") or {}
    if prepared.get("status") != "pass":
        reasons.append("embedded prepared_host report did not pass")
    if prepared.get("evidence_scope") != "prepared_host_tcp_rnode":
        reasons.append("embedded prepared_host evidence_scope is not prepared_host_tcp_rnode")
    commands = prepared.get("management_commands") or []
    queued = {(item.get("expected_command"), item.get("queued")) for item in commands}
    if ("radio_state_query", True) not in queued:
        reasons.append("radio_state_query management command was not queued")
    if ("blink", True) not in queued:
        reasons.append("blink management command was not queued")
    post = prepared.get("post_management_status") or {}
    if post.get("online") is not True:
        reasons.append("post-management status is not online")
    if post.get("radio_state") != 1:
        reasons.append("post-management radio_state is not 1")
    if post.get("last_command_error") is not None:
        reasons.append("post-management last_command_error is not null")
    if post.get("hardware_errors") not in ([], None):
        reasons.append("post-management hardware_errors is not empty")
    return {
        "id": "rnode_fake_tcp_prepared_path",
        "status": "pass" if not reasons else "fail",
        "path": relative_path,
        "evidence_scope": payload.get("evidence_scope"),
        "reason": "; ".join(reasons) if reasons else None,
    }


def candidate_rnode_hil_paths(manifest_report_paths):
    paths = []
    for item in manifest_report_paths:
        paths.append(str(resolve_path(item)))
    for item in rnode_hil_reports.split(":"):
        item = item.strip()
        if item:
            paths.append(str(resolve_path(item)))
    paths.extend(glob.glob(str(root / "target/rnode-hil/report.json")))
    paths.extend(glob.glob(str(root / "target/rnode-hil/*/report.json")))
    paths.extend(glob.glob(str(root / "target/rnode-hil/**/*.report.json"), recursive=True))
    seen = set()
    result = []
    for path in paths:
        relative = pathlib.Path(path)
        if relative in seen:
            continue
        seen.add(relative)
        result.append(relative)
    return result


def rnode_hardware_matrix(artifact_manifest):
    required_scopes = {
        "prepared_host_serial_rnode": "serial",
        "prepared_host_tcp_rnode": "tcp",
        "prepared_host_ble_rnode": "ble",
    }
    found = {}
    inspected = []
    for path in candidate_rnode_hil_paths(artifact_manifest.get("report_paths") or []):
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except Exception as exc:
            inspected.append({"path": str(path), "status": "invalid", "reason": str(exc)})
            continue
        scope = payload.get("evidence_scope")
        inspected.append({"path": str(path), "status": payload.get("status"), "evidence_scope": scope})
        if scope not in required_scopes:
            continue
        expected_bearer = required_scopes[scope]
        reasons = []
        if payload.get("status") != "pass":
            reasons.append(f"status is {payload.get('status')!r}")
        if payload.get("report_schema") != "rnode_prepared_host_smoke.v1":
            reasons.append(f"report_schema is {payload.get('report_schema')!r}")
        if not isinstance(payload.get("captured_at_utc"), str) or not payload.get("captured_at_utc"):
            reasons.append("captured_at_utc missing")
        if not isinstance(payload.get("captured_by_host"), str) or not payload.get("captured_by_host"):
            reasons.append("captured_by_host missing")
        if payload.get("script") != "tools/scripts/rnode-prepared-host-smoke.sh":
            reasons.append(f"script is {payload.get('script')!r}")
        if payload.get("transport_kind") != expected_bearer:
            reasons.append(f"transport_kind is {payload.get('transport_kind')!r}")
        if payload.get("bearer") != expected_bearer:
            reasons.append(f"bearer is {payload.get('bearer')!r}")
        if not payload.get("endpoint"):
            reasons.append("endpoint missing")
        if payload.get("detected") is not True:
            reasons.append("detected is not true")
        firmware = payload.get("firmware_version") or {}
        if not isinstance(firmware.get("label"), str) or not firmware.get("label"):
            reasons.append("firmware_version.label missing")
        if payload.get("platform") is None:
            reasons.append("platform missing")
        if payload.get("mcu") is None:
            reasons.append("mcu missing")
        if payload.get("online") is not True:
            reasons.append("online is not true")
        if payload.get("radio_state") != 1:
            reasons.append("radio_state is not 1")
        if payload.get("last_command_error") is not None:
            reasons.append("last_command_error is not null")
        if payload.get("hardware_errors") not in ([], None):
            reasons.append("hardware_errors is not empty")
        commands = payload.get("management_commands") or []
        queued = {(item.get("expected_command"), item.get("queued")) for item in commands}
        if ("radio_state_query", True) not in queued:
            reasons.append("radio_state_query management command was not queued")
        if ("blink", True) not in queued:
            reasons.append("blink management command was not queued")
        post = payload.get("post_management_status") or {}
        if post.get("online") is not True:
            reasons.append("post-management online is not true")
        if post.get("radio_state") != 1:
            reasons.append("post-management radio_state is not 1")
        if post.get("last_command_error") is not None:
            reasons.append("post-management last_command_error is not null")
        if post.get("hardware_errors") not in ([], None):
            reasons.append("post-management hardware_errors is not empty")
        candidate = {
            "bearer": expected_bearer,
            "path": str(path.relative_to(root) if path.is_absolute() and root in path.parents else path),
            "status": "pass" if not reasons else "fail",
            "captured_at_utc": payload.get("captured_at_utc"),
            "captured_by_host": payload.get("captured_by_host"),
            "endpoint": payload.get("endpoint"),
            "firmware_version": payload.get("firmware_version"),
            "platform": payload.get("platform"),
            "mcu": payload.get("mcu"),
            "reason": "; ".join(reasons) if reasons else None,
        }
        existing = found.get(scope)
        if existing is None or (existing.get("status") != "pass" and candidate["status"] == "pass"):
            found[scope] = candidate
    checks = []
    for scope, bearer in required_scopes.items():
        checks.append(
            {
                "id": f"rnode_prepared_host_{bearer}",
                "required_evidence_scope": scope,
                **found.get(
                    scope,
                    {
                        "bearer": bearer,
                        "status": "missing",
                        "reason": "no prepared-host hardware report found",
                    },
                ),
            }
        )
    return {
        "status": "pass" if all(item["status"] == "pass" for item in checks) else "incomplete",
        "checks": checks,
        "inspected_reports": inspected,
        "report_search": [
            "target/rnode-hil/report.json",
            "target/rnode-hil/*/report.json",
            "target/rnode-hil/**/*.report.json",
            "RNODE_HIL_REPORTS colon-separated override",
            "RNODE_HIL_ARTIFACT_MANIFEST verified rnode_*_report artifacts",
        ],
    }


artifact_manifest = artifact_manifest_check()
local_checks = [
    passed_report("target/local-interface-smoke/report.json"),
    passed_report("target/local-interface-unix-smoke/report.json", "software_unix_shared_instance_local"),
    local_python_shared_check(),
]
rnode_software_checks = [
    passed_report("target/rnode-ble-software-smoke/report.json", "software_rnode_ble_fallback_management"),
    rnode_fake_tcp_check(),
]
rnode_hardware = rnode_hardware_matrix(artifact_manifest)

local_status = "pass" if all(item["status"] == "pass" for item in local_checks) else "incomplete"
rnode_software_status = "pass" if all(item["status"] == "pass" for item in rnode_software_checks) else "incomplete"
artifact_manifest_status = artifact_manifest["status"]
artifact_manifest_ok = artifact_manifest_status in ("not_configured", "pass")
full_status = (
    "pass"
    if local_status == "pass"
    and rnode_software_status == "pass"
    and rnode_hardware["status"] == "pass"
    and artifact_manifest_ok
    else "incomplete"
)

missing = []
for group_name, checks in [
    ("local_interface", local_checks),
    ("rnode_software", rnode_software_checks),
    ("rnode_hardware", rnode_hardware["checks"]),
]:
    for check in checks:
        if check["status"] != "pass":
            missing.append(
                {
                    "group": group_name,
                    "id": check.get("id"),
                    "status": check["status"],
                    "reason": check.get("reason"),
                }
            )
if not artifact_manifest_ok:
    missing.append(
        {
            "group": "rnode_hardware",
            "id": "artifact_manifest",
            "status": artifact_manifest_status,
            "reason": artifact_manifest.get("reason"),
        }
    )

report = {
    "status": full_status,
    "evidence_scope": "reticulum_interfaces_384_385_parity_audit",
    "product_boundary": (
        "This audit proves full #384/#385 interface parity only when LocalInterface "
        "software/Python-shared evidence and serial, TCP/Wi-Fi, and BLE prepared-host "
        "RNode hardware reports all pass. Software-only RNode evidence is recorded but "
        "does not replace the hardware matrix."
    ),
    "local_interface_384": {
        "status": local_status,
        "checks": local_checks,
    },
    "rnode_ble_385": {
        "software_status": rnode_software_status,
        "hardware_status": rnode_hardware["status"],
        "software_checks": rnode_software_checks,
        "hardware_matrix": rnode_hardware,
        "artifact_manifest": artifact_manifest,
    },
    "missing_full_parity": missing,
}

pathlib.Path(report_path).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"[reticulum-interface-parity-audit] status={full_status}")
print(f"[reticulum-interface-parity-audit] report={report_path}")
if missing:
    print("[reticulum-interface-parity-audit] missing:")
    for item in missing:
        print(f"  - {item['group']}::{item['id']} ({item['status']}): {item.get('reason')}")

if require_full == "true" and full_status != "pass":
    sys.exit(1)
PY
