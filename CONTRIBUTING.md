# Contributing to LXMF-rs

This repository has a large documentation surface because some docs are also
contract inputs, schema fixtures, or release gates. Before deleting or moving
anything under `docs/`, check `docs/README.md` so you do not remove a file that
CI or tests depend on.

## Start Here

1. Read `README.md` for the workspace overview.
2. Read `docs/README.md` for the docs map and retention rules.
3. Run `make bootstrap` to install the expected Rust toolchain and local tools.

## Workspace Shape

Active workspace members are defined in `Cargo.toml`.

The [workspace and package guide](docs/project-layout.md) maps Cargo package
names to source directories, binaries, and responsibilities. In particular,
`lxmf-wire` is in `crates/libs/lxmf-core`; `lxmf-runtime` is an active in-process
SDK backend. `test-support` and `xtask` are internal, unpublished packages.
The separately published `reticulum-rs-zeromq` transport is vendored under
`vendor/zeromq` and excluded from workspace membership.

Before adding a local dependency edge, check
`[workspace.metadata.boundaries]` and run `tools/scripts/check-boundaries.sh`.

## Bootstrap

Use Rust 1.88 or newer. The checkout selects the stable toolchain; the build
matrix also checks the declared minimum. The Bash bootstrap script requires
`rustup`, Cargo, and the native build tools needed by the selected targets.

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

## Common Local Commands

Fast confidence:

```bash
cargo check --workspace --all-targets
cargo test --workspace
./tools/scripts/check-boundaries.sh
```

Full local gate:

```bash
cargo xtask ci
```

Release-oriented gate:

```bash
cargo xtask release-check
```

Target one binary when you do not need the whole workspace:

```bash
make check-bin PKG=lxmf-cli BIN=lxmf-cli
make run-bin PKG=rns-tools BIN=rnx ARGS="--help"
```

## Documentation Rules

- Prefer updating an existing source-of-truth doc over adding a parallel note.
- Use relative repository paths in docs and examples. Do not commit machine-
  specific absolute paths.
- If a new doc supersedes an old one, delete the old doc in the same PR after
  fixing inbound links.
- Treat `docs/contracts/`, `docs/schemas/`, and `docs/fixtures/` as code-adjacent
  artifacts. Many are validated by CI.
- Keep `README.md` short and contributor-oriented. Deep operator guidance should
  live in `docs/runbooks/` or package-specific `README.md` files.
- Separate installation/tutorials, operator procedures, API contracts, and
  architecture explanations through the existing [documentation map](docs/README.md).
- Check package names, feature flags, CLI options, and examples against the
  current manifests and source. Declare prerequisites and use `--locked` for
  reproducible source builds.
- Keep historical release notes, evidence, reference pins, and generated
  benchmark tables tied to their original versions. Regenerate derived
  documentation with its owning tool.
- Separate project versions, SDK contract versions, and Python reference
  versions; a newer package version does not establish new parity evidence.

For documentation changes, check local paths and heading links, then run:

```bash
cargo xtask sdk-docs-check
python3 tools/scripts/performance_docs.py --check
cargo doc --locked --workspace --no-deps --lib
```

When changing Rust documentation examples, also run the affected package's
`cargo test --doc` checks. See the [Cargo documentation command](https://doc.rust-lang.org/cargo/commands/cargo-doc.html)
and [rustdoc example rules](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html).

## License metadata and packaged notices

Keep the existing [license and secondary-license notice](LICENSE) intact.
Publishable first-party packages inherit `license.workspace = true` and
`license-file.workspace = true`. The inherited file is included in each Cargo
package. The retained upstream MIT grant for `vendor/zeromq` is documented in
`[workspace.metadata.license-check.exceptions]`; do not replace it with the
workspace expression.

```bash
python3 -m unittest discover -s tools/scripts -p test_license_metadata.py -v
python3 tools/scripts/check_license_metadata.py
cargo xtask licenses
```

The deterministic check discovers publishable local packages from Cargo
metadata, resolves inheritance, and checks Cargo's actual package file lists.
Intentional exceptions require an SPDX expression, retained notice paths and
a reason in workspace metadata. These checks run in CI, the existing security
and release gates, and the crates.io publication preflight. They check
consistency and distributable contents; they do not determine copyright
ownership or legal compatibility.

## When Docs Must Change

Update docs when you change:

- public SDK or RPC behavior
- schema or fixture shape
- contributor bootstrap or validation commands
- operator workflows that are described in `docs/runbooks/`
- support, migration, or compatibility guarantees

Useful entry points:

- `docs/README.md`
- `docs/sdk/README.md`
- `docs/runbooks/release-readiness.md`
- `docs/contracts/support-policy.md`

## Pull Requests

Before opening a PR, run the narrowest validation that proves your change.
For contract, schema, or contributor-workflow changes, prefer `cargo xtask ci`
or the specific `xtask` check that covers the affected area.

If you touch docs:

- remove or update stale links
- keep docs consistent with the actual workspace layout
- call out any intentionally retained historical docs in the PR summary

## Security

Do not open public issues for suspected vulnerabilities. Follow
`SECURITY.md` and `.github/SECURITY.md`.
