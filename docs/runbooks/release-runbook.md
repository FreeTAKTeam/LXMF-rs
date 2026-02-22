# Release Runbook

## Preconditions
- CI is green on all required jobs from `.github/workflows/ci.yml`.
- Contract docs in `docs/contracts/` are updated.
- Breaking changes are documented in release notes and migration docs.

## Steps
1. Run local quality gates (`cargo xtask release-check`).
2. Run binary smoke tests (`cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20`).
3. Tag release with a signed git tag (`git tag -s`).
4. Push tag and confirm release artifacts.

## Checklist
- [ ] Version bump committed
- [ ] Changelog updated
- [ ] Signed tag created
- [ ] Post-release smoke check completed
