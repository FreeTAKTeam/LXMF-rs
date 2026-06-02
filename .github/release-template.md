# LXMF-rs <version>

## Download

- Linux x64: `lxmd-daemon-<version>-linux-x64.tar.gz`
- macOS Apple Silicon: `lxmd-daemon-<version>-macos-arm64.tar.gz`
- Windows x64: `lxmd-daemon-<version>-windows-x64.zip`

## Included

- `lxmd`
- `reticulumd`
- `lxmd.example.config`
- `README.md`
- checksums

## Quick Start

1. Extract the archive into one directory.
2. Copy `lxmd.example.config` to the path you want to use for your daemon config.
3. Start the daemon:

```bash
./lxmd --config ./lxmd.example.config --rpc 127.0.0.1:4243
```

4. Check status:

```bash
./lxmd --config ./lxmd.example.config --status --rpc 127.0.0.1:4243
```

## macOS First Run

The macOS bundle is not currently code-signed or notarized. If Gatekeeper blocks
the binaries after download, remove the quarantine attribute:

```bash
xattr -dr com.apple.quarantine /path/to/lxmd-daemon-<version>-macos-arm64
chmod +x /path/to/lxmd-daemon-<version>-macos-arm64/lxmd
chmod +x /path/to/lxmd-daemon-<version>-macos-arm64/reticulumd
```

## Notes

- Replace `<version>` with the tagged release version.
- Keep this release page focused on the daemon bundle unless there is a
  user-facing reason to call out additional binaries.

## crates.io

- If this release train also ships library crates, list them here with exact versions.
- GitHub release versions and crates.io package versions do not need to match 1:1.
- When both ship together, they should point to the same release train and migration notes.
