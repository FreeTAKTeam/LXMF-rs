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


## Our position on independent Reticulum implementations

Reticulum is a public-domain protocol whose authoritative specification is its Python reference implementation.

We respect Mark Qvist's original work and intellectual property rights. However, we reject the assertion that AI-assisted analysis or reimplementation automatically constitutes copyright infringement.

**No individual can claim exclusive control over the independent implementation of a public-domain protocol.** Compatibility should be established through reproducible interoperability tests and technical evidence, not personal endorsement.

**This project will continue its development**, committed to software quality, security, interoperability, and the freedom to innovate.

Read our [full statement on independent implementations and AI-assisted development](PROJECT_POSITION.md).

<!-- performance-summary:start -->
## Measured performance

Dataset: `main-2026-11-01` at `c59f35f62fb3196eeb391c943b840f0ad53ca2f3`; Python Reticulum `ea98db4f53dc` and LXMF `727830cefda8`.

| Matched workload | Rust p50 | Python p50 | Rust/Python |
|---|---:|---:|---:|
| LXMF message decode + verify | 33.00 us | 89.44 us | 2.71x |
| LXMF message encode + sign | 13.83 us | 37.22 us | 2.69x |
| LXMF large message decode + verify | 36.59 us | 94.59 us | 2.59x |
| LXMF large message encode + sign | 21.18 us | 45.26 us | 2.14x |

In the same-topology end-to-end comparison, Python was faster for: link setup, direct delivery, opportunistic delivery, resource delivery.

These are matched-workload comparisons, not a claim of whole-system superiority. See [methodology, complete results, variability, and limitations](docs/performance.md).
<!-- performance-summary:end -->


## Current status

The latest stable release is
[`v0.16.0`](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.16.0).
It aligns 21 public packages and includes delivery receipt, announce-cache,
ZeroMQ readiness and WAL maintenance fixes. Production resource and retention
acceptance remain open; see the release notes. Rust 1.88 or newer is required. A source checkout of
`main` can contain changes after the release; use the release tag to reproduce
its published artifacts.

The canonical release reference remains Python Reticulum 1.5.2 at
`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`, with Python LXMF 0.9.6 at
`727830cefda83d9c6e3982b48675425f3f988f9c`. The separate Reticulum 1.5.5
feature update has a [scoped delta ledger](docs/status/rns-1.5.5-delta.md);
it does not establish full forward behavioral parity.

| Area | Current position |
| --- | --- |
| Python callable inventory | 1,857 applicable entries complete, 0 partial, 0 unmapped, and 1 provenance-backed not-applicable entry |
| Reticulum software parity | 1,655 mapped callables complete against the pinned baseline; forward behavioral coverage remains separate |
| LXMF software parity | 202 mapped callables and all seven tracked software scenarios complete |
| Rust/Python interoperability | Direct, link, channel, paper, propagation, and daemon scenarios exercised against pinned references |
| Independent interoperability | Versioned rns-rs and supported-subset Reticulum-Go reports are attached to `v0.14.0`; known peer divergences remain explicit |
| Real-world interoperability | Bounded field evidence exists for REM on physical RNode/LoRa, two-phone TCP, Sideband phone HIL, Columba bidirectional direct LXMF, and live RCH paths; see [real-world interoperability evidence](docs/status/real-world-interoperability.md) |
| Remaining operational matrix | Permanent HIL, the full declared hardware/platform matrix, public-network rows, long soak, and current-release client reruns remain separate from the completed software-parity result |

Read the [current roadmap](docs/status/current-roadmap.md) for the authoritative
project posture, the [v0.16.0 release notes](docs/release-notes-v0.16.0.md),
the [v0.14.0 migration guide](docs/migrations/v0.14.0-zmq.md), and the
[Reticulum](docs/status/reticulum-parity-matrix.md) and
[LXMF](docs/status/lxmf-parity-matrix.md) parity matrices for row-level detail.

## Start here

| Goal | Documentation |
| --- | --- |
| Install, build, or run the tools | [Getting started](docs/getting-started.md) |
| Integrate the Rust SDK | [SDK guide](docs/sdk/README.md) and [quickstart](docs/sdk/quickstart.md) |
| Find the three main applications and their binaries | [Application and binary table](docs/project-layout.md#applications-and-binaries) |
| Understand the crates and dependency boundaries | [Workspace and package guide](docs/project-layout.md) |
| Deploy a daemon | [`lxmd` systemd guide](docs/runbooks/lxmd-systemd.md) or [`reticulumd` operations](docs/runbooks/reticulumd-operational-deployment.md) |
| Try the development MeshChat Rust POC | [MeshChat API runbook](docs/runbooks/meshchat-api.md) and [pinned application contract](docs/contracts/meshchat/README.md) |
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
lxmf = "0.14.0"
reticulum-rs = "0.14.0"
```

See [Getting started](docs/getting-started.md) for release downloads, checksum
verification, component crates, and additional run commands.

## Main packages

| Package group | Purpose | Links |
| --- | --- | --- |
| `lxmf`, `lxmf-sdk`, `lxmf-wire` | LXMF wire types and high-level client APIs | [crates.io](https://crates.io/crates/lxmf), [docs.rs](https://docs.rs/lxmf), [SDK guide](docs/sdk/README.md) |
| `reticulum-rs`, `reticulum-rs-core`, `reticulum-rs-transport`, `reticulum-rs-rpc` | Reticulum primitives, transport, interfaces, resources, and RPC | [crates.io](https://crates.io/crates/reticulum-rs), [docs.rs](https://docs.rs/reticulum-rs), [API overview](docs/lxmf-rs-api.md) |
| Main applications | `lxmf-cli` for LXMF client and daemon workflows; `reticulumd` for the Reticulum daemon and interchange service; `rns-tools` for Reticulum utilities | [Application and binary table](docs/project-layout.md#applications-and-binaries), [CLI reference](docs/lxmf-cli.md), [examples](docs/examples.md) |
| `meshchat-api` | Opt-in local MeshChat HTTP/WebSocket adapter for the development POC | [Runbook](docs/runbooks/meshchat-api.md) |
| Embedded crates | Constrained-device, managed-runtime, mini-node, and C ABI integration | [Package guide](docs/project-layout.md#embedded-libraries), [FFI guide](crates/libs/rns-embedded-ffi/README.md) |

Embedded portability varies by crate. `reticulum-rs-core` builds for bare-metal
`no_std` targets with `--no-default-features --features alloc`, verified in CI
against `thumbv7em-none-eabi` ([#676](https://github.com/FreeTAKTeam/LXMF-rs/issues/676));
the embedding application supplies its own RNG and clock. `lxmf-wire` still
requires the Rust standard library, even with `--no-default-features --features
alloc`; its `std` and `alloc` feature names do not guarantee bare-metal support.
Full `no_std` + `alloc` support for that path is deferred pending maintainer
capacity or contributor help ([#646](https://github.com/FreeTAKTeam/LXMF-rs/issues/646)). See the
[capability audit](docs/contracts/sdk-v2-feature-matrix.md#no_std--alloc-capability-audit)
for crate-specific limits.

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
- [v0.14.0 performance dashboard](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.14.0/lxmf-rs-performance.html) (superseded: measured before Rust and Python did matched work, so it overstates Rust)
- [Support and LTS policy](docs/contracts/support-policy.md)
- [Security policy](SECURITY.md)

## License

LXMF-rs is distributed under the [Eclipse Public License 2.0](LICENSE).
The same file contains an explicit [GPL version 2 or later secondary-license
notice](LICENSE#L280); availability under that secondary license follows the conditions in
the EPL and that notice. Cargo records the existing arrangement as
`EPL-2.0 OR GPL-2.0-or-later`; the license text and notice govern its terms.
Vendored components retain their own licenses and attribution, including
[zeromq's MIT notice](vendor/zeromq/LICENSE.md).
