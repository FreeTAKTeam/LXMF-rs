# Release Runbook

## Preconditions
- CI green on default and compatibility jobs.
- `docs/compatibility-contract.md` updated.
- Breaking changes called out in release notes.

## Steps
1. Run local quality gates (`fmt`, `clippy`, `test`, `doc`, `deny`, `audit`).
2. Verify cross-repo compatibility against pinned revision.
3. Tag release with signed git tag (`git tag -s`).
4. Push tag and confirm release artifacts.

## Checklist
- [ ] Version bump committed
- [ ] Changelog updated
- [ ] Signed tag created
- [ ] Post-release smoke check completed
