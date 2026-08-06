# LXMF-rs Monorepo

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/FreeTAKTeam/LXMF-rs)

Rust monorepo for LXMF and Reticulum with strict library/app boundaries and
enterprise quality gates. The `0.9.8` line is a patch release over the
usable `0.9.5` sub-1.0 daemon/product baseline, with stricter failure handling,
receipt validation, storage migrations, typed SDK decoding, and operational
diagnostics. It is not a compatibility layer or a complete drop-in replacement
for every Python Reticulum/LXMF behavior.

## Start Here

- Contributor workflow: `CONTRIBUTING.md`
- Current status and execution order: `docs/status/current-roadmap.md`
- Release notes: `docs/release-notes-v0.9.8.md`
- Docs map and retention rules: `docs/README.md`
- SDK guide: `docs/sdk/README.md`
- Support policy: `docs/contracts/support-policy.md`

## Release Status

Current release train: `0.9.8`. The latest published stable artifacts are
`v0.9.7`; `v0.9.8` is being prepared from the reviewed post-`v0.9.7`
candidate on `origin/main`.

Use `docs/release-notes-v0.9.8.md` for the candidate release summary and
`docs/runbooks/release-readiness.md` for the release gate record. The
repository-level parity source of truth remains
`docs/status/current-roadmap.md`; the detailed parity supplements are
`docs/status/reticulum-parity-matrix.md` and
`docs/status/lxmf-parity-matrix.md`.

The `0.9.8` release scope covers the Rust libraries, SDK entry points, `lxmd`,
`reticulumd`, and `rns-tools`, plus host-native GitHub bundles for all
implemented user-facing tools. The patch train keeps the SDK-first product
shape while hardening authoritative return values, persistence, identity
parsing, link delivery, and error visibility. Operational substitutability is
materially stronger but remains bounded by the parity matrices and external
evidence requirements.
External-client compatibility claims for REM, RCH, Sideband, MeshChatX,
Columba, or other third-party clients require separate interop gate evidence.

## Workspace Layout

```text
LXMF-rs/
├── crates/
│   ├── libs/
│   │   ├── lxmf/
│   │   ├── lxmf-core/
│   │   ├── lxmf-sdk/
│   │   ├── lxmf-runtime/
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
- `lxmf-runtime`: in-process `SdkBackend` over Reticulum transport for embedded host applications.
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
- `lxmf-embedded-mini`
- `reticulum-rs`
- `reticulum-rs-core`
- `reticulum-rs-transport`
- `reticulum-rs-rpc`
- `rns-embedded-core`
- `rns-embedded-runtime`
- `rns-embedded-ffi`
- `rns-embedded-mininode`
- `lxmf-cli`
- `reticulumd`
- `rns-tools`

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

Embedded crates:

- [`lxmf-embedded-mini`](https://crates.io/crates/lxmf-embedded-mini) ![Crates.io Version](https://img.shields.io/crates/v/lxmf-embedded-mini) ([docs.rs](https://docs.rs/lxmf-embedded-mini)): no-alloc mini LXMF runtime for embedded targets.
- [`rns-embedded-core`](https://crates.io/crates/rns-embedded-core) ![Crates.io Version](https://img.shields.io/crates/v/rns-embedded-core) ([docs.rs](https://docs.rs/rns-embedded-core)): embedded-friendly Reticulum core primitives.
- [`rns-embedded-runtime`](https://crates.io/crates/rns-embedded-runtime) ![Crates.io Version](https://img.shields.io/crates/v/rns-embedded-runtime) ([docs.rs](https://docs.rs/rns-embedded-runtime)): runtime support for embedded Reticulum targets.
- [`rns-embedded-ffi`](https://crates.io/crates/rns-embedded-ffi) ![Crates.io Version](https://img.shields.io/crates/v/rns-embedded-ffi) ([docs.rs](https://docs.rs/rns-embedded-ffi)): FFI and static-library surface for embedded runtimes.
- [`rns-embedded-mininode`](https://crates.io/crates/rns-embedded-mininode) ![Crates.io Version](https://img.shields.io/crates/v/rns-embedded-mininode) ([docs.rs](https://docs.rs/rns-embedded-mininode)): minimal embedded Reticulum node helpers.

Command crates:

- [`lxmf-cli`](https://crates.io/crates/lxmf-cli) ![Crates.io Version](https://img.shields.io/crates/v/lxmf-cli): command-line LXMF client tools.
- [`reticulumd`](https://crates.io/crates/reticulumd) ![Crates.io Version](https://img.shields.io/crates/v/reticulumd): Reticulum daemon and interchange binaries.
- [`rns-tools`](https://crates.io/crates/rns-tools) ![Crates.io Version](https://img.shields.io/crates/v/rns-tools): Reticulum diagnostic and embedded tooling binaries.

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
cargo xtask package-daemon-bundle --version 0.9.8
cargo xtask api-diff
cargo xtask python-impl-bench-compare
cargo xtask python-impl-bench-compare --profile report
cargo xtask python-impl-bench-report
```

For fast local iteration on one binary, prefer narrow commands:

```bash
make check-bin PKG=lxmf-cli BIN=lxmd
make run-bin PKG=rns-tools BIN=rnsd ARGS="--help"
make package-daemon-bundle VERSION=0.9.8
make python-lxmd-smoke
```

## Binaries

- `lxmf-cli`
- `lxmd`
- `reticulumd`
- `lxm-interchange`
- `rnsd`, `rnstatus-rs`, `rnx`

Run examples:

```bash
cargo run -p lxmf-cli -- --help
cargo run -p reticulumd -- --help
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
```

## Documentation Entry Points

- Docs map: `docs/README.md`
- Practical examples: `docs/examples.md`
- Current status: `docs/status/current-roadmap.md`
- API surface and stability: `docs/lxmf-rs-api.md`
- CLI quick reference: `docs/lxmf-cli.md`
- Architecture overview: `docs/architecture/overview.md`
- JSON and wire-field mapping: `docs/architecture/json-lxmf-fields.md`
- Meshtastic tunnel interface: `docs/interfaces/meshtastic.md`
- Compatibility contract: `docs/contracts/compatibility-contract.md`
- Compatibility matrix: `docs/contracts/compatibility-matrix.md`
- Third-party compatibility kit: `docs/contracts/third-party-compatibility-kit.md`
- Support and LTS policy: `docs/contracts/support-policy.md`
- Extension registry: `docs/contracts/extension-registry.md`
- RPC contract: `docs/contracts/rpc-contract.md`
- Payload contract: `docs/contracts/payload-contract.md`
- Historical performance comparison archive: `docs/PerformancesComparison.html`; current generated results: [`docs/performance.md`](docs/performance.md)
- reticulumd operational deployment: `docs/runbooks/reticulumd-operational-deployment.md`
- Logging and diagnostics: `docs/runbooks/logging-and-diagnostics.md`
- crates.io publish plan: `docs/runbooks/crates-io-publish-plan.md`
- Release readiness: `docs/runbooks/release-readiness.md`

## crates.io Consumers

For library consumers, prefer the published package names rather than the
workspace directory names:

```toml
[dependencies]
lxmf = "0.9.8"
reticulum-rs = "0.9.8"
```

Or depend on the component crates directly:

```toml
[dependencies]
lxmf-sdk = "0.9.8"
reticulum-rs-rpc = "0.9.8"
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
xattr -dr com.apple.quarantine /path/to/lxmf-rs-tools-<version>-macos-arm64
chmod +x /path/to/lxmf-rs-tools-<version>-macos-arm64/lxmd
chmod +x /path/to/lxmf-rs-tools-<version>-macos-arm64/reticulumd
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

Release artifacts are published on the GitHub releases page:

[https://github.com/FreeTAKTeam/LXMF-rs/releases](https://github.com/FreeTAKTeam/LXMF-rs/releases)

For `v0.9.5`, use the release at:

[https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.9.5](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.9.5)

1. Open the release page and download the package and matching `.sha256` file
   for your platform.

2. Linux/macOS

```bash
sha256sum -c lxmf-rs-tools-v0.9.5-linux-x64.tar.gz.sha256
tar -xzf lxmf-rs-tools-v0.9.5-linux-x64.tar.gz

sha256sum -c lxmf-rs-tools-v0.9.5-macos-arm64.tar.gz.sha256
tar -xzf lxmf-rs-tools-v0.9.5-macos-arm64.tar.gz
```

3. Windows

```powershell
Get-FileHash .\lxmf-rs-tools-v0.9.5-windows-x64.zip -Algorithm SHA256
Get-Content .\lxmf-rs-tools-v0.9.5-windows-x64.zip.sha256
Expand-Archive .\lxmf-rs-tools-v0.9.5-windows-x64.zip .
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

<!-- performance-summary:start -->
## Measured performance

Release dataset: `v0.9.5` at `c4fd18761e41caf2f7d2c7307d49c37aa6dc43ca`; Python Reticulum `15320e4d2cfa` and LXMF `727830cefda8`.

| Matched workload | Rust p50 | Python p50 | Rust/Python |
|---|---:|---:|---:|
| LXMF message decode | 150 ns | 86.56 us | 576.59x |
| LXMF message encode | 260 ns | 36.27 us | 139.59x |
| LXMF large message decode | 250 ns | 92.94 us | 371.58x |
| LXMF large message encode | 500 ns | 43.43 us | 86.95x |

These are matched-workload comparisons, not a claim of whole-system superiority. See [methodology, complete results, variability, and limitations](docs/performance.md).
<!-- performance-summary:end -->

## License

Eclipse Public License 2.0 (`EPL-2.0`). See [LICENSE](LICENSE).
