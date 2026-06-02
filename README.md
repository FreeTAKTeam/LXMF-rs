# LXMF-rs Monorepo

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/FreeTAKTeam/LXMF-rs)

Rust monorepo for LXMF and Reticulum with strict library/app boundaries and enterprise quality gates.

## Start Here

- Contributor workflow: `CONTRIBUTING.md`
- Current status and execution order: `docs/status/current-roadmap.md`
- Docs map and retention rules: `docs/README.md`
- SDK guide: `docs/sdk/README.md`
- Support policy: `docs/contracts/support-policy.md`

## Workspace Layout

```text
LXMF-rs/
├── crates/
│   ├── libs/
│   │   ├── lxmf/
│   │   ├── lxmf-core/
│   │   ├── lxmf-sdk/
│   │   ├── reticulum-rs/
│   │   ├── rns-core/
│   │   ├── rns-embedded-core/
│   │   ├── rns-embedded-ffi/
│   │   ├── rns-embedded-runtime/
│   │   ├── rns-transport/
│   │   ├── rns-rpc/
│   │   └── test-support/
│   ├── apps/
│   │   ├── lxmf-cli/
│   │   ├── reticulumd/
│   │   └── rns-tools/
├── docs/
    ├── adr/
    ├── architecture/
    ├── contracts/
    ├── fixtures/
    ├── migrations/
    ├── runbooks/
    ├── schemas/
    └── sdk/
├── examples/
├── tools/
│   └── scripts/
├── scripts/
└── xtask/
```

`Cargo.toml` is the source of truth for active workspace members. Retired
migration-era crates are not kept in the repository surface.

## Active Libraries

- `lxmf-wire` (`crates/libs/lxmf-core`): message/payload/identity primitives.
- `lxmf`: umbrella crate for `lxmf-sdk` and `lxmf-wire`.
- `lxmf-sdk`: host-facing client API (`start/send/cancel/status/configure/poll/snapshot/shutdown`).
- `rns-embedded-runtime`: node-centric embedded runtime facade with lifecycle, event, and managed `std` driver support.
- `rns-embedded-ffi`: C ABI for embedded/manual-tick compatibility and the v1 node-centric API.
- `rns-embedded-core`: shared embedded/runtime types and fixtures.
- `reticulum-rs`: umbrella crate for the Reticulum stack crates.
- `reticulum-rs-core` (`crates/libs/rns-core`): Reticulum cryptographic and packet primitives.
- `reticulum-rs-transport` (`crates/libs/rns-transport`): transport + iface + receipt/resource API.
- `reticulum-rs-rpc` (`crates/libs/rns-rpc`): JSON-RPC request/response/event contracts and bridges.
- `test-support`: schema/fixture validation and integration-test helpers.

Published crates.io entry points:

- `lxmf`
- `lxmf-sdk`
- `lxmf-wire`
- `reticulum-rs`
- `reticulum-rs-core`
- `reticulum-rs-transport`
- `reticulum-rs-rpc`

## Published Crates

Main entry points:

- [`lxmf`](https://crates.io/crates/lxmf) ![Crates.io Version](https://img.shields.io/crates/v/lxmf) ([docs.rs](https://docs.rs/lxmf)): umbrella crate for LXMF wire types and the high-level SDK.
- [`reticulum-rs`](https://crates.io/crates/reticulum-rs) ![Crates.io Version](https://img.shields.io/crates/v/reticulum-rs) ([docs.rs](https://docs.rs/reticulum-rs)): umbrella crate for the Reticulum stack crates.

Component crates:

- [`lxmf-sdk`](https://crates.io/crates/lxmf-sdk) ![Crates.io Version](https://img.shields.io/crates/v/lxmf-sdk) ([docs.rs](https://docs.rs/lxmf-sdk)): high-level Rust SDK for LXMF clients.
- [`lxmf-wire`](https://crates.io/crates/lxmf-wire) ![Crates.io Version](https://img.shields.io/crates/v/lxmf-wire) ([docs.rs](https://docs.rs/lxmf-wire)): LXMF wire format, message primitives, and identity helpers.
- [`reticulum-rs-core`](https://crates.io/crates/reticulum-rs-core) ![Crates.io Version](https://img.shields.io/crates/v/reticulum-rs-core) ([docs.rs](https://docs.rs/reticulum-rs-core)): core Reticulum cryptographic and packet primitives.
- [`reticulum-rs-transport`](https://crates.io/crates/reticulum-rs-transport) ![Crates.io Version](https://img.shields.io/crates/v/reticulum-rs-transport) ([docs.rs](https://docs.rs/reticulum-rs-transport)): transport, interface, receipt, and resource layers.
- [`reticulum-rs-rpc`](https://crates.io/crates/reticulum-rs-rpc) ![Crates.io Version](https://img.shields.io/crates/v/reticulum-rs-rpc) ([docs.rs](https://docs.rs/reticulum-rs-rpc)): JSON-RPC request, response, event, and daemon bridge contracts.

## Active Applications

- `lxmf-cli`
- `reticulumd`
- `rns-tools`

## Bootstrap

Recommended:

```bash
make bootstrap
```

Direct script form:

```bash
./tools/scripts/bootstrap-dev.sh
```

Verification-only mode:

```bash
./tools/scripts/bootstrap-dev.sh --check --skip-smoke
```

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
cargo run -p xtask -- architecture-checks
cargo run -p xtask -- sdk-docs-check
cargo run -p xtask -- sdk-migration-check
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

## Binaries

- `lxmf-cli`
- `lxmd`
- `reticulumd`
- `rnsd`, `rnx`

Run examples:

```bash
cargo run -p lxmf-cli -- --help
cargo run -p reticulumd -- --help
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
```

## Documentation Entry Points

- Docs map: `docs/README.md`
- Current status: `docs/status/current-roadmap.md`
- API surface and stability: `docs/lxmf-rs-api.md`
- CLI quick reference: `docs/lxmf-cli.md`
- Architecture overview: `docs/architecture/overview.md`
- JSON and wire-field mapping: `docs/architecture/json-lxmf-fields.md`
- Compatibility contract: `docs/contracts/compatibility-contract.md`
- Compatibility matrix: `docs/contracts/compatibility-matrix.md`
- Third-party compatibility kit: `docs/contracts/third-party-compatibility-kit.md`
- Support and LTS policy: `docs/contracts/support-policy.md`
- Extension registry: `docs/contracts/extension-registry.md`
- RPC contract: `docs/contracts/rpc-contract.md`
- Payload contract: `docs/contracts/payload-contract.md`
- reticulumd operational deployment: `docs/runbooks/reticulumd-operational-deployment.md`
- crates.io publish plan: `docs/runbooks/crates-io-publish-plan.md`
- Release readiness: `docs/runbooks/release-readiness.md`

## crates.io Consumers

For library consumers, prefer the published package names rather than the
workspace directory names:

```toml
[dependencies]
lxmf = "0.3.0"
reticulum-rs = "0.2.0"
```

Or depend on the component crates directly:

```toml
[dependencies]
lxmf-sdk = "0.2.1"
reticulum-rs-rpc = "0.3.0"
```

## SDK Guide

- Guide index: `docs/sdk/README.md`
- Quickstart: `docs/sdk/quickstart.md`
- Profiles/configuration: `docs/sdk/configuration-profiles.md`
- Config cookbook: `docs/runbooks/sdk-config-cookbook.md`
- Lifecycle/events: `docs/sdk/lifecycle-and-events.md`
- Remote mTLS: `docs/sdk/remote-mtls.md`
- Delivery states: `docs/sdk/delivery-states.md`
- Error handling: `docs/sdk/error-handling.md`
- Advanced embedding: `docs/sdk/advanced-embedding.md`

## Release Bundles

`cargo xtask package-daemon-bundle` builds the host-native `lxmd` and
`reticulumd` binaries, generates `lxmd.example.config`, copies `README.md`, and
writes a release archive under `target/release-bundles/`. The command emits
`.zip` bundles on Windows and `.tar.gz` bundles on macOS/Linux.

On macOS, Gatekeeper may quarantine a downloaded release bundle because the
project does not currently ship signed/notarized binaries. If that happens,
remove the quarantine attribute after extracting the archive:

```bash
xattr -dr com.apple.quarantine /path/to/lxmd-daemon-<version>-macos-arm64
chmod +x /path/to/lxmd-daemon-<version>-macos-arm64/lxmd
chmod +x /path/to/lxmd-daemon-<version>-macos-arm64/reticulumd
```

## Embedded Node FFI

- Header: `crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h`
- Guide and example: `crates/libs/rns-embedded-ffi/README.md`
- Stable core contract: lifecycle, status, capability probe, send/broadcast, subscriptions, structured errors
- Compatibility surface: legacy manual tick, raw wire ingress/egress, low-level queueing
- Extension surface: numeric extension IDs validated by `docs/fixtures/embedded/public-node-api-v1/extension-ids.json`
- `v1` node-centric API: `rns_embedded_v1_node_new/start/stop/restart/get_status/send/broadcast/set_log_level/subscribe_events`
- legacy compatibility API remains available for manual tick, raw wire ingress/egress, and low-level queueing

## Governance

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

## Notes

- If `sccache` is installed and you want to use it, set
  `RUSTC_WRAPPER=sccache` before building.
- Cross-language benchmark configuration lives in
  `tools/benchmarks/python_impl.toml`, and the operating runbook is
  `docs/runbooks/python-impl-benchmarking.md`.
- For daemon-level mixed-runtime smoke coverage, `make python-lxmd-smoke`
  launches a Rust `lxmd` node and an installed Python `lxmd` node together.

## License

EPL
