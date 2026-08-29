# Release Runbook

## Preconditions
- CI is green on all required jobs from `.github/workflows/ci.yml`.
- Contract docs in `docs/contracts/` are updated.
- Breaking changes are documented in release notes and migration docs.

## Release Alignment

- GitHub releases are product and bundle releases.
- crates.io releases are library API releases that use the same version number
  as the GitHub release that publishes them.
- When binaries and libraries ship together, they must align on the same
  release train:
  - same commit or release branch
  - same version number
  - same changelog context
  - same compatibility and migration notes
- If a change is binary-only, cut only a GitHub release.
- If a change is library-only, crates.io releases may move without a new GitHub
  daemon bundle release, but use that library version as the next GitHub release
  number when a GitHub release is later created from the same train.

## Steps
1. Run local quality gates (`cargo xtask release-check`).
2. Run binary smoke tests (`cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20`).
3. Tag release with an annotated tag, signing it when a release key is configured (`git tag -a` or `git tag -s`).
4. Push tag and confirm release artifacts.

## Checklist
- [ ] Root `VERSION` bump committed
- [ ] All public crate package versions bumped to the release version
- [ ] Public workspace/path dependency versions bumped to match the crate package versions
- [ ] Changelog updated
- [ ] Signed tag created
- [ ] GitHub release notes list any crates.io versions shipped from the same release train
- [ ] Post-release smoke check completed

## crates.io Automation

Publishing a final GitHub Release triggers `.github/workflows/crates-io-publish.yml`.
Prereleases are intentionally skipped so RC tags can publish simulation and
bundle evidence without publishing crates. For a final release, the workflow
publishes the public library crates listed in
`docs/runbooks/crates-io-publish-plan.md` in dependency order.
The workflow rejects the release if any public crate version differs from the
GitHub release tag after removing the leading `v`.

After publication, verify that a `Publish crates.io` run exists for the exact
tag and commit. GitHub suppresses recursive workflow events created with the
default `GITHUB_TOKEN`; if the release event did not create a crates run,
dispatch the workflow manually with `ref=<tag>`, `version=<version>`, and
`dry_run=false`, then verify every package through the crates.io API. The
v0.10.1 train required this explicit dispatch as run
[33255408925](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33255408925).

Repository setup required before the first automated publish:

- add a `CARGO_REGISTRY_TOKEN` repository secret with permission to publish the
  project-owned crates
- optionally protect the `crates-io` GitHub Actions environment if releases
  should require manual approval before crates go live
