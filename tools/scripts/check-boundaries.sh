#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "boundary check failed: $*" >&2
  exit 1
}

require_command() {
  local cmd="$1"
  command -v "$cmd" >/dev/null || fail "required tool missing: ${cmd}"
}

has_dependency() {
  local package="$1"
  local dep="$2"
  jq -e --arg package "$package" --arg dep "$dep" '
    .packages[]
    | select(.name == $package)
    | .dependencies[]
    | select(.name == $dep)
  ' "$METADATA_FILE" >/dev/null 2>&1
}

check_source_imports() {
  local crate_root="$1"
  local symbol="$2"
  if rg -n "\\b${symbol}::" "$crate_root" >/dev/null; then
    fail "${crate_root} imports '${symbol}' symbols"
  fi
}

check_source_use_imports() {
  local crate_root="$1"
  local symbol="$2"
  if rg -n "^[[:space:]]*use .*\\b${symbol}::" "$crate_root" >/dev/null; then
    fail "${crate_root} imports '${symbol}' symbols in source use statements"
  fi
}

check_legacy_root_imports() {
  local crate_root="$1"
  if rg -n "^[[:space:]]*use .*\\breticulum::" "$crate_root" >/dev/null; then
    fail "${crate_root} still imports legacy 'reticulum::' paths"
  fi
}

check_forbidden_dependency() {
  local package="$1"
  shift
  local dep
  for dep in "$@"; do
    if has_dependency "$package" "$dep"; then
      fail "package '${package}' depends on forbidden crate '${dep}'"
    fi
  done
}

check_no_app_dependencies() {
  local package="$1"
  shift
  local app
  for app in "$@"; do
    if has_dependency "$package" "$app"; then
      fail "package '${package}' must not depend on app crate '${app}'"
    fi
  done
}

metadata_deps() {
  jq -r '.packages[] | select(.manifest_path | contains("/crates/libs/")) | .name as $from | .dependencies[]? | "\($from)\t\(.name)"' "$METADATA_FILE"
}

METADATA_FILE="$(mktemp)"
trap 'rm -f "${METADATA_FILE}"' EXIT
ENFORCE_RETM_LEGACY_SHIMS="${ENFORCE_RETM_LEGACY_SHIMS:-0}"
ENFORCE_LEGACY_APP_IMPORTS="${ENFORCE_LEGACY_APP_IMPORTS:-0}"

require_command jq
require_command rg

cargo metadata --no-deps --format-version 1 > "$METADATA_FILE"

if rg -n "crates/internal/lxmf-legacy|crates/internal/reticulum-legacy" Cargo.toml >/dev/null; then
  fail "Workspace membership must not include crates/internal legacy crates"
fi

if [ -d "LXMF" ]; then
  fail "Top-level LXMF python package must be externalized from this repository"
fi
if [ -f "setup.py" ] || [ -f "requirements.txt" ]; then
  fail "Python packaging files must be owned by external interoperability repo"
fi
if [ -d "lxmf.egg-info" ]; then
  fail "Python egg metadata must not be present in this repository"
fi

# 1) Core dependency constraints (hard policy).
check_forbidden_dependency "lxmf-core" "tokio" "clap" "ureq"
check_forbidden_dependency "rns-core" "tokio" "clap"
check_no_app_dependencies "lxmf-core" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "rns-core" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "rns-transport" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "rns-rpc" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "test-support" "lxmf-cli" "rns-tools" "reticulumd"

# 2) Explicit manifest-pattern checks for accidental legacy wiring.
for manifest in crates/libs/*/Cargo.toml; do
  if rg -n "lxmf_legacy|reticulum_legacy|crates/internal/" "$manifest" >/dev/null; then
    fail "${manifest} must not reference legacy shim crates"
  fi
  if rg -n "path\\s*=\\s*\"\\.\\./\\.\\./apps/|path\\s*=\\s*\"\\.\\./apps/" "$manifest" >/dev/null; then
    fail "${manifest} must not depend on crates/apps/*"
  fi
done

# 3) Validate app crate directionality by dependency graph.
while IFS=$'\t' read -r from to; do
  case "$to" in
    lxmf-cli|reticulumd|rns-tools)
      fail "library crate '${from}' must not depend on app crate '${to}'"
      ;;
  esac
done <<<"$(metadata_deps)"

if [ -d "crates/internal" ]; then
  if find crates/internal -type d -path "*/tests/fixtures/python" | rg -q .; then
    fail "Legacy Python fixtures under crates/*/tests/fixtures/python are not allowed in this repo"
  fi
fi
if [ -d "crates/apps" ]; then
  if find crates/apps -type d -path "*/tests/fixtures/python" | rg -q .; then
    fail "Legacy Python fixtures under crates/apps/*/tests/fixtures/python are not allowed in this repo"
  fi
fi

if (( ENFORCE_LEGACY_APP_IMPORTS == 1 )); then
  # 3a) Ensure app crates migrate away from legacy crate import paths.
  check_legacy_root_imports "crates/apps/lxmf-cli/src"
  check_legacy_root_imports "crates/apps/rns-tools/src"
  check_legacy_root_imports "crates/apps/reticulumd/src"
  check_legacy_root_imports "crates/apps/reticulumd/tests"
else
  echo "boundary notice: legacy app import gate is currently allowed"
fi

# 4) Keep core crates free of direct runtime/CLI imports at source level.
check_source_imports "crates/libs/lxmf-core/src" "tokio"
check_source_imports "crates/libs/lxmf-core/src" "clap"
check_source_imports "crates/libs/rns-core/src" "tokio"
check_source_imports "crates/libs/rns-core/src" "clap"

check_source_use_imports "crates/libs/rns-core/src" "reticulum"
check_source_use_imports "crates/libs/rns-core/src" "legacy_reticulum"

check_source_use_imports "crates/libs/lxmf-core/src" "reticulum"
check_source_use_imports "crates/libs/lxmf-core/src" "legacy_reticulum"

if (( ENFORCE_RETM_LEGACY_SHIMS == 1 )); then
  if rg -n "\b(legacy_reticulum|reticulum::|reticulum-rs)\\b" crates/libs/rns-transport/src crates/libs/rns-rpc/src >/dev/null; then
    fail "legacy reticulum symbols still referenced in rns transport/rpc source"
  fi
  if rg -n "reticulum-rs|legacy_reticulum" crates/libs/rns-transport/Cargo.toml crates/libs/rns-rpc/Cargo.toml >/dev/null; then
    fail "rns transport/rpc still depend on legacy reticulum crate"
  fi
else
  echo "boundary notice: rns-transport/rns-rpc legacy shim references are currently allowed"
fi

echo "boundary checks: ok"
