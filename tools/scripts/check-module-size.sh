#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "module-size check failed: $*" >&2
  exit 1
}

MAX_LINES_PROD="${MAX_LINES_PROD:-500}"
MAX_LINES_TEST="${MAX_LINES_TEST:-1200}"
ALLOWLIST_PATH="${ALLOWLIST_PATH:-docs/architecture/module-size-allowlist.txt}"

[[ -f "${ALLOWLIST_PATH}" ]] || fail "allowlist not found: ${ALLOWLIST_PATH}"

line_limit_for() {
  local file="$1"
  case "${file}" in
    */tests/*|*/fuzz/*|*/benches/*)
      echo "${MAX_LINES_TEST}"
      ;;
    *)
      echo "${MAX_LINES_PROD}"
      ;;
  esac
}

is_allowlisted() {
  local file="$1"
  read_allowlist | grep -Fqx "${file}"
}

read_allowlist() {
  sed -e 's/[[:space:]]*$//' "${ALLOWLIST_PATH}" \
    | sed -e '/^[[:space:]]*#/d' -e '/^[[:space:]]*$/d'
}

violations=()
while IFS= read -r file; do
  [[ -n "${file}" ]] || continue
  line_count="$(wc -l < "${file}" | tr -d '[:space:]')"
  limit="$(line_limit_for "${file}")"
  if [[ "${line_count}" -gt "${limit}" ]]; then
    if ! is_allowlisted "${file}"; then
      violations+=("${file}:${line_count} (limit=${limit})")
    fi
  fi
done < <(find crates/libs crates/apps xtask/src -type f -name '*.rs' | sort)

stale_allowlist=()
while IFS= read -r file; do
  [[ -n "${file}" ]] || continue
  if [[ ! -f "${file}" ]]; then
    stale_allowlist+=("${file} (missing)")
    continue
  fi
  line_count="$(wc -l < "${file}" | tr -d '[:space:]')"
  limit="$(line_limit_for "${file}")"
  if [[ "${line_count}" -le "${limit}" ]]; then
    stale_allowlist+=("${file} (now ${line_count} <= ${limit})")
  fi
done < <(read_allowlist)

if [[ "${#violations[@]}" -gt 0 ]]; then
  printf '%s\n' "${violations[@]}" >&2
  fail "found files above module-size budget that are not allowlisted"
fi

if [[ "${#stale_allowlist[@]}" -gt 0 ]]; then
  printf '%s\n' "${stale_allowlist[@]}" >&2
  fail "allowlist contains stale entries; remove them"
fi

echo "module-size checks: ok"
