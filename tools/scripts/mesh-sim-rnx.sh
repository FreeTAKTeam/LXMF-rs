#!/usr/bin/env bash
set -euo pipefail

# Multi-node mesh simulation harness for nightly/scheduled interoperability checks.
# Launches a ring of reticulumd nodes and validates announce propagation + delivery workflows.

MANIFEST_PATH="${MANIFEST_PATH:-crates/apps/rns-tools/Cargo.toml}"
NODES="${NODES:-5}"
TIMEOUT_SECS="${TIMEOUT_SECS:-60}"
MODES="${MODES:-}"

if [[ ! -f "${MANIFEST_PATH}" ]]; then
  echo "MANIFEST_PATH not found: ${MANIFEST_PATH}" >&2
  exit 1
fi

if [[ -n "${MODES}" ]]; then
  IFS=',' read -r -a mode_array <<<"${MODES}"
  mode_args=()
  for mode in "${mode_array[@]}"; do
    trimmed="$(echo "${mode}" | xargs)"
    if [[ -n "${trimmed}" ]]; then
      mode_args+=(--mode "${trimmed}")
    fi
  done
else
  mode_args=()
fi

cmd=(
  cargo run --quiet --manifest-path "${MANIFEST_PATH}" --bin rnx --
  mesh-sim --nodes "${NODES}" --timeout-secs "${TIMEOUT_SECS}"
)
if [[ ${#mode_args[@]} -gt 0 ]]; then
  cmd+=("${mode_args[@]}")
fi

output="$("${cmd[@]}" 2>&1)"
echo "${output}"

grep -q "MESH ok: nodes=${NODES} announce propagation established across mesh" <<<"${output}"
grep -q "MESH ok: multi-hop delivery workflows completed" <<<"${output}"

echo "MESH_SIM_SUCCESS nodes=${NODES} timeout_secs=${TIMEOUT_SECS}"
