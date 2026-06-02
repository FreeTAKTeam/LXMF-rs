# ADR-0003: Aggressive Platform Split

## Status
Accepted

## Date
2026-02-19

## Context
The previous monolithic crate boundaries made it difficult to enforce architecture rules across protocol logic, transport/runtime orchestration, and operator tooling.

Historical naming note: this ADR describes the repository split using the
workspace directory names that existed at the time. The current published
crates.io names for those crates are `lxmf-wire`, `reticulum-rs-core`,
`reticulum-rs-transport`, and `reticulum-rs-rpc`.

## Decision
- Introduce layered public crates under `crates/libs/*`:
  - `lxmf-core` (`lxmf-wire` on crates.io), `lxmf-sdk`
  - `rns-core` (`reticulum-rs-core`), `rns-transport` (`reticulum-rs-transport`), `rns-rpc` (`reticulum-rs-rpc`)
- Move binary entrypoints to `crates/apps/*`:
  - `lxmf-cli`, `reticulumd`, `rns-tools`
- Add boundary checks and CI jobs that enforce layering and API drift control.
- Move Python interop harness ownership out of this repository.
- Keep any router/runtime transition surface outside the active workspace and
  outside the stable public contract surface. The temporary `lxmf-router` and
  `lxmf-runtime` stub crates have since been removed.

## Consequences
- Immediate hard break in repository structure and crate paths.
- Faster independent evolution of protocol libraries vs operator binaries.
- Stronger CI posture for API governance and dependency policy.
- Reduced public-surface ambiguity by keeping router/runtime stubs
  non-authoritative during the SDK v2.5 cutover window, then retiring them.
