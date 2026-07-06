#!/usr/bin/env bash
set -euo pipefail

VERSION="${OPENAPI_GENERATOR_VERSION:-7.20.0}"
CACHE_ROOT="${XDG_CACHE_HOME:-$HOME/.cache}/lxmf-rs/openapi-generator"
JAR_PATH="${CACHE_ROOT}/openapi-generator-cli-${VERSION}.jar"
URL="https://repo1.maven.org/maven2/org/openapitools/openapi-generator-cli/${VERSION}/openapi-generator-cli-${VERSION}.jar"

mkdir -p "${CACHE_ROOT}"

if [[ ! -f "${JAR_PATH}" ]]; then
  curl -fsSL "${URL}" -o "${JAR_PATH}"
fi

filter_python_schema_warnings=false
prev=""
for arg in "$@"; do
  if [[ "${prev}" == "-g" || "${prev}" == "--generator-name" ]] && [[ "${arg}" == "python" ]]; then
    filter_python_schema_warnings=true
    break
  fi
  prev="${arg}"
done

if [[ "${filter_python_schema_warnings}" == "true" ]]; then
  set +e
  java -jar "${JAR_PATH}" "$@" 2>&1 | sed \
    -e '/^\[main\] WARN  o\.o\.codegen\.utils\.ModelUtils - Failed to get the schema name: null$/d' \
    -e '/^\[main\] WARN  o\.o\.c\.l\.AbstractPythonCodegen - Codegen property is null (e\.g\. map\/dict of undefined type)\. Default to typing\.Any\.$/d'
  status=${PIPESTATUS[0]}
  set -e
  exit "${status}"
fi

exec java -jar "${JAR_PATH}" "$@"
