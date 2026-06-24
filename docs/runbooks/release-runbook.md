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
3. Tag release with a signed git tag (`git tag -s`).
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

Publishing a GitHub Release now triggers `.github/workflows/crates-io-publish.yml`.
The workflow publishes the public library crates listed in
`docs/runbooks/crates-io-publish-plan.md` in dependency order.
The workflow rejects the release if any public crate version differs from the
GitHub release tag after removing the leading `v`.

Repository setup required before the first automated publish:

- add a `CARGO_REGISTRY_TOKEN` repository secret with permission to publish the
  project-owned crates
- optionally protect the `crates-io` GitHub Actions environment if releases
  should require manual approval before crates go live
