# Release Runbook

## Preconditions
- CI is green on all required jobs from `.github/workflows/ci.yml`.
- Contract docs in `docs/contracts/` are updated.
- Breaking changes are documented in release notes and migration docs.

## Release Alignment

- GitHub releases are product and bundle releases.
- crates.io releases are library API releases.
- When binaries and libraries ship together, they should align on the same
  release train:
  - same commit or release branch
  - same changelog context
  - same compatibility and migration notes
- They do not need to use identical version numbers.
- If a change is binary-only, cut only a GitHub release.
- If a change is library-only, crates.io releases may move without a new GitHub
  daemon bundle release.

## Steps
1. Run local quality gates (`cargo xtask release-check`).
2. Run binary smoke tests (`cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20`).
3. Tag release with a signed git tag (`git tag -s`).
4. Push tag and confirm release artifacts.

## Checklist
- [ ] Version bump committed
- [ ] Changelog updated
- [ ] Signed tag created
- [ ] GitHub release notes list any crates.io versions shipped from the same release train
- [ ] Post-release smoke check completed
