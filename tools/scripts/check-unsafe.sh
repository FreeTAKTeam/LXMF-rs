#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "unsafe audit check failed: $*" >&2
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
  if [[ "${PATTERN_MATCHER}" == "rg" ]]; then
    rg -n -- "$@"
  else
    grep -RInP -- "$@"
  fi
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

has_safety_comment() {
  local file="$1"
  local line="$2"
  if (( line <= 1 )); then
    return 1
  fi

  local start=$(( line - 3 ))
  if (( start < 1 )); then
    start=1
  fi
  local end=$(( line - 1 ))
  if [[ "${PATTERN_MATCHER}" == "rg" ]]; then
    sed -n "${start},${end}p" "$file" | rg -q "SAFETY:"
  else
    sed -n "${start},${end}p" "$file" | grep -q "SAFETY:"
  fi
}

INVENTORY_PATH="${INVENTORY_PATH:-docs/architecture/unsafe-inventory.md}"

[[ -f "${INVENTORY_PATH}" ]] || fail "inventory not found: ${INVENTORY_PATH}"

require_command "${PATTERN_MATCHER}"

inventory_keys_file="$(mktemp)"
observed_keys_file="$(mktemp)"
trap 'rm -f "${inventory_keys_file}" "${observed_keys_file}"' EXIT

found_none_row=0

while IFS='|' read -r _ raw_id raw_file raw_line raw_invariant raw_owner raw_review _; do
  id="$(trim "${raw_id:-}")"
  file="$(trim "${raw_file:-}")"
  line="$(trim "${raw_line:-}")"
  invariant="$(trim "${raw_invariant:-}")"
  owner="$(trim "${raw_owner:-}")"
  review="$(trim "${raw_review:-}")"

  if [[ -z "${id}" || "${id}" == "Id" || "${id}" =~ ^-+$ ]]; then
    continue
  fi

  if [[ "${id}" == "NONE" ]]; then
    found_none_row=1
    continue
  fi

  [[ -n "${file}" && "${file}" != "n/a" ]] || fail "inventory entry '${id}' has invalid file"
  [[ -n "${line}" && "${line}" =~ ^[0-9]+$ ]] || fail "inventory entry '${id}' has invalid line"
  [[ -n "${invariant}" && "${invariant}" != "n/a" ]] || fail "inventory entry '${id}' missing invariant"
  [[ "${owner}" =~ ^@ ]] || fail "inventory entry '${id}' must use @owner handle"
  [[ "${review}" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || fail "inventory entry '${id}' has invalid review date"
  [[ -f "${file}" ]] || fail "inventory entry '${id}' references missing file '${file}'"

  key="${file}:${line}"
  if grep -Fqx "${key}" "${inventory_keys_file}"; then
    fail "duplicate inventory entry for ${key}"
  fi
  printf '%s\n' "${key}" >> "${inventory_keys_file}"
done < "${INVENTORY_PATH}"

while IFS=: read -r file line _; do
  [[ -n "${file}" ]] || continue
  [[ -n "${line}" ]] || continue
  [[ -f "${file}" ]] || fail "unsafe match references missing file '${file}'"
  [[ "${line}" =~ ^[0-9]+$ ]] || fail "unsafe match has invalid line '${line}' for ${file}"

  key="${file}:${line}"
  if ! grep -Fqx "${key}" "${observed_keys_file}"; then
    printf '%s\n' "${key}" >> "${observed_keys_file}"
  fi

  if ! has_safety_comment "${file}" "${line}"; then
    fail "${key} must include a nearby SAFETY: invariant comment"
  fi
  if ! grep -Fqx "${key}" "${inventory_keys_file}"; then
    fail "${key} has unsafe usage but no matching inventory entry in ${INVENTORY_PATH}"
  fi
done < <(
  if [[ "${PATTERN_MATCHER}" == "rg" ]]; then
    rg -n \
      --no-heading \
      --color never \
      --glob '*.rs' \
      '(\\bunsafe\\s*\\{|\\bunsafe\\s+fn\\b|\\bunsafe\\s+impl\\b|\\bunsafe\\s+trait\\b|\\bunsafe\\s+extern\\b)' \
      crates xtask/src 2>/dev/null || true
  else
    grep -RInP \
      --include='*.rs' \
      '(\\bunsafe\\s*\\{|\\bunsafe\\s+fn\\b|\\bunsafe\\s+impl\\b|\\bunsafe\\s+trait\\b|\\bunsafe\\s+extern\\b)' \
      crates xtask/src 2>/dev/null || true
  fi
)

if [[ ! -s "${observed_keys_file}" ]]; then
  (( found_none_row == 1 )) || fail "inventory must include a NONE row when no unsafe sites exist"
else
  (( found_none_row == 0 )) || fail "inventory NONE row must be removed when unsafe sites exist"
fi

while IFS= read -r key; do
  [[ -n "${key}" ]] || continue
  if ! grep -Fqx "${key}" "${observed_keys_file}"; then
    fail "inventory entry ${key} is stale (unsafe site no longer present)"
  fi
done < "${inventory_keys_file}"

echo "unsafe audit checks: ok"
