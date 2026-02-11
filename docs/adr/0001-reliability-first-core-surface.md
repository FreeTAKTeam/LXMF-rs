# ADR-0001: Reliability-First Core Surface

## Status
Accepted

## Context
The repository had drifted toward mixed protocol/runtime concerns with broad dependency exposure.

## Decision
- Convert repository to workspace layout with `crates/lxmf` as the core crate.
- Make CLI dependencies optional behind the `cli` feature.
- Keep default features minimal.
- Define explicit stable surface modules: `message`, `identity`, `router_api`, `errors`.

## Consequences
- Breaking change for consumers relying on prior broad exports.
- Lower default dependency and compile footprint.
- Clearer separation for future operator tooling crates.
