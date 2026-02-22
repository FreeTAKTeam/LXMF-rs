# ADR-0003: Aggressive Platform Split

## Status
Accepted

## Date
2026-02-19

## Context
The previous monolithic crate boundaries made it difficult to enforce architecture rules across protocol logic, transport/runtime orchestration, and operator tooling.

## Decision
- Introduce layered public crates under `crates/libs/*`:
  - `lxmf-core`, `lxmf-sdk`
  - `rns-core`, `rns-transport`, `rns-rpc`
- Move binary entrypoints to `crates/apps/*`:
  - `lxmf-cli`, `reticulumd`, `rns-tools`
- Add boundary checks and CI jobs that enforce layering and API drift control.
- Move Python interop harness ownership out of this repository.
- Keep `lxmf-router` and `lxmf-runtime` as transitional stubs outside the active workspace and
  outside the stable public contract surface.

## Consequences
- Immediate hard break in repository structure and crate paths.
- Faster independent evolution of protocol libraries vs operator binaries.
- Stronger CI posture for API governance and dependency policy.
- Reduced public-surface ambiguity by treating router/runtime stubs as non-authoritative during
  the SDK v2.5 cutover window.
