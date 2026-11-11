#!/usr/bin/env bash
set -euo pipefail

mirror_list=/etc/apt/apt-mirrors.txt
if [[ "${GITHUB_ACTIONS:-}" != true || ! -f "$mirror_list" ]]; then
  exit 0
fi

if timeout 5s getent ahosts archive.ubuntu.com >/dev/null; then
  # The runner's Azure mirror can throttle the large media download.
  if [[ "${1:-}" == tools ]]; then
    echo 'Using the official Ubuntu HTTPS archive for media dependencies'
    sudo sed -i 's|http://azure\.archive\.ubuntu\.com|https://archive.ubuntu.com|g' "$mirror_list"
  fi
elif ! timeout 5s getent ahosts security.ubuntu.com >/dev/null &&
     ! timeout 5s getent ahosts azure.archive.ubuntu.com >/dev/null; then
  fallback=https://mirrors.edge.kernel.org/ubuntu/
  if curl --fail --silent --head --location --connect-timeout 5 --max-time 10 "$fallback" >/dev/null; then
    echo 'Standard Ubuntu DNS unavailable; using the kernel.org Ubuntu mirror'
    printf '%s\n' "$fallback" | sudo tee "$mirror_list" >/dev/null
  else
    echo 'Standard Ubuntu DNS and the kernel.org mirror are unavailable; keeping the runner mirror list' >&2
  fi
fi
