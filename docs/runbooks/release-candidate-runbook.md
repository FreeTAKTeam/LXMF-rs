# Release Candidate Runbook

This runbook defines the exact steps to cut and validate an RC.

## 1. Preconditions

- Working tree is clean except intended release changes.
- Workspace builds on stable and MSRV toolchains.

## 2. Local gates (must pass)

```bash
cargo xtask release-check
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
```

Optional longer soak:

```bash
./tools/scripts/soak-rnx.sh
# Example: CYCLES=5 BURST_ROUNDS=20 ./tools/scripts/soak-rnx.sh
# Example with explicit threshold + report artifact:
# CYCLES=5 BURST_ROUNDS=20 MAX_FAILURES=0 REPORT_PATH=target/soak/soak-report.json ./tools/scripts/soak-rnx.sh
```

## 3. CI gates (must pass)

- `CI / lint-format`
- `CI / build-matrix`
- `CI / test-nextest-unit`
- `CI / test-integration`
- `CI / doc`
- `CI / security`
- `CI / unused-deps`
- `CI / api-surface-check`

## 4. RC tagging

Use an RC tag format like `vX.Y.Z-rcN`.

```bash
git tag -a vX.Y.Z-rc1 -m "LXMF-rs vX.Y.Z-rc1"
git push origin vX.Y.Z-rc1
```

## 5. RC record

Record the following in release notes or a tracking issue:

- Commit SHA.
- CI run URL(s).
- Local gate command outputs.
- Known risks or deferred items.
