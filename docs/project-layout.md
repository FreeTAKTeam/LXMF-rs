# Workspace and Package Guide

LXMF-rs is a Cargo workspace with explicit library, application, embedded, and
tooling boundaries. The root [`Cargo.toml`](../Cargo.toml) is the source of
truth for active members and allowed workspace-local dependency edges.

## Directory layout

```text
LXMF-rs/
├── crates/
│   ├── libs/          reusable protocol, SDK, runtime, and embedded crates
│   └── apps/          daemons and command-line applications
├── docs/              maintained guides, contracts, status, and evidence
├── tests/hil/         repository-owned interoperability scenarios
├── tools/             validation, parity, benchmark, and release scripts
└── xtask/             Cargo-native project automation
```

## LXMF libraries

| Package | Workspace path | Responsibility |
| --- | --- | --- |
| [`lxmf`](https://crates.io/crates/lxmf) | `crates/libs/lxmf` | Umbrella crate for the SDK and wire types |
| [`lxmf-sdk`](https://crates.io/crates/lxmf-sdk) | `crates/libs/lxmf-sdk` | High-level client APIs and backend contracts |
| [`lxmf-wire`](https://crates.io/crates/lxmf-wire) | `crates/libs/lxmf-core` | LXMF messages, payloads, identities, and wire encoding |
| `lxmf-runtime` | `crates/libs/lxmf-runtime` | In-process SDK backend over the Reticulum transport runtime |
| `meshchat-api` | `crates/libs/meshchat-api` | Opt-in local MeshChat HTTP/WebSocket adapter used by `reticulumd` for the development POC; see the [runbook](runbooks/meshchat-api.md) |
| `lxmf-reference` | `crates/libs/lxmf-reference` | Pinned reference metadata used by compatibility gates |

Start application integration with the [SDK guide](sdk/README.md).

## Reticulum libraries

| Package | Workspace path | Responsibility |
| --- | --- | --- |
| [`reticulum-rs`](https://crates.io/crates/reticulum-rs) | `crates/libs/reticulum-rs` | Umbrella crate for the Reticulum stack |
| [`reticulum-rs-core`](https://crates.io/crates/reticulum-rs-core) | `crates/libs/rns-core` | Packet, identity, ratchet, and cryptographic primitives |
| [`reticulum-rs-transport`](https://crates.io/crates/reticulum-rs-transport) | `crates/libs/rns-transport` | Transport, interface, link, receipt, and resource behavior |
| [`reticulum-rs-rpc`](https://crates.io/crates/reticulum-rs-rpc) | `crates/libs/rns-rpc` | RPC, HTTP, event, and daemon bridge contracts |
| `test-support` | `crates/libs/test-support` | Shared fixtures, schema validation, and integration helpers |

## Embedded libraries

| Package | Workspace path | Responsibility |
| --- | --- | --- |
| [`lxmf-embedded-mini`](https://crates.io/crates/lxmf-embedded-mini) | `crates/libs/lxmf-embedded-mini` | No-allocation LXMF runtime for constrained targets |
| [`rns-embedded-core`](https://crates.io/crates/rns-embedded-core) | `crates/libs/rns-embedded-core` | Embedded-friendly Reticulum primitives and shared types |
| [`rns-embedded-runtime`](https://crates.io/crates/rns-embedded-runtime) | `crates/libs/rns-embedded-runtime` | Managed and manual-tick runtime support |
| [`rns-embedded-ffi`](https://crates.io/crates/rns-embedded-ffi) | `crates/libs/rns-embedded-ffi` | C ABI and static-library surface; see the [FFI guide](../crates/libs/rns-embedded-ffi/README.md) |
| [`rns-embedded-mininode`](https://crates.io/crates/rns-embedded-mininode) | `crates/libs/rns-embedded-mininode` | Minimal Reticulum node helpers |

Embedded implementation status and physical-device evidence are distinct. See
the [current roadmap](status/current-roadmap.md) before making hardware-support
claims.

## Applications and binaries

| Package | Main binaries | Purpose |
| --- | --- | --- |
| [`lxmf-cli`](https://crates.io/crates/lxmf-cli) | `lxmf`, `lxmf-cli`, `lxmd` | LXMF client and daemon workflows |
| [`reticulumd`](https://crates.io/crates/reticulumd) | `reticulumd`, `lxm-interchange` | Reticulum daemon and interchange service |
| [`rns-tools`](https://crates.io/crates/rns-tools) | `rnsd`, `rnstatus-rs`, `rnx`, `rnpath-rs`, `rngit`, and related utilities | Reticulum operation, diagnostics, repository service, and embedded tooling |

Run examples are collected in [Getting started](getting-started.md) and the
[CLI reference](lxmf-cli.md).

## Automation and evidence

- [`xtask`](../xtask) owns Cargo-native CI, architecture, release, HIL, and
  packaging commands.
- [`tools/scripts`](../tools/scripts) contains the maintained parity,
  interoperability, benchmark, validation, and release helpers.
- [`tests/hil`](../tests/hil) contains repository-owned scenario definitions.
- [`docs/status`](status) contains the current roadmap and parity records.
- [`docs/interop`](interop/README.md) and
  [`docs/performance.md`](performance.md) publish evidence separately from
  implementation status.

Prefer these maintained entry points over new ad hoc scripts.

## Dependency boundaries

Library crates cannot depend on application crates. Workspace-local edges are
an allowlist in [`Cargo.toml`](../Cargo.toml) under
`[workspace.metadata.boundaries]` and are checked with:

```bash
tools/scripts/check-boundaries.sh
```

The broader layering rules and security architecture are documented in the
[architecture overview](architecture/overview.md).
