#!/usr/bin/env bash
set -euo pipefail
tree_file="$(mktemp)"
trap 'rm -f "$tree_file"' EXIT
cargo tree -p lxmf-propagation --no-default-features --features embedded -e features --prefix none > "$tree_file"
cargo tree -p lxmf-runtime -e features --prefix none >> "$tree_file"
if rg '^(reticulum-rs-rpc|reticulum-rs-zeromq|zeromq|rns-rpc) ' "$tree_file"; then
  echo 'embedded propagation imports a daemon/RPC/ZeroMQ dependency' >&2
  exit 1
fi
cargo test -p lxmf-propagation --no-default-features --features embedded
