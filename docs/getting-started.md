# Getting Started

This guide covers the shortest supported paths for installing LXMF-rs,
building a checkout, running the host tools, and adding the Rust libraries to
an application.

## Install a stable release

The [`v0.10.0` release](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.10.0)
is the prepared RNS 1.5.0-aligned release train. Once its immutable tag is
published, it provides Linux, macOS, and Windows archives, Debian and RPM
packages, a Windows MSI, SBOMs, and checksum files. Choose the asset for your
platform and verify it against `SHA256SUMS` before extracting or installing it.

For example, after downloading the Linux x86-64 archive and `SHA256SUMS` into
the same directory:

```bash
grep 'lxmf-rs_0.10.0_linux-x86_64.tar.gz$' SHA256SUMS | sha256sum -c -
tar -xzf lxmf-rs_0.10.0_linux-x86_64.tar.gz
```

The [latest release page](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest)
is the durable entry point for future versions. Release claims and evidence
boundaries are described in the [release notes](release-notes-v0.10.0.md) and
[current roadmap](status/current-roadmap.md).

## Build from source

The workspace requires Rust 1.85 or newer. The bootstrap helper checks the
Rust toolchains, installs the repository's Cargo tools, fetches locked
dependencies, and runs a formatting smoke check:

```bash
git clone https://github.com/FreeTAKTeam/LXMF-rs.git
cd LXMF-rs
make bootstrap
```

The direct script form is:

```bash
./tools/scripts/bootstrap-dev.sh
```

To inspect an existing environment without installing tools or running the
smoke check:

```bash
./tools/scripts/bootstrap-dev.sh --check --skip-smoke
```

Build the user-facing host tools with:

```bash
cargo build --release -p lxmf-cli -p reticulumd -p rns-tools
```

## Run locally

Start `reticulumd` on its default local Unix RPC endpoint:

```bash
cargo run -p reticulumd --bin reticulumd
```

Useful command entry points include:

```bash
cargo run -p lxmf-cli --bin lxmf -- --help
cargo run -p lxmf-cli --bin lxmd -- --help
cargo run -p rns-tools --bin rnsd -- --help
cargo run -p rns-tools --bin rnstatus-rs -- --help
cargo run -p rns-tools --bin rnx -- --help
```

For a working daemon-and-client flow, continue with the
[checked examples](examples.md). For production operation, use the
[`reticulumd` deployment runbook](runbooks/reticulumd-operational-deployment.md)
or the [`lxmd` systemd guide](runbooks/lxmd-systemd.md).

## Add the Rust libraries

The umbrella crates provide the simplest dependency surface:

```toml
[dependencies]
lxmf = "0.10.0"
reticulum-rs = "0.10.0"
```

Applications that need narrower features can depend on component crates:

```toml
[dependencies]
lxmf-sdk = "0.10.0"
lxmf-wire = "0.10.0"
reticulum-rs-core = "0.10.0"
reticulum-rs-transport = "0.10.0"
reticulum-rs-rpc = "0.10.0"
```

Use the [SDK quickstart](sdk/quickstart.md) for a minimal client, the
[SDK guide](sdk/README.md) for lifecycle and backend choices, and the
[workspace guide](project-layout.md) for package responsibilities.

## Validate a checkout

Start with the checks closest to your change:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --tests
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
tools/scripts/check-boundaries.sh
```

The repository also provides consolidated gates:

```bash
cargo xtask ci
cargo run -p xtask -- architecture-checks
cargo xtask release-check
```

Read the [contributor guide](../CONTRIBUTING.md) before submitting changes and
the [release-readiness runbook](runbooks/release-readiness.md) before treating
local results as release evidence.

## Next steps

- [SDK integration guide](sdk/README.md)
- [CLI quick reference](lxmf-cli.md)
- [Workspace and package guide](project-layout.md)
- [Current project status](status/current-roadmap.md)
- [Complete documentation map](README.md)
