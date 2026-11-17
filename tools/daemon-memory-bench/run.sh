#!/usr/bin/env bash
set -euo pipefail
if [ "$#" -ne 3 ]; then
  echo "usage: run.sh EXPERIMENT.json python|rust RUN_ID" >&2
  exit 2
fi
script_dir=$(cd "$(dirname "$0")" && pwd)
exec unshare --user --map-root-user --net python3 "$script_dir/run.py" "$@"
