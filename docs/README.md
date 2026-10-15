# Documentation Map

The documentation tree contains maintained guidance, public contracts,
generated baselines, and historical release records. Use this page to choose
the current source of truth instead of relying on similarly named historical
documents.

The current stable release is [v0.14.0](release-notes-v0.14.0.md), with Rust
1.88 as its minimum. Guides describe the maintained source; versioned release
notes and evidence describe their named immutable tags.

## Start here

- [Getting started](getting-started.md): install, build, run, and validate the
  project.
- [Workspace and package guide](project-layout.md): active crates, binaries,
  embedded surfaces, and dependency boundaries.
- [Current roadmap](status/current-roadmap.md): repository-wide parity posture,
  release confidence, evidence boundaries, and execution order.
- [SDK integration guide](sdk/README.md): supported application integration
  path.
- [MeshChat Rust POC runbook](runbooks/meshchat-api.md): opt-in local API,
  supported chat flows, and current test limits.
- [MeshChat 2.4.0 source contract](contracts/meshchat/README.md): pinned HTTP
  and WebSocket reference specification for the POC.
- [Checked examples](examples.md): daemon, SDK, transport, and validation
  examples.

## Sources of truth

Update these documents when their corresponding behavior changes:

- [Current roadmap](status/current-roadmap.md): repository-level status and
  execution order.
- [Reticulum parity matrix](status/reticulum-parity-matrix.md): maintained
  Reticulum row-level parity status.
- [LXMF parity matrix](status/lxmf-parity-matrix.md): maintained LXMF row-level
  parity status.
- [Software parity ledger](status/software-parity-ledger.md): implementation
  work packets and evidence ownership.
- [Independent interoperability evidence](interop/README.md): versioned rns-rs
  and Reticulum-Go evidence.
- [Performance report](performance.md): current methodology and generated
  results for the explicitly pinned v0.10.1 dataset. The
  [v0.14.0 release dashboard](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.14.0/lxmf-rs-performance.html)
  carries the newer published measurements.
- [Contracts](contracts/): public compatibility, support, API, payload, RPC,
  and protocol guarantees.
- [Interfaces](interfaces/): interface-specific configuration and integration
  guidance.
- [Runbooks](runbooks/): operator, verification, and release procedures.
- [Architecture](architecture/): active architecture policy and governance.
- [Architecture decisions](adr/): rationale for major design directions.

## Integration and operation

- [SDK guide](sdk/README.md)
- [SDK quickstart](sdk/quickstart.md)
- [API surface and stability](lxmf-rs-api.md)
- [CLI quick reference](lxmf-cli.md)
- [`lxmd` systemd deployment](runbooks/lxmd-systemd.md)
- [`reticulumd` operational deployment](runbooks/reticulumd-operational-deployment.md)
- [Logging and diagnostics](runbooks/logging-and-diagnostics.md)
- [SDK configuration cookbook](runbooks/sdk-config-cookbook.md)
- [Meshtastic tunnel interface](interfaces/meshtastic.md)
- [RNode Bluetooth Classic/SPP interface](interfaces/rnode-spp.md)

## Durable ZeroMQ integration

- [v0.14.0 upgrade guide](migrations/v0.14.0-zmq.md): package alignment,
  envelope construction, and opt-in storage migration.
- [Durable-broker operations](goals/zmq-rch-durable-broker/OPERATIONS.md):
  controlled cutover, stored acknowledgements, finite budgets, backup restore,
  and downgrade boundaries.
- [Broker implementation evidence](goals/zmq-rch-durable-broker/PROGRESS.md):
  tested behavior and remaining production/physical qualification.
- [Original implementation plan](goals/zmq-rch-durable-broker/PLAN.md): design
  rationale; use the operations and evidence pages for shipped behavior.

## Contracts and architecture

- [Architecture overview](architecture/overview.md)
- [JSON and wire-field mapping](architecture/json-lxmf-fields.md)
- [Compatibility contract](contracts/compatibility-contract.md)
- [Compatibility matrix](contracts/compatibility-matrix.md)
- [Third-party compatibility kit](contracts/third-party-compatibility-kit.md)
- [Support and LTS policy](contracts/support-policy.md)
- [Extension registry](contracts/extension-registry.md)
- [RPC contract](contracts/rpc-contract.md)
- [Payload contract](contracts/payload-contract.md)

## Release and evidence

- [Real-world interoperability evidence](status/real-world-interoperability.md)

- [Latest GitHub release](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest)
- [v0.14.0 release notes](release-notes-v0.14.0.md) and
  [published release record](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.14.0)
- [v0.14.0 ZeroMQ migration guide](migrations/v0.14.0-zmq.md)
- [v0.13.0 release notes](release-notes-v0.13.0.md) and
  [Reticulum 1.5.5 delta](status/rns-1.5.5-delta.md)
- [v0.12.0 release notes](release-notes-v0.12.0.md) and
  [historical candidate ledger](status/v0.12.0-release-candidate.md)
- [v0.11.0 release notes](release-notes-v0.11.0.md)
- [v0.10.1 release notes](release-notes-v0.10.1.md) and
  [stable release evidence](status/v0.10.1-release.md)
- [v0.9.9 release notes](release-notes-v0.9.9.md)
- [v0.10.0 release notes](release-notes-v0.10.0.md)
- [v0.10.0 release evidence](status/v0.10.0-release.md)
- [v0.10.0 performance dataset](performance/v0.10.0.json) and
  [dashboard](performance/v0.10.0.html)
- [v0.10.0 historical candidate evidence](status/v0.10.0-release-candidate.md)
- [v0.10.0 RNS 1.5 migration guide](migrations/v0.10.0-rns-1.5.md)
- [v0.10.1 RNS 1.5.2 migration guide](migrations/v0.10.1-rns-1.5.2.md)
- [v0.11.0 RNode migration guide](migrations/v0.11.0-rnode.md)
- [Release readiness](runbooks/release-readiness.md)
- [Release process](RELEASING.md)
- [crates.io publication plan](runbooks/crates-io-publish-plan.md)
- [Independent implementation evidence](interop/README.md)
- [Checked-in performance methodology and v0.10.1 results](performance.md)
- [Latest public performance dashboard](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest/download/lxmf-rs-performance.html)
- [Historical performance snapshot](PerformancesComparison.html)

## Code-adjacent artifacts

The following directories contain documentation-shaped files that are consumed
by tests, code generation, or CI. Treat changes to them with the same care as
source changes:

- [`docs/schemas`](schemas/)
- [`docs/fixtures`](fixtures/)
- [`docs/openrpc`](openrpc/)
- [`docs/contracts/baselines`](contracts/baselines/)

## Historical and migration material

[Migration guides](migrations/) are retained for users crossing public API or
architecture boundaries. Superseded release-candidate notes and evidence
ledgers are historical records; they do not override the current roadmap or
stable release notes. Completed implementation plans and issue boards belong
in Git history instead of the live documentation tree.

## Retention rules

- Prefer one maintained document over several overlapping notes.
- When adding a canonical document, remove or clearly mark the superseded one
  in the same change.
- Keep file paths portable; do not commit machine-local absolute paths.
- Link broad entry points to current sources of truth so historical notes do
  not become the default.
- Before deleting documentation, search code, workflows, `xtask`, and other
  docs for consumers.
