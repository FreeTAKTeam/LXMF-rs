#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

required_files=(
  "crates/apps/reticulumd/examples/service-reference.toml"
  "crates/apps/lxmf-cli/examples/desktop-reference.toml"
  "crates/apps/rns-tools/examples/gateway-reference.toml"
  "docs/runbooks/reference-integrations.md"
)

for file in "${required_files[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "missing required reference integration artifact: $file" >&2
    exit 1
  fi
done

cargo run -p reticulumd -- --help >/dev/null
cargo run -p lxmf-cli -- --help >/dev/null
cargo run -p rns-tools --bin rnprobe -- --help >/dev/null
cargo run -p rns-tools --bin rnx -- --help >/dev/null
