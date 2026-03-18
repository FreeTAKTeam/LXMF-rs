# LXMF-rs Monorepo

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/FreeTAKTeam/LXMF-rs)

Rust monorepo for LXMF and Reticulum with strict library/app boundaries and enterprise quality gates.

## Repository Layout

```text
LXMF-rs/
├── crates/
│   ├── libs/
│   │   ├── lxmf-core/
│   │   ├── lxmf-sdk/
│   │   ├── rns-core/
│   │   ├── rns-transport/
│   │   ├── rns-rpc/
│   │   └── test-support/
│   ├── apps/
│   │   ├── lxmf-cli/
│   │   ├── reticulumd/
│   │   └── rns-tools/
└── docs/
    ├── adr/
    ├── architecture/
    ├── contracts/
    ├── migrations/
    └── runbooks/
├── tools/
│   └── scripts/
├── xtask/
└── target/

Note: legacy migration-only implementation crates are retained under
`crates/internal/` and are excluded from the active workspace graph.
```

## Public Crates

- `lxmf-core`: message/payload/identity primitives.
- `lxmf-sdk`: host-facing client API (`start/send/cancel/status/configure/poll/snapshot/shutdown`).
- `rns-embedded-runtime`: node-centric embedded runtime facade with lifecycle, event, and managed `std` driver support.
- `rns-embedded-ffi`: C ABI for embedded/manual-tick compatibility and the v1 node-centric API.
- `rns-core`: Reticulum cryptographic and packet primitives.
- `rns-transport`: transport + iface + receipt/resource API.
- `rns-rpc`: RPC request/response/event contracts and bridges.

## Build and Validation

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo doc --workspace --no-deps
./tools/scripts/check-boundaries.sh
```

or via `xtask`:

```bash
cargo xtask ci
cargo xtask release-check
cargo xtask package-daemon-bundle --version 0.1
cargo xtask api-diff
cargo xtask python-impl-bench-compare
cargo xtask python-impl-bench-compare --profile report
cargo xtask python-impl-bench-report
```

For fast local iteration on one binary, prefer narrow commands:

```bash
make check-bin PKG=lxmf-cli BIN=lxmd
make run-bin PKG=rns-tools BIN=rnsd ARGS="--help"
make package-daemon-bundle VERSION=0.1
make python-lxmd-smoke
```

For release artifacts, `cargo xtask package-daemon-bundle` builds the
host-native `lxmd` and `reticulumd` binaries, generates
`lxmd.example.config`, copies `README.md`, and writes a release archive under
`target/release-bundles/`. The command emits `.zip` bundles on Windows and
`.tar.gz` bundles on macOS/Linux.

On macOS, Gatekeeper may quarantine a downloaded release bundle because the
project does not currently ship signed/notarized binaries. If that happens,
remove the quarantine attribute after extracting the archive:

```bash
xattr -dr com.apple.quarantine /path/to/lxmd-daemon-<version>-macos-arm64
chmod +x /path/to/lxmd-daemon-<version>-macos-arm64/lxmd
chmod +x /path/to/lxmd-daemon-<version>-macos-arm64/reticulumd
```

You can also remove the attribute on just one binary, for example
`xattr -d com.apple.quarantine ./lxmd`.

If `sccache` is installed and you want to use it, set `RUSTC_WRAPPER=sccache`
in your shell before building. The workspace no longer hardcodes a Unix-only
wrapper path, so native Windows builds work without extra Cargo config.

Cross-language protocol benchmark reports are written to
`target/criterion/python-impl-compare.txt` and compare Rust core paths to the
installed Python `RNS` and `LXMF` implementations. Benchmark configuration
lives in `tools/benchmarks/python_impl.toml`, and the operating runbook is
`docs/runbooks/python-impl-benchmarking.md`. For publishable claims, use
`cargo xtask python-impl-bench-report`, which runs the stricter `report`
profile repeatedly and writes aggregated timing/resource artifacts under
`target/criterion/python-impl-report/`.

For daemon-level mixed-runtime smoke coverage, `make python-lxmd-smoke` launches
a Rust `lxmd` node and an installed Python `lxmd` node together, then verifies
cross-runtime remote status control and Python-originated inbound delivery into
the Rust node.

## Developer Bootstrap

One-command local setup:

```bash
make bootstrap
```

Direct script form:

```bash
./tools/scripts/bootstrap-dev.sh
```

Verification-only mode (used by CI):

```bash
./tools/scripts/bootstrap-dev.sh --check --skip-smoke
```

## Binaries

- `lxmf-cli`
- `lxmd`
- `reticulumd`
- `rncp`, `rnid`, `rnir`, `rnodeconf`, `rnpath`, `rnpkg`, `rnprobe`, `rnsd`, `rnstatus`, `rnx`

Run examples:

```bash
cargo run -p lxmf-cli -- --help
cargo run -p reticulumd -- --help
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
```

## Contracts and Runbooks

- Compatibility contract: `docs/contracts/compatibility-contract.md`
- Compatibility matrix: `docs/contracts/compatibility-matrix.md`
- Third-party compatibility kit: `docs/contracts/third-party-compatibility-kit.md`
- Support and LTS policy: `docs/contracts/support-policy.md`
- Extension registry: `docs/contracts/extension-registry.md`
- RPC contract: `docs/contracts/rpc-contract.md`
- gRPC adoption and migration: `docs/contracts/grpc-adoption-and-migration.md`
- Payload contract: `docs/contracts/payload-contract.md`
- gRPC getting started: `docs/grpc-getting-started.md`
- gRPC runbook: `docs/runbooks/grpc.md`
- gRPC release notes: `docs/releases/2026-03-grpc-surface.md`
- Release readiness: `docs/runbooks/release-readiness.md`
- Release scorecard process: `docs/architecture/overview.md`

## SDK Guide

- Guide index: `docs/sdk/README.md`
- Quickstart: `docs/sdk/quickstart.md`
- Profiles/configuration: `docs/sdk/configuration-profiles.md`
- Config cookbook: `docs/runbooks/sdk-config-cookbook.md`
- Lifecycle/events: `docs/sdk/lifecycle-and-events.md`
- Advanced embedding: `docs/sdk/advanced-embedding.md`

## Embedded Node FFI

- Header: `crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h`
- Guide and example: `crates/libs/rns-embedded-ffi/README.md`
- Stable core contract: lifecycle, status, capability probe, send/broadcast, subscriptions, structured errors
- Compatibility surface: legacy manual tick, raw wire ingress/egress, low-level queueing
- Extension surface: numeric extension IDs validated by `docs/fixtures/embedded/public-node-api-v1/extension-ids.json`
- `v1` node-centric API: `rns_embedded_v1_node_new/start/stop/restart/get_status/send/broadcast/set_log_level/subscribe_events`
- legacy compatibility API remains available for manual tick, raw wire ingress/egress, and low-level queueing

## Governance

- Governance docs: `SECURITY.md`
- Security policy: `SECURITY.md`
- Code ownership: `.github/CODEOWNERS`

## Linux daemon setup (systemd)

The following installs a long-running `lxmd` service. `lxmd` also launches `reticulumd`, so a single unit is enough for most deployments.

1. Install binaries (from source)

```bash
cargo build --release -p lxmf-cli -p reticulumd
sudo install -m 0755 target/release/lxmd /usr/local/bin/lxmd
sudo install -m 0755 target/release/reticulumd /usr/local/bin/reticulumd
```

2. Create a dedicated service user and daemon directories

```bash
sudo useradd --system --create-home --shell /usr/sbin/nologin lxmd
sudo mkdir -p /etc/lxmf/lxmd /etc/lxmf/reticulumd /var/log/lxmf
sudo chown -R lxmd:lxmd /etc/lxmf /var/log/lxmf
```

3. Create a starting config file for `lxmd`

```bash
sudo mkdir -p /etc/lxmf/lxmd
sudo chown lxmd:lxmd /etc/lxmf/lxmd
sudo -u lxmd /usr/local/bin/lxmd --exampleconfig > /etc/lxmf/lxmd/config
sudo chmod 600 /etc/lxmf/lxmd/config
```

Optional: set an explicit Reticulum config for `reticulumd` (instead of relying on generated defaults).

```bash
sudo cp crates/apps/reticulumd/examples/service-reference.toml /etc/lxmf/reticulumd/config.toml
sudo chown lxmd:lxmd /etc/lxmf/reticulumd/config.toml
```

4. Install a systemd unit for the daemon

```bash
sudo tee /etc/systemd/system/lxmd.service > /dev/null <<'EOF'
[Unit]
Description=LXMF daemon (lxmd + reticulumd)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=lxmd
Group=lxmd
WorkingDirectory=/etc/lxmf/lxmd
ExecStart=/usr/local/bin/lxmd --config /etc/lxmf/lxmd/config --rnsconfig /etc/lxmf/reticulumd/config.toml
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF
```

If you are not using `/etc/lxmf/reticulumd/config.toml`, remove `--rnsconfig /etc/lxmf/reticulumd/config.toml` from `ExecStart` and run only with `--config`.

5. Enable and start the service

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now lxmd.service
sudo systemctl status lxmd.service --no-pager
```

6. Tail logs and verify health

```bash
sudo journalctl -u lxmd.service -f
```

## License

MIT

## Using the official GitHub release binaries

Latest release artifacts are published at:

[https://github.com/FreeTAKTeam/LXMF-rs/releases](https://github.com/FreeTAKTeam/LXMF-rs/releases)

1. Open the release page and download the package for your platform.

2. Linux/macOS

```bash
tar -xzf lxmd-daemon-<version>-linux-amd64.tar.gz
tar -xzf lxmd-daemon-<version>-macos-arm64.tar.gz
```

3. Windows

```powershell
Expand-Archive lxmd-daemon-<version>-windows-x86_64.zip .
```

4. Run directly for validation

```bash
./lxmd --help
./reticulumd --help
```

5. Generate a starter `lxmd` config and follow the same daemon setup flow as above

```bash
./lxmd --exampleconfig > /tmp/lxmd.config
```

If you are using Linux and the Linux daemon guide above, point `--config` at the downloaded config file and keep binaries in place via your package manager path or your custom install path.
