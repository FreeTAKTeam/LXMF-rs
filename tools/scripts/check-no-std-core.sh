#!/usr/bin/env bash
# Build every designated protocol core in isolation on a target without std.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"
NO_STD_TARGET="${NO_STD_TARGET:-thumbv7em-none-eabi}"
if ! rustup target list --installed 2>/dev/null | grep -Fxq "$NO_STD_TARGET"; then
  echo "error: Rust target ${NO_STD_TARGET} is not installed; run rustup target add ${NO_STD_TARGET}" >&2
  exit 1
fi
metadata="$(mktemp)"
trap 'rm -f "$metadata"' EXIT
cargo metadata --locked --no-deps --format-version 1 > "$metadata"
mapfile -t packages < <(jq -r '(.workspace_metadata // .metadata).boundaries.no_std_core[]' "$metadata")
((${#packages[@]} > 0)) || { echo 'no_std core policy is empty' >&2; exit 1; }
for package in "${packages[@]}"; do
  mapfile -t feature_sets < <(jq -er --arg package "$package" '(.workspace_metadata // .metadata).boundaries.no_std_features[$package][]' "$metadata")
  ((${#feature_sets[@]} > 0)) || { echo "missing feature contract for $package" >&2; exit 1; }
  for features in "${feature_sets[@]}"; do
    args=(--locked -p "$package" --no-default-features --target "$NO_STD_TARGET")
    [[ -z "$features" ]] || args+=(--features "$features")
    echo "no_std check: ${package} features='${features}' target=${NO_STD_TARGET}"
    cargo check "${args[@]}"
    # Includes active transitive normal/build dependencies, excluding host tests.
    # The bare-metal build rejects any other transitive std requirement as well.
    tree="$(cargo tree "${args[@]}" -e normal,build --prefix none)"
    while IFS= read -r dependency; do
      if grep -Eq "^${dependency} v" <<<"$tree"; then
        echo "no_std core '${package}' transitively depends on forbidden crate '${dependency}'" >&2
        exit 1
      fi
    done < <(jq -r '(.workspace_metadata // .metadata).boundaries.forbidden_core_deps[]' "$metadata")
  done
done
echo "no_std core checks passed on ${NO_STD_TARGET}"
