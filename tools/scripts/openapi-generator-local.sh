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

exec java -jar "${JAR_PATH}" "$@"
