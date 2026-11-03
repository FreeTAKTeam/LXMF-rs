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


## Linux-only prereleases

Use the existing Release workflow with a valid SemVer tag ending in `-linux`,
for example `v0.16.1-rc.1-linux`. Commit the matching `VERSION`, public package
versions, dependency requirements and lockfiles before tagging. The prepare step
resolves the literal existing tag and checks `VERSION` from that immutable source;
manual rebuilds cannot silently package a different checkout. Build metadata
(`+suffix`) is not supported because the same version labels the OCI image.

`tools/scripts/release_config.py` owns the build inventory and tag classification.
Linux-only tags select x86-64, AArch64 and ARMv7 musl builds and their deb/rpm
packages. Windows and macOS packaging are skipped; other tags retain all six
build targets. SDK wrapper parity gates the builds, and publication requires
successful builds, Linux packages, SBOMs and container checks. Only the two
intentional platform skips are accepted by the explicit publication guard.

Every accepted SemVer prerelease is published with `prerelease: true` and
`make_latest: false`, including when the Linux performance or independent-evidence
publisher finishes first. These auxiliary publishers retain their validation;
independent nightly publishing still uploads workflow artifacts without creating
a release. Prereleases do not update the OCI `latest` tag or Homebrew, and the
crates.io workflow excludes them. Linux OCI version images, SBOMs, signed
checksums and provenance retain the existing publication path.

Asset names retain the release version, including its prerelease suffix.
`packaging/nfpm.yaml` uses `version_schema: semver` for distro metadata: nfpm
normalizes `0.16.1-rc.1-linux` to Debian `0.16.1~rc.1-linux` and RPM
`0.16.1~rc.1_linux` (release `1`). Download and verify `SHA256SUMS.txt` alongside
its cosign bundle; `release-build-source.json` records the product commit,
packaging workflow commit and OCI digest. Record the exact hosted run and
post-download smoke results. Software and packaging checks do not establish
production resource stability, device HIL or long-duration delivery acceptance.
