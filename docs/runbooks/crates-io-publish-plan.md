# crates.io Publish Plan

This runbook defines the public crates.io packaging strategy for the workspace
after the repository refactor that split runtime, transport, RPC, embedded, and
application concerns into separate workspace crates.

## 1. Goals

- Keep GitHub releases as the binary distribution path.
- Publish only reusable library crates to crates.io.
- Preserve short Rust import ergonomics even when crates.io package names must
  change.
- Use owned umbrella crates for discoverability:
  - `lxmf`
  - `reticulum-rs`

## 2. Constraints

- crates.io uses a flat global namespace, not nested package namespaces.
- The names `lxmf-core` and `rns-core` are already owned by another publisher.
- The names `lxmf` and `reticulum-rs` are already owned by this project and are
  reserved for umbrella/facade crates.
- Published crates may not depend on path-only dependencies. Workspace-local
  dependencies must use `path + version`.

## 3. Naming Strategy

Use a two-tier public model:

- Umbrella crates:
  - `lxmf`
  - `reticulum-rs`
- Component crates:
  - `lxmf-reference`
  - `lxmf-wire`
  - `lxmf-sdk`
  - `reticulum-rs-core`
  - `reticulum-rs-transport`
  - `reticulum-rs-rpc`

The public package name does not need to match the local dependency alias or
the Rust import path. Preserve local ergonomics with dependency aliasing.

Example:

```toml
[dependencies]
lxmf-core = { package = "lxmf-wire", version = "0.2.0" }
rns-core = { package = "reticulum-rs-core", version = "0.2.0" }
rns-rpc = { package = "reticulum-rs-rpc", version = "0.3.0" }
```

For crates whose package names change, keep the Rust crate names stable with
`[lib] name = "..."` where needed so examples, doctests, and internal code do
not have to rewrite `use` paths just to complete the package rename.

## 4. Publish Matrix

### Wave 1: Core public surface

| Current workspace package | crates.io package | Rust crate name | Version target | Publish |
| --- | --- | --- | --- | --- |
| `lxmf-reference` | `lxmf-reference` | `lxmf_reference` | `0.1.0` | yes |
| `lxmf-core` | `lxmf-wire` | `lxmf_core` | `0.2.0` | yes |
| `lxmf-sdk` | `lxmf-sdk` | `lxmf_sdk` | `0.2.1` | yes |
| `rns-core` | `reticulum-rs-core` | `rns_core` | `0.2.0` | yes |
| `rns-transport` | `reticulum-rs-transport` | `rns_transport` | `0.2.0` | yes |
| `rns-rpc` | `reticulum-rs-rpc` | `rns_rpc` | `0.3.0` | yes |

### Wave 1.5: Facades after components exist

| New facade package | Role | Version target | Publish |
| --- | --- | --- | --- |
| `lxmf` | curated high-level facade over `lxmf-sdk` and selected wire types | `0.3.0` | yes |
| `reticulum-rs` | curated facade over core, with optional transport/RPC features | `0.2.0` | yes |

### Wave 2: Embedded family once support policy is explicit

| Current workspace package | crates.io package | Version target | Publish |
| --- | --- | --- | --- |
| `rns-embedded-core` | `reticulum-rs-embedded-core` | `0.2.0` | later |
| `rns-embedded-runtime` | `reticulum-rs-embedded-runtime` | `0.2.0` | later |
| `rns-embedded-ffi` | `reticulum-rs-embedded-ffi` | `0.2.0` | later |
| `rns-embedded-mininode` | `reticulum-rs-embedded-mininode` | `0.2.0` | later |

## 5. Do Not Publish

Keep these unpublished:

- `crates/apps/lxmf-cli`
- `crates/apps/reticulumd`
- `crates/apps/rns-tools`
- `crates/libs/test-support`
- `xtask`

These are distributed through GitHub releases, used only for local tooling, or
are not intended to carry a public support commitment.

Retired migration-era crates such as `crates/internal/*`, `lxmf-router`, and
`lxmf-runtime` are not part of the publish plan. If any of those names are
revived, they need a fresh support-policy decision before publication.

## 6. Versioning Policy

- Do not apply one blanket version to every existing published crate.
- New component package names may start at `0.2.0`.
- `lxmf-reference` starts at `0.1.0` and changes only when pinned compatibility
  metadata changes.
- `reticulum-rs-rpc` moved to `0.3.0` when the gRPC/protobuf surface was removed from the public crate.
- Already-published umbrella crates must continue forward monotonically from
  their crates.io history:
  - `lxmf`: next planned breaking line `0.3.0`
  - `reticulum-rs`: next planned breaking line `0.2.0`
- Keep Wave 1 component crates on a coordinated version line at first to reduce
  release-management overhead.

## 7. Required Manifest Work

For every published crate:

- add `description`
- add `readme`
- add `documentation`
- add `keywords`
- add `categories`
- verify `license`, `repository`, and `rust-version`
- trim packaging with `include` or `exclude` if the package would otherwise ship
  unnecessary fixtures or artifacts

For renamed packages:

- update `[package].name`
- add `[lib].name` when preserving the Rust crate name matters
- convert workspace dependencies to use `package = "published-name"` while
  keeping existing alias keys where that reduces churn

## 8. Workspace Changes Required

Primary files that must be updated together:

- `Cargo.toml`
- `xtask/Cargo.toml`
- `crates/libs/lxmf-core/Cargo.toml`
- `crates/libs/lxmf-sdk/Cargo.toml`
- `crates/libs/rns-core/Cargo.toml`
- `crates/libs/rns-transport/Cargo.toml`
- `crates/libs/rns-rpc/Cargo.toml`
- `crates/apps/reticulumd/Cargo.toml`
- `crates/apps/rns-tools/Cargo.toml`
- `crates/libs/rns-embedded-mininode/Cargo.toml`
- `crates/libs/rns-embedded-runtime/Cargo.toml`
- `crates/libs/rns-embedded-ffi/Cargo.toml`
- `crates/libs/test-support/Cargo.toml`

Supporting tooling and policy references that are package-name sensitive:

- `xtask/src/main.rs`
- `tools/scripts/check-boundaries.sh`
- `tools/scripts/backup-restore-drill.sh`
- `tools/scripts/embedded-footprint-check.sh`
- `.github/workflows/ci.yml`
- docs and runbooks that mention `cargo ... -p <package>`

## 9. Publish Order

Publish in dependency order, not with a blanket `cargo publish --workspace`.

Recommended order:

1. `lxmf-reference`
2. `reticulum-rs-core`
3. `lxmf-wire`
4. `reticulum-rs-transport`
5. `reticulum-rs-rpc`
6. `lxmf-sdk`
7. `reticulum-rs`
8. `lxmf`
Reason:

- `reticulum-rs-rpc` and `lxmf-sdk` share pinned compatibility metadata through
  `lxmf-reference`
- `lxmf-wire` depends on `reticulum-rs-core`
- `reticulum-rs-transport` depends on `reticulum-rs-core`
- `lxmf-sdk` depends on `reticulum-rs-rpc`
- facade crates should only publish after the underlying components are live

## 10. Pre-Publish Checklist

For each published crate:

```bash
cargo package --list --manifest-path <crate>/Cargo.toml
cargo publish --dry-run --manifest-path <crate>/Cargo.toml
```

For dependency-linked publish waves, only the first crate in the chain may be
able to complete `cargo publish --dry-run` before anything is live on
crates.io. Once a crate depends on a renamed package that is not yet published,
Cargo will resolve against the crates.io index and reject the downstream
dry-run. In that situation:

- use `cargo check --workspace --all-targets` to validate local path wiring
- use `cargo package --list` to verify packaged contents
- run `cargo publish --dry-run` for each downstream crate immediately after its
  upstream dependency has been published

Before the first publish wave:

```bash
cargo check --workspace --all-targets
cargo xtask release-check
```

If a crates.io publish wave ships alongside a daemon or product release:

- publish from the same commit or short-lived release branch used for the GitHub release
- list exact crate versions in the GitHub release notes
- keep migration notes and compatibility statements shared between the GitHub and crates.io release records

If the change is library-only, crates.io releases may ship without a new GitHub
bundle release.

Recommended follow-up automation:

- use `cargo xtask publish-crates --wave wave1 --dry-run --allow-dirty` for Wave 1 packaging validation
- use `cargo xtask publish-crates --wave all --dry-run --allow-dirty` to validate facades too
- use `cargo xtask yank-crate <package> <version>` if a bad crate needs to be yanked quickly
- add a docs check that the publish matrix in this file stays aligned with
  actual package names and versions

## 11. Migration Notes

- The package rename is mostly Cargo plumbing, CI/script references, and
  documentation maintenance. It is not expected to require a wide Rust source
  rewrite if alias keys and `[lib].name` values are preserved carefully.
- Umbrella crates should be curated and feature-gated facades, not blanket
  `pub use` dumps of every subcrate symbol.
- GitHub releases remain the supported binary delivery path even after crates.io
  publication is introduced for library consumers.
