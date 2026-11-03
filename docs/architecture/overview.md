# LXMF-rs Architecture

The [package guide](../project-layout.md) maps the current workspace. The
release baseline and evidence limits live in the [roadmap](../status/current-roadmap.md);
the [SDK guide](../sdk/README.md) is the application integration entry point.

## Core Principles
- Protocol code is isolated from operator workflows.
- Runtime behavior is explicit and testable.
- Public API surfaces are narrow and crate-scoped.
- Security controls are default-on, measurable, and contract-backed.
- Protocol extension growth is versioned and governed by registry.

## Stable Public Crates
- `lxmf`
- `lxmf-wire`
- `lxmf-sdk`
- `reticulum-rs`
- `reticulum-rs-core`
- `reticulum-rs-transport`
- `reticulum-rs-rpc`

## Extension Governance
- Registry source of truth: `docs/contracts/extension-registry.md`
- Governance ADR: `docs/adr/0005-extension-registry-governance.md`
- CI gate: `extension-registry-check`

## SDK Integration Guide
- Guide index: `docs/sdk/README.md`
- Lifecycle/event operations: `docs/sdk/lifecycle-and-events.md`
- Remote mTLS: `docs/sdk/remote-mtls.md`
- Delivery states: `docs/sdk/delivery-states.md`
- Error handling: `docs/sdk/error-handling.md`
- Profile/security configuration: `docs/sdk/configuration-profiles.md`
- JSON and wire-field mapping details: `docs/architecture/json-lxmf-fields.md`

For desktop/service integrations, prefer the typed ZeroMQ backend and the
app-facing `lxmf_sdk::app::Client` domain handles. The daemon compiles ZeroMQ
support by default but opens a listener only when explicitly configured.
Unix, HTTP, and TLS/mTLS RPC remain supported. `lxmf-runtime` supplies the
active in-process SDK backend.

The opt-in durable broker adds SQLite-backed custody, an ordered consumer
journal, stored acknowledgements, and outbound operation reconciliation.
It requires persistent storage and capability negotiation; ordinary SDK event
replay and transport handoff do not establish durable custody or peer delivery.
See [durable-broker operations](../goals/zmq-rch-durable-broker/OPERATIONS.md).

The patched ZeroMQ transport is published separately as `reticulum-rs-zeromq`
from `vendor/zeromq`, retaining its upstream MIT notice. Workspace boundary
checks continue to govern first-party library/application dependency edges.

## Layering Rules
- `crates/libs/*` must not depend on `crates/apps/*`.
- `lxmf-wire` (`crates/libs/lxmf-core`) must not directly depend on `tokio`, `clap`, or `ureq`; its JSON/file helpers are optional under `std`.
- `reticulum-rs-core` (`crates/libs/rns-core`) must not directly depend on `tokio` or `clap`.
- CLI/daemon concerns live in `crates/apps/*`.
- Module size policy and exception registry:
  - `docs/architecture/module-size-policy.md`
  - `docs/architecture/module-size-allowlist.txt`
- `no_std` / `alloc` capability map:
  - `docs/contracts/sdk-v2-feature-matrix.md` (`no_std` audit table)
- Architecture lint gate:
  - `cargo xtask ci --stage architecture-lint`
  - report artifact: `target/architecture/boundary-report.txt`

## Protocol core boundary (#625)

`[workspace.metadata.boundaries]` in `Cargo.toml` is authoritative for the
`no_std_core` crate set and each crate's `no_std_features` build configurations:

| Crate | Bare-metal features | Responsibility |
| --- | --- | --- |
| `reticulum-rs-core` | `alloc`; `alloc,fernet-aes128` | packets, identities, crypto and ratchets |
| `rns-embedded-core` | `alloc`; `alloc,serde` | constrained packet/message primitives and store/transport ports |
| `rns-embedded-runtime` | `alloc` | alloc-backed scheduling and dispatch over those ports |
| `lxmf-embedded-mini` | none | fixed-capacity, no-alloc LXMF primitives |

Dependencies point inward: applications/SDK/FFI use runtime/transport adapters,
which use protocol primitives. The designated core must not import daemon policy,
Tokio orchestration, CLI, HTTP, TAK/CoT or ZeroMQ APIs. Existing store/transport
traits, caller-provided RNGs and clocks keep external services outside this
boundary; no new service abstraction is needed for this change. `lxmf-wire`,
`lxmf-propagation`, the full transport and `rns-embedded-mininode` remain
`std-first`; the `alloc` compatibility feature is not a bare-metal guarantee.

`check-boundaries.sh` inspects dependency declarations of every kind, including
inactive optional and target-specific dependencies, against
`forbidden_core_deps`, and checks source imports and the existing exact workspace
edge allowlists. Host dev-only JSON fixtures are allowed; production/build JSON
is rejected. `check-no-std-core.sh` builds each feature set separately for
`thumbv7em-none-eabi`, including the optional embedded Serde path, and checks
active transitive normal/build dependencies. Host proc macros and host test
helpers may use std; they are not part of the target protocol graph. Separate
Cargo invocations prevent workspace feature unification from masking a std
requirement. A host feature-tree check also rejects accidental
`reticulum-rs-core/std` activation from the mininode without defaults.

The existing per-PR `architecture-checks` job runs these checks, seeded forbidden
optional/dev/build dependencies, a seeded unlisted workspace edge, and the host
embedded adapter tests. Default desktop features continue to enable core/std
explicitly. This is compile and dependency-boundary evidence, not device HIL.

## Security Architecture
- Threat model source of truth: `docs/adr/0004-sdk-v25-threat-model.md`.
- Security review checklist source of truth: `docs/runbooks/security-review-checklist.md`.
- Unsafe governance ADR: `docs/adr/0006-unsafe-code-audit-governance.md`.
- Unsafe policy and inventory:
  - `docs/architecture/unsafe-code-policy.md`
  - `docs/architecture/unsafe-inventory.md`
- Unsafe CI gate:
  - `unsafe-audit-check` via `cargo xtask ci --stage unsafe-audit-check`
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
