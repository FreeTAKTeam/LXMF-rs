#!/usr/bin/env bash
set -euo pipefail

packages=(pkg-config libdbus-1-dev)
if [[ "${1:-}" == tools ]]; then
  packages+=(ffmpeg imagemagick graphicsmagick squashfs-tools)
fi

missing=()
for package in "${packages[@]}"; do
  if [[ "$(dpkg-query -W -f='${Status}' "$package" 2>/dev/null)" != 'install ok installed' ]]; then
    missing+=("$package")
  fi
done
if (( ${#missing[@]} == 0 )); then
  echo 'HIL dependencies already installed; no package downloads needed'
  exit 0
fi

echo "Installing missing HIL dependencies: ${missing[*]}"
# The runner's Azure mirror can throttle the large media dependency download.
if [[ "${1:-}" == tools && "${GITHUB_ACTIONS:-}" == true && -f /etc/apt/apt-mirrors.txt ]]; then
  echo 'Using the official Ubuntu HTTPS archive for media dependencies'
  sudo sed -i 's|http://azure\.archive\.ubuntu\.com|https://archive.ubuntu.com|g' /etc/apt/apt-mirrors.txt
fi
apt_options=(-o Acquire::Retries=2 -o Acquire::http::Timeout=20
  -o Acquire::https::Timeout=20 -o DPkg::Lock::Timeout=30
  -o APT::Update::Error-Mode=any)
sudo timeout --kill-after=10s 120s apt-get "${apt_options[@]}" update
sudo timeout --kill-after=10s 120s apt-get "${apt_options[@]}" install -y --no-install-recommends "${missing[@]}"
