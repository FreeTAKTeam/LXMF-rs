# LXMF-rs Architecture

## Core Principles
- Protocol code is isolated from operator workflows.
- Runtime behavior is explicit and testable.
- Public API surfaces are narrow and crate-scoped.
- Security controls are default-on, measurable, and contract-backed.
- Protocol extension growth is versioned and governed by registry.

## Stable Public Crates
- `lxmf-core`
- `lxmf-sdk`
- `rns-core`
- `rns-transport`
- `rns-rpc`

## Extension Governance
- Registry source of truth: `docs/contracts/extension-registry.md`
- Governance ADR: `docs/adr/0005-extension-registry-governance.md`
- CI gate: `extension-registry-check`

## SDK Integration Guide
- Guide index: `docs/sdk/README.md`
- Lifecycle/event operations: `docs/sdk/lifecycle-and-events.md`
- Profile/security configuration: `docs/sdk/configuration-profiles.md`

## Layering Rules
- `crates/libs/*` must not depend on `crates/apps/*`.
- `lxmf-core` must not directly depend on `tokio`, `clap`, `ureq`, or `serde_json`.
- `rns-core` must not directly depend on `tokio` or `clap`.
- CLI/daemon concerns live in `crates/apps/*`.
- Module size policy and exception registry:
  - `docs/architecture/module-size-policy.md`
  - `docs/architecture/module-size-allowlist.txt`
- `no_std` / `alloc` capability map:
  - `docs/contracts/sdk-v2-feature-matrix.md` (`no_std` audit table)

## Security Architecture
- Threat model source of truth: `docs/adr/0004-sdk-v25-threat-model.md`.
- Security review checklist source of truth: `docs/runbooks/security-review-checklist.md`.
- Primary controls:
  - Local-only default RPC binding with explicit secure auth required for remote bind.
  - Token replay protection (`jti`) and rate limiting for authenticated HTTP RPC.
  - Structured redaction for event/error payloads and request traces.
  - Cursor and stream-gap semantics that fail closed on invalid/expired cursors.
  - Bounded queue capacities with overflow policy enforcement.

## Release Scorecard Process
- Generated scorecard artifacts:
  - `target/release-scorecard/release-scorecard.md`
  - `target/release-scorecard/release-scorecard.json`
- Leader certification artifact:
  - `target/release-readiness/leader-grade-readiness.md`
- Generation command:
  - `cargo run -p xtask -- release-scorecard-check`
- Full leader readiness command:
  - `cargo run -p xtask -- leader-readiness-check`
- CI gate:
  - `release-scorecard-check` job in `.github/workflows/ci.yml`
  - `leader-readiness-check` job in `.github/workflows/leader-readiness.yml`
- Inputs for scorecard generation:
  - perf budget report (`target/criterion/bench-budget-report.txt`)
  - soak/chaos report (`target/soak/soak-report.json`)
  - supply-chain provenance (`target/supply-chain/provenance/artifact-provenance.json`)
  - security checklist (`docs/runbooks/security-review-checklist.md`)

## Legacy Cutover Note
Legacy implementation crates are no longer part of the active workspace graph. Migration artifacts remain only where retention is still required.
