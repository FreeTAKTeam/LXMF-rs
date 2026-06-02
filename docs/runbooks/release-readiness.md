# Release Readiness Checklist

This checklist is the publication gate for the Rust workspace.
It must reflect the checks and status sources that are actually enforced on the
active branch.

## 1. Parity truth

- Repository-wide status is tracked first in `docs/status/current-roadmap.md`.
- `docs/plans/lxmf-parity-matrix.md` and `docs/plans/reticulum-parity-matrix.md`
  are historical parity snapshots, not the primary release gate.
- If a parity matrix disagrees with `docs/status/current-roadmap.md`, treat the
  matrix as stale until it is refreshed in the same change.
- Rust/Python live interop is enforced by `.github/workflows/python-interop.yml`
  on pull requests for the pinned Python Reticulum/LXMF references. Do not mark
  parity complete until non-ignored evidence exists for the specific matrix row.

## 2. Contract and schema gates

- RPC contract remains aligned with `docs/contracts/rpc-contract.md`.
- Payload contract remains aligned with `docs/contracts/payload-contract.md`.
- Contract v2 schema artifacts remain valid:
  - `docs/schemas/contract-v2/payload-envelope.schema.json`
  - `docs/schemas/contract-v2/event-payload.schema.json`
- RPC contract checks in this repo and external interop gate are kept aligned with `docs/contracts/rpc-contract.md` and migration evidence.

## 3. API stability gates

- Public API surface checks pass for:
  - `lxmf-wire`
  - `lxmf-sdk`
  - `reticulum-rs-core`
  - `reticulum-rs-transport`
  - `reticulum-rs-rpc`
- Breaking changes are called out in migration docs under `docs/migrations/`.

## 4. CI quality gates

Current GitHub PR CI in `.github/workflows/ci.yml` enforces these jobs:

- `quality`
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`
  - `cargo check --workspace --all-targets`
- `tests`
  - `cargo nextest run --workspace --lib --bins`
  - `cargo test --workspace --tests`
- `contracts`
  - `cargo xtask ci --stage sdk-schema-check`
  - `cargo xtask publish-crates --wave all --dry-run --allow-dirty`
  - `cargo check -p reticulumd -p rns-tools`
  - `bash tools/scripts/check-boundaries.sh`
  - `cargo run -p xtask -- architecture-checks`
  - `cargo run -p xtask -- sdk-docs-check`
  - `cargo run -p xtask -- sdk-migration-check`
- `security`
  - `cargo deny check bans licenses sources`
  - `cargo audit --ignore RUSTSEC-2024-0421 --ignore RUSTSEC-2024-0436 --ignore RUSTSEC-2026-0009 --ignore RUSTSEC-2025-0134`

`.github/workflows/python-interop.yml` is also a pull-request gate for pinned
reference compatibility. It runs:

- Python reference conformance baseline against pinned Reticulum/LXMF commits.
- `cargo xtask ci --stage interop-artifacts`
- `cargo xtask ci --stage sdk-conformance`
- `cargo xtask ci --stage e2e-compatibility`
- ignored live Rust/Python channel, paper, compatibility-matrix, and LXMD
  remote-relay interop tests with the pinned checkouts.

The SDK reports the parity checkpoint as its crate version plus the pinned
reference revisions from `.github/workflows/python-interop.yml`: Reticulum
conformance `0319444b20e0815f26c6b9ceeba8fa44de037c9b`, Python Reticulum
`15320e4d2cfabb143c1db20ca887e275fd521585`, and Python LXMF
`727830cefda83d9c6e3982b48675425f3f988f9c`. The latest GitHub metadata checked
on 2026-05-29 showed the newest `Python reference interop` run failing on
`switch_to_tracing`, with the latest successful run on 2026-05-28 for
`udp-multicast`; do not encode transient run IDs in runtime SDK responses.

The commands below remain useful release checks, but they are not currently
enforced by pull-request CI unless and until `.github/workflows/ci.yml` is
expanded.

## 5. Local release checks

Current high-signal local checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo check --workspace --all-targets
cargo nextest run --workspace --lib --bins
cargo test --workspace --tests
cargo xtask ci --stage sdk-schema-check
cargo xtask publish-crates --wave all --dry-run --allow-dirty
bash tools/scripts/check-boundaries.sh
cargo run -p xtask -- architecture-checks
cargo run -p xtask -- sdk-docs-check
cargo run -p xtask -- sdk-migration-check
```

ZeroMQ transport readiness checks before considering a default switch:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo doc --workspace --no-deps
bash tools/scripts/check-boundaries.sh
cargo run -p rns-tools --bin rnx -- replay --trace docs/fixtures/sdk-v2/rpc/replay_known_send_cancel.v1.json
```

Additional required evidence:

- local TCP SDK plus daemon integration covers start, send, cancel, status, configure, poll events,
  snapshot, and shutdown
- multi-client tests prove no cross-session response delivery
- queue pressure, restart, reconnect, no-peer, oversized-frame, and sustained-event stress cases
  map to documented SDK errors
- remote ZeroMQ endpoints fail closed without token auth

Extended/manual release checks:

```bash
cargo xtask release-check
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
cargo run -p rns-tools --bin rnx -- replay --trace docs/fixtures/sdk-v2/rpc/replay_known_send_cancel.v1.json
cargo run -p xtask -- sdk-bench-check
cargo run -p xtask -- sdk-memory-budget-check
cargo run -p xtask -- embedded-footprint-check
cargo run -p xtask -- sdk-queue-pressure-check
cargo run -p xtask -- security-review-check
cargo run -p xtask -- sdk-fuzz-check
cargo run -p xtask -- sdk-metrics-check
cargo run -p xtask -- crypto-agility-check
cargo run -p xtask -- key-management-check
cargo run -p xtask -- sdk-docs-check
cargo run -p xtask -- sdk-cookbook-check
cargo run -p xtask -- sdk-ergonomics-check
cargo run -p xtask -- sdk-incident-runbook-check
cargo run -p xtask -- sdk-drill-check
cargo run -p xtask -- sdk-soak-check
cargo run -p xtask -- lxmf-cli-check
cargo run -p xtask -- reference-integration-check
cargo run -p xtask -- schema-client-check
cargo run -p xtask -- dx-bootstrap-check
cargo run -p xtask -- compat-kit-check
cargo run -p xtask -- compliance-profile-check
cargo run -p xtask -- support-policy-check
cargo run -p xtask -- unsafe-audit-check
cargo run -p xtask -- release-scorecard-check
cargo run -p xtask -- canary-criteria-check
cargo run -p xtask -- extension-registry-check
cargo run -p xtask -- plugin-negotiation-check
cargo run -p xtask -- certification-report-check
cargo run -p xtask -- architecture-lint-check
cargo run -p xtask -- architecture-checks
cargo run -p xtask -- changelog-migration-check
cargo run -p xtask -- supply-chain-check
cargo run -p xtask -- reproducible-build-check
cargo run -p xtask -- leader-readiness-check
```

External-client interop release gate:

```bash
tools/scripts/external-client-interop-gate.sh meshchatx /path/to/MeshChatX
tools/scripts/external-client-interop-gate.sh sideband /path/to/Sideband
tools/scripts/external-client-interop-gate.sh columba /path/to/columba
```

The gate does not download external clients. Provide the source checkout as the
second argument or set `MESHCHATX_ROOT`, `SIDEBAND_ROOT`, or `COLUMBA_ROOT`.
Do not claim interoperability for a client unless this gate emits
`status: "pass"` in its summary artifact for the release candidate.

Optional soak:

```bash
./tools/scripts/soak-rnx.sh
```

Nightly mesh simulation (3-10 node ring):

- Scheduled workflow: `.github/workflows/nightly-mesh.yml`
- Local dry-run command:

```bash
cargo run -p rns-tools --bin rnx -- mesh-sim --nodes 5 --timeout-secs 60
./tools/scripts/mesh-sim-rnx.sh
```

Nightly ESP32 hardware-in-loop smoke:

- Scheduled workflow: `.github/workflows/nightly-embedded-hil.yml`
- Runbook: `docs/runbooks/embedded-hil-esp32.md`
- Local gated command:

```bash
cargo run -p xtask -- embedded-hil-check
```

Queue pressure tuning and overflow policy guidance:

- `docs/runbooks/queue-pressure-tuning.md`
- `docs/runbooks/security-review-checklist.md`
- `docs/runbooks/fuzzing-campaign.md`
- `docs/runbooks/incident-response-playbooks.md`
- `docs/runbooks/disaster-recovery-drills.md`
- `docs/runbooks/soak-chaos-campaign.md`
- `docs/runbooks/compliance-profiles.md`
- `docs/runbooks/cve-response-workflow.md`
- `docs/runbooks/reference-integrations.md`

Supply-chain artifacts generated by the release gate:

- `target/supply-chain/sbom/cargo-metadata.sbom.json`
- `target/supply-chain/provenance/artifact-provenance.json`
- `target/supply-chain/provenance/artifact-provenance.sha256`
- `target/supply-chain/reproducible/reproducible-build-report.txt`

Leader-grade readiness certification artifact:

- `target/release-readiness/leader-grade-readiness.md`
- `target/release-readiness/certification-report.md`
- `target/release-readiness/certification-report.json`

External-client interop gate artifacts:

- `target/interop/external-client-gate/<client>/report.json`
- `target/interop/external-client-gate/<client>/gate-summary.json`

The summary artifact must include the selected external client checkout path,
Git revision metadata when available, generated client config/state artifacts,
logs, and destination hashes. Keep this artifact with the release candidate
evidence before making any external-client interoperability claim.

When the gate fails, classify it as a Rust-side regression, external client
setup/dependency drift, external client behavior change, or harness/environment
flake. Record that classification with the report/log paths in the release notes
or a follow-up issue before shipping.

Embedded footprint report artifact:

- `target/embedded/footprint-report.txt`

## 6. Canary Lane and Rollback Criteria

This section describes a desired release lane. It should not be read as a
statement that all referenced artifacts are produced by current PR CI.

Canary gate command:

```bash
cargo run -p xtask -- canary-criteria-check
```

Rollback triggers (objective):

1. `overall_status != PASS` in `target/release-scorecard/release-scorecard.json`
2. `soak_status != pass`
3. `soak_failures > 0` or `soak_mesh_failures > 0`
4. security checklist PASS rows below required floor (`CANARY_MIN_SECURITY_PASS_ROWS`, default `8`)
5. supply-chain artifact count below required floor (`CANARY_MIN_SUPPLY_CHAIN_ARTIFACTS`, default `1`)

`performance_status` in the release scorecard is advisory only until the legacy Criterion
budgets are re-baselined and maintained again.

Report artifacts:

- `target/release-readiness/canary-criteria-report.md`
- `target/release-readiness/canary-criteria-report.json`

## 7. Release metadata

- Workspace versions bumped intentionally.
- `Cargo.lock` committed for reproducible builds.
- Changelog/release notes summarize API and migration impacts.
- Relevant migration notes updated in `docs/migrations/`.
- RC execution and tagging follow `docs/runbooks/release-candidate-runbook.md`.
- crates.io packaging and rename policy follow `docs/runbooks/crates-io-publish-plan.md`.
