#!/usr/bin/env bash
set -euo pipefail

# Repeated Reticulum daemon E2E runs used as a soak gate.
# Each round starts fresh daemons, performs peer discovery,
# sends A->B and B->A messages, and requires delivery confirmations.

MANIFEST_PATH="${MANIFEST_PATH:-../Reticulum-rs/crates/reticulum/Cargo.toml}"
CYCLES="${CYCLES:-3}"
BURST_ROUNDS="${BURST_ROUNDS:-10}"
TIMEOUT_SECS="${TIMEOUT_SECS:-20}"
PAUSE_SECS="${PAUSE_SECS:-1}"

if [[ ! -f "${MANIFEST_PATH}" ]]; then
  echo "MANIFEST_PATH not found: ${MANIFEST_PATH}" >&2
  exit 1
fi

total_rounds=$(( CYCLES * BURST_ROUNDS ))
round=0

for cycle in $(seq 1 "${CYCLES}"); do
  echo "== soak cycle ${cycle}/${CYCLES} =="
  for burst in $(seq 1 "${BURST_ROUNDS}"); do
    round=$(( round + 1 ))
    echo "-- round ${round}/${total_rounds} (burst ${burst}/${BURST_ROUNDS})"
    output="$(
      cargo run --quiet --manifest-path "${MANIFEST_PATH}" --bin rnx -- \
        e2e --timeout-secs "${TIMEOUT_SECS}" 2>&1
    )"
    echo "${output}"

    grep -q "E2E ok: peer discovery A<->B succeeded" <<<"${output}"
    grep -q "E2E ok: message .* delivered A->B" <<<"${output}"
    grep -q "E2E ok: message .* delivered B->A" <<<"${output}"
  done
  sleep "${PAUSE_SECS}"
done

echo "SOAK_RNX_SUCCESS cycles=${CYCLES} burst_rounds=${BURST_ROUNDS} timeout_secs=${TIMEOUT_SECS}"
