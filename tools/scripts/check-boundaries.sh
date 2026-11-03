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

pattern_matcher() {
  if command -v rg >/dev/null; then
    echo "rg"
    return
  fi
  require_command grep
  echo "grep"
}

PATTERN_MATCHER="$(pattern_matcher)"

search_text() {
  local pattern="$1"
  local path="$2"

  if [[ "${PATTERN_MATCHER}" == "rg" ]]; then
    rg -n -- "${pattern}" "${path}"
  else
    grep -RInP -- "${pattern}" "${path}"
  fi
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
  if search_text "\\b${symbol}::" "$crate_root" >/dev/null; then
    fail "${crate_root} imports '${symbol}' symbols"
  fi
}

check_source_use_imports() {
  local crate_root="$1"
  local symbol="$2"
  if search_text "^[[:space:]]*use .*\\b${symbol}::" "$crate_root" >/dev/null; then
    fail "${crate_root} imports '${symbol}' symbols in source use statements"
  fi
}

check_legacy_root_imports() {
  local crate_root="$1"
  if search_text "^[[:space:]]*use .*\\breticulum::" "$crate_root" >/dev/null; then
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
  jq -r '
    .packages[]
    | select(.manifest_path | gsub("\\\\"; "/") | contains("/crates/libs/"))
    | .name as $from
    | .dependencies[]?
    | "\($from)\t\(.name)"
  ' "$METADATA_FILE"
}

normalize_lines() {
  sed '/^[[:space:]]*$/d' | sort -u
}

load_allowlisted_edges() {
  local key="$1"
  jq -r --arg key "$key" '
    ((.workspace_metadata.boundaries[$key] // .metadata.boundaries[$key]) // [])[]?
    | gsub("->"; "\t")
  ' "$METADATA_FILE" | normalize_lines
}

metadata_lib_workspace_edges() {
  jq -r '
    .packages[]
    | select(.manifest_path | gsub("\\\\"; "/") | contains("/crates/libs/"))
    | .name as $from
    | .dependencies[]?
    | select(.path != null and (.path | gsub("\\\\"; "/") | contains("/crates/libs/")))
    | "\($from)\t\(.name)"
  ' "$METADATA_FILE" | normalize_lines
}

metadata_app_workspace_edges() {
  jq -r '
    .packages[]
    | select(.manifest_path | gsub("\\\\"; "/") | contains("/crates/apps/"))
    | .name as $from
    | .dependencies[]?
    | select(.path != null and ((.path | gsub("\\\\"; "/") | contains("/crates/libs/")) or (.path | gsub("\\\\"; "/") | contains("/crates/apps/"))))
    | "\($from)\t\(.name)"
  ' "$METADATA_FILE" | normalize_lines
}

check_edge_set_matches_allowlist() {
  local scope="$1"
  local actual="$2"
  local allowed="$3"

  if [ -z "$allowed" ]; then
    fail "workspace.metadata.boundaries.${scope} allowlist is missing or empty"
  fi

  if [ "$actual" != "$allowed" ]; then
    echo "boundary check failed: ${scope} dependency edges diverged from workspace allowlist" >&2
    diff -u <(printf '%s\n' "$allowed") <(printf '%s\n' "$actual") >&2 || true
    fail "${scope} dependency edge policy mismatch"
  fi
}

# The override supports seeded negative tests; production always uses Cargo metadata.
METADATA_FILE="${BOUNDARY_METADATA_FILE:-}"
if [[ -z "$METADATA_FILE" ]]; then
  METADATA_FILE="$(mktemp)"
  trap 'rm -f "${METADATA_FILE}"' EXIT
  cargo metadata --locked --no-deps --format-version 1 > "$METADATA_FILE"
fi
ENFORCE_RETM_LEGACY_SHIMS="${ENFORCE_RETM_LEGACY_SHIMS:-0}"
ENFORCE_LEGACY_APP_IMPORTS="${ENFORCE_LEGACY_APP_IMPORTS:-0}"
ARCH_REPORT_PATH="${ARCH_REPORT_PATH:-target/architecture/boundary-report.txt}"

require_command jq
require_command "${PATTERN_MATCHER}"


if search_text "crates/internal/lxmf-legacy|crates/internal/reticulum-legacy" Cargo.toml >/dev/null; then
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

# Designated core declarations are checked even when optional, target-specific,
# build-only or dev-only. Host test serializers are allowed; production JSON is not.
mapfile -t no_std_core < <(jq -r '(.workspace_metadata // .metadata).boundaries.no_std_core[]' "$METADATA_FILE")
mapfile -t forbidden_core_deps < <(jq -r '(.workspace_metadata // .metadata).boundaries.forbidden_core_deps[]' "$METADATA_FILE")
((${#no_std_core[@]} > 0 && ${#forbidden_core_deps[@]} > 0)) || fail "no_std core policy is empty"
for package in "${no_std_core[@]}"; do
  for dependency in "${forbidden_core_deps[@]}"; do
    if jq -e --arg package "$package" --arg dep "$dependency" '
        .packages[] | select(.name == $package) | .dependencies[]
        | select(.name == $dep and (.name != "serde_json" or .kind != "dev"))
      ' "$METADATA_FILE" >/dev/null; then
      fail "package '${package}' depends on forbidden crate '${dependency}'"
    fi
  done
  crate_root="$(jq -r --arg package "$package" '.packages[] | select(.name == $package) | .manifest_path' "$METADATA_FILE")"
  [[ -n "$crate_root" ]] || fail "designated no_std package '${package}' is missing"
  for symbol in tokio tokio_util tokio_serial zeromq rusqlite clap reticulumd lxmf_sdk meshchat_api; do
    check_source_imports "$(dirname "$crate_root")/src" "$symbol"
  done
done

# 1) Core dependency constraints (hard policy).
check_forbidden_dependency "lxmf-wire" "tokio" "clap" "ureq"
check_forbidden_dependency "reticulum-rs-core" "tokio" "clap"
check_forbidden_dependency "reticulum-rs-transport" "clap"
check_forbidden_dependency "reticulum-rs-rpc" "clap"
check_no_app_dependencies "lxmf-wire" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "lxmf-sdk" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "reticulum-rs-core" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "reticulum-rs-transport" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "reticulum-rs-rpc" "lxmf-cli" "rns-tools" "reticulumd"
check_no_app_dependencies "test-support" "lxmf-cli" "rns-tools" "reticulumd"

allowed_library_edges="$(load_allowlisted_edges "allowed_library_edges")"
allowed_app_edges="$(load_allowlisted_edges "allowed_app_edges")"
actual_library_edges="$(metadata_lib_workspace_edges)"
actual_app_edges="$(metadata_app_workspace_edges)"

mkdir -p "$(dirname "${ARCH_REPORT_PATH}")"
{
  echo "# Architecture Boundary Report"
  echo
  echo "## Allowed library edges"
  printf '%s\n' "${allowed_library_edges}"
  echo
  echo "## Actual library edges"
  printf '%s\n' "${actual_library_edges}"
  echo
  echo "## Allowed app edges"
  printf '%s\n' "${allowed_app_edges}"
  echo
  echo "## Actual app edges"
  printf '%s\n' "${actual_app_edges}"
} > "${ARCH_REPORT_PATH}"

check_edge_set_matches_allowlist "library" "$actual_library_edges" "$allowed_library_edges"
check_edge_set_matches_allowlist "app" "$actual_app_edges" "$allowed_app_edges"

# 2) Explicit manifest-pattern checks for accidental legacy wiring.
for manifest in crates/libs/*/Cargo.toml; do
  if search_text "lxmf_legacy|reticulum_legacy|crates/internal/" "$manifest" >/dev/null; then
    fail "${manifest} must not reference legacy shim crates"
  fi
  if search_text "path\\s*=\\s*\"\\.\\./\\.\\./apps/|path\\s*=\\s*\"\\.\\./apps/" "$manifest" >/dev/null; then
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
  if find crates/internal -type d -path "*/tests/fixtures/python" -print -quit | grep -q .; then
    fail "Legacy Python fixtures under crates/*/tests/fixtures/python are not allowed in this repo"
  fi
fi
if [ -d "crates/apps" ]; then
  if find crates/apps -type d -path "*/tests/fixtures/python" -print -quit | grep -q .; then
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
  if search_text "\\b(legacy_reticulum|reticulum::|reticulum-rs)\\b" crates/libs/rns-transport/src crates/libs/rns-rpc/src >/dev/null; then
    fail "legacy reticulum symbols still referenced in rns transport/rpc source"
  fi
  # Only flag actual legacy dependency declarations here. Package names and docs.rs
  # URLs for the active crates also contain `reticulum-rs`, so a raw substring
  # search would create false positives and break strict mode permanently.
  if search_text "^reticulum-rs\\s*=\\s*\\{|package\\s*=\\s*\"reticulum-rs\"|legacy_reticulum|crates/internal/(reticulum|lxmf)-legacy" \
    crates/libs/rns-transport/Cargo.toml crates/libs/rns-rpc/Cargo.toml >/dev/null; then
    fail "rns transport/rpc still depend on legacy reticulum crate"
  fi
else
  echo "boundary notice: rns-transport/rns-rpc legacy shim references are currently allowed"
fi

# A host consumer may compile while silently enabling core/std. Inspect the
# real feature graph separately from the bare-metal, isolated crate builds.
feature_tree="$(cargo tree --locked -p rns-embedded-mininode --no-default-features -e features --prefix none)"
if grep -Fq 'reticulum-rs-core feature "std"' <<<"$feature_tree"; then
  fail "rns-embedded-mininode without defaults enables reticulum-rs-core/std"
fi

echo "boundary report: ${ARCH_REPORT_PATH}"
echo "boundary checks: ok"
