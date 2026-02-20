#!/usr/bin/env bash
set -euo pipefail

# Soak + chaos-lite campaign for rnx E2E workflows.
# This script does not stop on first failure. It records pass/fail
# rounds and enforces a configurable failure budget.

MANIFEST_PATH="${MANIFEST_PATH:-crates/apps/rns-tools/Cargo.toml}"
CYCLES="${CYCLES:-3}"
BURST_ROUNDS="${BURST_ROUNDS:-10}"
TIMEOUT_SECS="${TIMEOUT_SECS:-20}"
PAUSE_SECS="${PAUSE_SECS:-1}"
CHAOS_INTERVAL="${CHAOS_INTERVAL:-5}"
CHAOS_NODES="${CHAOS_NODES:-5}"
CHAOS_TIMEOUT_SECS="${CHAOS_TIMEOUT_SECS:-75}"
MAX_FAILURES="${MAX_FAILURES:-0}"
REPORT_PATH="${REPORT_PATH:-target/soak/soak-report.json}"

if [[ ! -f "${MANIFEST_PATH}" ]]; then
  echo "MANIFEST_PATH not found: ${MANIFEST_PATH}" >&2
  exit 1
fi

mkdir -p "$(dirname "${REPORT_PATH}")"

total_rounds=$(( CYCLES * BURST_ROUNDS ))
round=0
failures=0
e2e_failures=0
mesh_runs=0
mesh_failures=0
started_epoch="$(date +%s)"

run_e2e_round() {
  local output
  local status=0
  if output="$(
    cargo run --quiet --manifest-path "${MANIFEST_PATH}" --bin rnx -- \
      e2e --timeout-secs "${TIMEOUT_SECS}" 2>&1
  )"; then
    :
  else
    status=1
  fi
  echo "${output}"
  if [[ "${status}" -eq 0 ]]; then
    grep -q "E2E ok: peer discovery A<->B succeeded" <<<"${output}" || status=1
    grep -q "E2E ok: compatibility delivery modes completed" <<<"${output}" || status=1
  fi
  return "${status}"
}

run_mesh_chaos_round() {
  local output
  local status=0
  if output="$(
    cargo run --quiet --manifest-path "${MANIFEST_PATH}" --bin rnx -- \
      mesh-sim --nodes "${CHAOS_NODES}" --timeout-secs "${CHAOS_TIMEOUT_SECS}" 2>&1
  )"; then
    :
  else
    status=1
  fi
  echo "${output}"
  if [[ "${status}" -eq 0 ]]; then
    grep -q "MESH ok: nodes=.* announce propagation established across mesh" <<<"${output}" \
      || status=1
    grep -q "MESH ok: multi-hop delivery workflows completed" <<<"${output}" || status=1
  fi
  return "${status}"
}

for cycle in $(seq 1 "${CYCLES}"); do
  echo "== soak cycle ${cycle}/${CYCLES} =="
  for burst in $(seq 1 "${BURST_ROUNDS}"); do
    round=$(( round + 1 ))
    echo "-- round ${round}/${total_rounds} (burst ${burst}/${BURST_ROUNDS})"
    if ! run_e2e_round; then
      e2e_failures=$(( e2e_failures + 1 ))
      failures=$(( failures + 1 ))
      echo "SOAK_RNX_FAILURE round=${round} kind=e2e"
    fi

    if [[ "${CHAOS_INTERVAL}" -gt 0 ]] && (( round % CHAOS_INTERVAL == 0 )); then
      mesh_runs=$(( mesh_runs + 1 ))
      echo "-- chaos round (mesh) ${mesh_runs} at soak round ${round}"
      if ! run_mesh_chaos_round; then
        mesh_failures=$(( mesh_failures + 1 ))
        failures=$(( failures + 1 ))
        echo "SOAK_RNX_FAILURE round=${round} kind=mesh-chaos"
      fi
    fi
  done
  sleep "${PAUSE_SECS}"
done

ended_epoch="$(date +%s)"
duration_secs=$(( ended_epoch - started_epoch ))
status="pass"
if [[ "${failures}" -gt "${MAX_FAILURES}" ]]; then
  status="fail"
fi

cat > "${REPORT_PATH}" <<EOF
{
  "status": "${status}",
  "cycles": ${CYCLES},
  "burst_rounds": ${BURST_ROUNDS},
  "total_rounds": ${total_rounds},
  "total_failures": ${failures},
  "e2e_failures": ${e2e_failures},
  "mesh_runs": ${mesh_runs},
  "mesh_failures": ${mesh_failures},
  "chaos_interval": ${CHAOS_INTERVAL},
  "max_failures": ${MAX_FAILURES},
  "duration_secs": ${duration_secs},
  "timestamp_utc": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
}
EOF

cat "${REPORT_PATH}"

if [[ "${status}" != "pass" ]]; then
  echo "SOAK_RNX_FAIL failures=${failures} max_failures=${MAX_FAILURES}" >&2
  exit 1
fi

echo "SOAK_RNX_SUCCESS failures=${failures} max_failures=${MAX_FAILURES}"
