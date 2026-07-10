# crates.io Publish Plan

This runbook defines the public crates.io packaging strategy for the workspace
after the repository refactor that split runtime, transport, RPC, embedded, and
application concerns into separate workspace crates.

## 1. Goals

- Keep GitHub releases as the primary binary bundle distribution path.
- Publish public library crates and installable command crates to crates.io.
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
  - `lxmf-embedded-mini`
  - `rns-embedded-core`
  - `rns-embedded-runtime`
  - `rns-embedded-ffi`
  - `rns-embedded-mininode`
- Command crates:
  - `lxmf-cli`
  - `reticulumd`
  - `rns-tools`

The public package name does not need to match the local dependency alias or
the Rust import path. Preserve local ergonomics with dependency aliasing.

Example:

```toml
[dependencies]
lxmf-core = { package = "lxmf-wire", version = "<release-version>" }
rns-core = { package = "reticulum-rs-core", version = "<release-version>" }
rns-rpc = { package = "reticulum-rs-rpc", version = "<release-version>" }
```

For crates whose package names change, keep the Rust crate names stable with
`[lib] name = "..."` where needed so examples, doctests, and internal code do
not have to rewrite `use` paths just to complete the package rename.

## 4. Publish Matrix

### Wave 1: Core public surface

| Current workspace package | crates.io package | Rust crate name | Version target | Publish |
| --- | --- | --- | --- | --- |
| `lxmf-reference` | `lxmf-reference` | `lxmf_reference` | GitHub release version | yes |
| `lxmf-core` | `lxmf-wire` | `lxmf_core` | GitHub release version | yes |
| `lxmf-sdk` | `lxmf-sdk` | `lxmf_sdk` | GitHub release version | yes |
| `lxmf-runtime` | `lxmf-runtime` | `lxmf_runtime` | GitHub release version | yes |
| `rns-core` | `reticulum-rs-core` | `rns_core` | GitHub release version | yes |
| `rns-transport` | `reticulum-rs-transport` | `rns_transport` | GitHub release version | yes |
| `rns-rpc` | `reticulum-rs-rpc` | `rns_rpc` | GitHub release version | yes |

### Wave 1.5: Facades after components exist

| New facade package | Role | Version target | Publish |
| --- | --- | --- | --- |
| `lxmf` | curated high-level facade over `lxmf-sdk` and selected wire types | GitHub release version | yes |
| `reticulum-rs` | curated facade over core, with optional transport/RPC features | GitHub release version | yes |

### Wave 2: Embedded family

| Current workspace package | crates.io package | Version target | Publish |
| --- | --- | --- | --- |
| `lxmf-embedded-mini` | `lxmf-embedded-mini` | GitHub release version | yes |
| `rns-embedded-core` | `rns-embedded-core` | GitHub release version | yes |
| `rns-embedded-runtime` | `rns-embedded-runtime` | GitHub release version | yes |
| `rns-embedded-ffi` | `rns-embedded-ffi` | GitHub release version | yes |
| `rns-embedded-mininode` | `rns-embedded-mininode` | GitHub release version | yes |

### Wave 3: Command crates

| Current workspace package | crates.io package | Version target | Publish |
| --- | --- | --- | --- |
| `lxmf-cli` | `lxmf-cli` | GitHub release version | yes |
| `reticulumd` | `reticulumd` | GitHub release version | yes |
| `rns-tools` | `rns-tools` | GitHub release version | yes |

## 5. Do Not Publish

Keep these unpublished:

- `crates/libs/test-support`
- `xtask`

These are used only for local tooling, test support, or release engineering and
are not intended to carry a public support commitment.

Retired migration-era crates such as `crates/internal/*` and `lxmf-router` are
not part of the publish plan. If any of those names are
revived, they need a fresh support-policy decision before publication.

## 6. Versioning Policy

- Public crates use the same version number as the GitHub release that
  publishes them.
- Before creating a GitHub release, bump every public crate listed in this
  runbook to the release version.
- Also bump the root `[workspace.dependencies]` versions for those crates so
  local path dependencies and published dependency metadata stay aligned.
- The automated crates.io workflow rejects a release when any public crate
  version differs from the GitHub release tag after removing the leading `v`.
- Existing crates.io histories remain valid, but future releases move all public
  crates forward together on the GitHub release version line.

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
- `crates/libs/lxmf-embedded-mini/Cargo.toml`
- `crates/libs/rns-embedded-core/Cargo.toml`
- `crates/libs/rns-embedded-mininode/Cargo.toml`
- `crates/libs/rns-embedded-runtime/Cargo.toml`
- `crates/libs/rns-embedded-ffi/Cargo.toml`
- `crates/libs/lxmf/Cargo.toml`
- `crates/libs/reticulum-rs/Cargo.toml`
- `crates/apps/lxmf-cli/Cargo.toml`
- `crates/apps/reticulumd/Cargo.toml`
- `crates/apps/rns-tools/Cargo.toml`
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
4. `rns-embedded-core`
5. `rns-embedded-runtime`
6. `rns-embedded-ffi`
7. `reticulum-rs-transport`
8. `reticulum-rs-rpc`
9. `lxmf-sdk`
10. `lxmf-runtime`
11. `rns-embedded-mininode`
12. `lxmf-embedded-mini`
13. `reticulum-rs`
14. `lxmf`
15. `lxmf-cli`
16. `reticulumd`
17. `rns-tools`
Reason:

- `reticulum-rs-rpc` and `lxmf-sdk` share pinned compatibility metadata through
  `lxmf-reference`
- `lxmf-wire` depends on `reticulum-rs-core`
- `rns-embedded-runtime` depends on `rns-embedded-core`
- `rns-embedded-ffi` depends on `rns-embedded-core` and `rns-embedded-runtime`
- `reticulum-rs-transport` depends on `reticulum-rs-core`
- `lxmf-sdk` depends on `reticulum-rs-rpc`
- `lxmf-runtime` depends on `lxmf-sdk`, `lxmf-wire`, and `reticulum-rs-transport`
- `rns-embedded-mininode` depends on `lxmf-wire` and `reticulum-rs-core`
- facade crates should only publish after the underlying components are live
- command crates publish last after their library dependencies are live

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
- use the same version number as the GitHub release tag
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
