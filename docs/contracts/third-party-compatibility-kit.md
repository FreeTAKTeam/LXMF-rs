# Third-Party Compatibility Test Kit

## Purpose
This kit provides a stable, versioned contract + fixture pack for external clients (Sideband, RCH, Columba, and other SDK/RPC consumers) to validate protocol compatibility without coupling to in-repo Rust internals.

## Kit Contents
- `contracts/compatibility-contract.md`
- `contracts/compatibility-matrix.md`
- `contracts/rpc-contract.md`
- `contracts/payload-contract.md`
- `contracts/interop-artifacts-manifest.json`
- `fixtures/golden-corpus.json`

## Build Modes
- Dry-run policy validation:
  - `tools/scripts/compatibility-kit.sh --dry-run`
- Full artifact build:
  - `tools/scripts/compatibility-kit.sh --build`

Default output directory:
- `target/compat-kit`

Override output directory:
- `COMPAT_KIT_OUT_DIR=/path/to/out tools/scripts/compatibility-kit.sh --build`

## Required Validation Gates
The build mode executes these gates before publishing artifacts:
1. `cargo run -p xtask -- interop-artifacts`
2. `cargo run -p xtask -- interop-matrix-check`
3. `cargo run -p xtask -- sdk-schema-check`
4. `cargo run -p xtask -- sdk-conformance`
5. `cargo run -p xtask -- e2e-compatibility`

## External Consumer Workflow
1. Pull the generated kit from CI artifacts or local build output.
2. Run schema validation against your client payloads using `contracts/interop-artifacts-manifest.json`.
3. Replay golden corpus vectors in `fixtures/golden-corpus.json`.
4. Produce a report mapping pass/fail results to `compatibility-matrix.md` slices.
5. Open a contract issue if any `required` slice fails.
