# Release Candidate Runbook

This runbook defines the exact steps to cut and validate an RC for `lxmf-rs`.

## 1. Preconditions

- Working tree is clean except intended release changes.
- Monorepo `crates/reticulum` builds.
- Python Reticulum source exists (default local path: `../Reticulum`).
- Sideband source clone exists (default local path: `../Sideband`).

## 2. Local gates (must pass)

```bash
make test
make interop-gate RETICULUM_PY_PATH=../Reticulum
cargo run --manifest-path crates/reticulum/Cargo.toml --bin rnx -- e2e --timeout-secs 20
make sideband-e2e RETICULUM_PY_PATH=../Reticulum SIDEBAND_PATH=../Sideband
```

Optional longer soak:

```bash
./scripts/soak-rnx.sh
# Example: CYCLES=5 BURST_ROUNDS=20 ./scripts/soak-rnx.sh
```

Single-command full local gate:

```bash
make production-gate-local RETICULUM_PY_PATH=../Reticulum SIDEBAND_PATH=../Sideband
```

## 3. CI gates (must pass)

- `CI / quality` (from `.github/workflows/ci.yml`)
- `CI / interop-python-live` (from `.github/workflows/ci.yml`)
- `Sideband E2E` workflow dispatch run (from `.github/workflows/sideband-e2e.yml`)

## 4. Manual interop validation (macOS + Sideband)

Perform at least one full bidirectional run against a real Sideband client:

1. Start `reticulumd` transport + RPC.
2. Start Sideband against the daemon transport interface.
3. Confirm announce/peer visibility.
4. Send daemon -> Sideband message and confirm receipt.
5. Send Sideband -> daemon message and confirm receipt.
6. Save logs/artifacts for the RC record.

Minimum acceptance:
- Announce exchange succeeds.
- Peer is listed.
- Bidirectional message delivery succeeds.

## 5. RC tagging

Use an RC tag format like `vX.Y.Z-rcN`.

```bash
git tag -a vX.Y.Z-rc1 -m "LXMF-rs vX.Y.Z-rc1"
git push origin vX.Y.Z-rc1
```

## 6. RC record

Record the following in release notes or tracking issue:

- Commit SHA.
- CI run URL(s).
- Local gate command outputs.
- Sideband manual validation artifacts/paths.
- Known risks or deferred items.
