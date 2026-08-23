# LXMF-rs — Reticulum and LXMF in Rust

[![Latest release](https://img.shields.io/github/v/release/FreeTAKTeam/LXMF-rs?sort=semver)](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest)
[![Crates.io](https://img.shields.io/crates/v/lxmf)](https://crates.io/crates/lxmf)
[![Documentation](https://docs.rs/lxmf/badge.svg)](https://docs.rs/lxmf)
[![License](https://img.shields.io/badge/license-EPL--2.0-blue.svg)](LICENSE)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/FreeTAKTeam/LXMF-rs)

LXMF-rs is a Rust implementation of the
[Reticulum](https://reticulum.network/) networking stack and the
[LXMF](https://github.com/markqvist/LXMF) messaging stack. The workspace
provides reusable libraries, a typed SDK, host daemons and command-line tools,
plus embedded and FFI surfaces.

<!-- performance-summary:start -->
## Measured performance

Release dataset: `v0.9.9` at `7199c4038a3ba786abb4dfbc95cbd6cd16ed9116`; Python Reticulum `b48b96e61676` and LXMF `727830cefda8`.

| Matched workload | Rust p50 | Python p50 | Rust/Python |
|---|---:|---:|---:|
| LXMF message decode | 285 ns | 8.33 ms | 29222.35x |
| LXMF message encode | 370 ns | 2.17 ms | 5864.16x |
| LXMF large message decode | 455 ns | 8.31 ms | 18252.45x |
| LXMF large message encode | 762 ns | 2.19 ms | 2869.52x |

These are matched-workload comparisons, not a claim of whole-system superiority. See [methodology, complete results, variability, and limitations](docs/performance.md).
<!-- performance-summary:end -->


## Current status

The latest stable release is
[`v0.9.9`](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.9.9),
published on 2026-08-12. The release-candidate workspace packages are version
`0.10.0`; current development targets Python Reticulum 1.5.0 at
`e32d4df754a7b87b1bf1bb0d08675d12ff505ae6` for the next release candidate.

| Area | Current position |
| --- | --- |
| Reticulum software parity | 1,838 applicable entries complete, 0 partial, 0 unmapped, and 1 provenance-backed not-applicable entry |
| LXMF software parity | All seven tracked software scenarios complete |
| Rust/Python interoperability | Direct, link, channel, paper, propagation, and daemon scenarios exercised against pinned references |
| Independent interoperability | Versioned rns-rs and Reticulum-Go evidence published separately |
| Hardware and external clients | Physical devices, public networks, and third-party clients remain separate evidence tracks and are not claimed by the software-parity result |

Read the [current roadmap](docs/status/current-roadmap.md) for the authoritative
project posture, the [v0.9.9 release notes](docs/release-notes-v0.9.9.md) for
the stable release summary, and the
[Reticulum](docs/status/reticulum-parity-matrix.md) and
[LXMF](docs/status/lxmf-parity-matrix.md) parity matrices for row-level detail.

## Start here

| Goal | Documentation |
| --- | --- |
| Install, build, or run the tools | [Getting started](docs/getting-started.md) |
| Integrate the Rust SDK | [SDK guide](docs/sdk/README.md) and [quickstart](docs/sdk/quickstart.md) |
| Understand the crates and binaries | [Workspace and package guide](docs/project-layout.md) |
| Deploy a daemon | [`lxmd` systemd guide](docs/runbooks/lxmd-systemd.md) or [`reticulumd` operations](docs/runbooks/reticulumd-operational-deployment.md) |
| Review compatibility claims | [Compatibility contract](docs/contracts/compatibility-contract.md) and [current roadmap](docs/status/current-roadmap.md) |
| Contribute | [Contributor guide](CONTRIBUTING.md) and [checked examples](docs/examples.md) |
| Browse all maintained documentation | [Documentation map](docs/README.md) |

## Quick start

Bootstrap a source checkout and run the local daemon:

```bash
make bootstrap
cargo run -p reticulumd --bin reticulumd
```

In another terminal, inspect the available tools:

```bash
cargo run -p lxmf-cli --bin lxmf -- --help
cargo run -p rns-tools --bin rnstatus-rs -- --help
```

Library consumers can start with the umbrella crates:

```toml
[dependencies]
lxmf = "0.9.9"
reticulum-rs = "0.9.9"
```

See [Getting started](docs/getting-started.md) for release downloads, checksum
verification, component crates, and additional run commands.

## Main packages

| Package group | Purpose | Links |
| --- | --- | --- |
| `lxmf`, `lxmf-sdk`, `lxmf-wire` | LXMF wire types and high-level client APIs | [crates.io](https://crates.io/crates/lxmf), [docs.rs](https://docs.rs/lxmf), [SDK guide](docs/sdk/README.md) |
| `reticulum-rs`, `reticulum-rs-core`, `reticulum-rs-transport`, `reticulum-rs-rpc` | Reticulum primitives, transport, interfaces, resources, and RPC | [crates.io](https://crates.io/crates/reticulum-rs), [docs.rs](https://docs.rs/reticulum-rs), [API overview](docs/lxmf-rs-api.md) |
| `lxmf-cli`, `reticulumd`, `rns-tools` | LXMF, daemon, diagnostic, and operator binaries | [CLI reference](docs/lxmf-cli.md), [examples](docs/examples.md) |
| Embedded crates | `no_std`, managed runtime, mini-node, and C ABI integration | [Package guide](docs/project-layout.md#embedded-libraries), [FFI guide](crates/libs/rns-embedded-ffi/README.md) |

The complete workspace inventory and dependency-boundary rules live in the
[workspace and package guide](docs/project-layout.md). The root
[`Cargo.toml`](Cargo.toml) remains the source of truth for active members.

## Validation

Use focused checks while developing and broaden them as the change requires:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo test --workspace --tests
tools/scripts/check-boundaries.sh
```

Release-facing changes should also pass:

```bash
cargo run -p xtask -- architecture-checks
cargo xtask release-check
```

See the [release-readiness runbook](docs/runbooks/release-readiness.md) for the
full gate and evidence boundaries.

## Evidence and project policy

- [Current roadmap and parity posture](docs/status/current-roadmap.md)
- [Reticulum parity matrix](docs/status/reticulum-parity-matrix.md)
- [LXMF parity matrix](docs/status/lxmf-parity-matrix.md)
- [Independent implementation evidence](docs/interop/README.md)
- [Performance methodology and results](docs/performance.md)
- [Support and LTS policy](docs/contracts/support-policy.md)
- [Security policy](SECURITY.md)

## License

LXMF-rs is licensed under the [Eclipse Public License 2.0](LICENSE).
