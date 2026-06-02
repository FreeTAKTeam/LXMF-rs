# SDK Operation Runtime Extraction Plan

## Goal

Move reusable client-side Reticulum/LXMF logic out of app repos and into `LXMF-rs` so new clients can:

- start from a ready-to-use app/runtime SDK layer
- use a standard operation and envelope model
- benefit from shared delivery, discovery, identity, and event handling
- still register product-specific commands, payloads, and workflows

This plan is driven by the current split between:

- shared runtime/client concerns already emerging in `LXMF-rs`
- reusable but app-local logic currently living in `R3AKTClient/crates/reticulum_mobile`

## Decision

`LXMF-rs` should own the generic operation runtime.

App-specific repos should only own:

- product catalogs
- domain payload schemas
- private workflows
- UI policy

They should not reimplement:

- operation registry plumbing
- envelope validation and dispatch
- discovery and peer/runtime event handling
- identity bootstrap
- common message execution helpers

## Current Gap

`lxmf-sdk::app` already provides:

- lifecycle
- delivery helpers
- event streams
- basic runtime status
- wrapper-friendly RPC semantics

It does not yet provide:

- a first-class operation registry
- a standard command/query envelope model
- a generic execute-envelope path
- peer/discovery directory abstractions
- custom operation registration hooks

`R3AKTClient` currently has those concepts embedded in app-local Rust:

- generated operation catalog and alias resolution
- operation kind and transport-variant routing
- message envelope validation and execution
- announce/peer/domain event emission
- identity and destination bootstrap helpers

## Target Architecture

### 1. Base SDK Runtime

`lxmf-sdk`

Owns:

- runtime lifecycle
- delivery, retries, and status tracking
- event subscriptions
- runtime snapshots and capability negotiation

### 2. Operation Runtime Layer

`lxmf-sdk::app`

Owns:

- `OperationRegistry`
- `OperationEntry`
- alias resolution
- `Envelope`
- standard query/command/result/error envelopes
- envelope validation
- execute/query/command helper APIs
- discovery and peer-facing typed events

### 3. Backend Integration Layer

`reticulumd`

Owns:

- RPC exposure of operation catalog
- RPC execution of standard envelopes
- contact/discovery queries
- message history and delivery queries
- extension passthrough for custom operations

### 4. App Catalog Layer

Client/application repos

Own:

- operation catalogs for their product domain
- payload schema definitions
- domain-specific event typing
- optional custom extensions

## Non-Goals

This plan does not move all R3AKT behavior into `LXMF-rs`.

It does not make `LXMF-rs` own:

- mission registry semantics
- checklists/task models
- R3AKT-specific topic/marker/assignment policy
- app UI state or product workflows

## Extraction Principles

1. Generic before product-specific

If a concept can support multiple clients, it belongs in `LXMF-rs`.

2. Registry, not hardcoding

The SDK should not hardcode one product command set. It should expose a registry model that product catalogs plug into.

3. Typed default path, extensible escape hatch

Common commands and events should be typed. Unknown/custom operations should still have a structured raw envelope path.

4. Daemon and wrapper parity

If the SDK exposes an operation or discovery concept, `reticulumd` RPC and wrappers should expose it too.

## Proposed PR Sequence

### PR 1: Operation Registry Foundation

Branch:

- `codex/sdk-operation-registry-foundation`

Scope:

- add `OperationId`, `OperationKind`, `TransportVariant`, `OperationEntry`
- add registry storage and lookup APIs
- add alias resolution and canonicalization
- add serialization/export support for wrappers and RPC

Acceptance:

- the SDK can publish a registry of supported operations
- aliases normalize to canonical operation ids
- registry output is stable and test-covered

### PR 2: Envelope Execution Core

Branch:

- `codex/sdk-envelope-execution-core`

Scope:

- add standard app envelope types
- add validation against the operation registry
- add generic execute/query/command APIs
- add result/error envelope mapping

Acceptance:

- callers can submit a structured envelope without app-specific glue code
- invalid or unknown operations fail in a standard way
- execution semantics are reusable across clients

### PR 3: Discovery and Peer Runtime Layer

Branch:

- `codex/sdk-discovery-peer-events`

Scope:

- add peer/contact/discovery types
- add announce and peer-state event types
- add identity/bootstrap helpers
- add directory-style queries and cache helpers

Acceptance:

- common client discovery flows no longer need app-local runtime glue
- wrappers can consume typed peer/discovery events

### PR 4: `reticulumd` Operation API

Branch:

- `codex/reticulumd-operation-api`

Scope:

- expose operation registry over RPC
- expose execute-envelope RPC
- expose typed contact/discovery queries
- expose message history and delivery queries aligned to the app layer

Acceptance:

- wrappers can build product clients around one standard daemon contract
- no client needs private Rust logic just to run custom commands

### PR 5: R3AKT Extraction Spike

Branch:

- `codex/r3akt-catalog-extraction-spike`

Scope:

- move the reusable catalog/runtime concepts out of `R3AKTClient`
- keep R3AKT operations as a product catalog on top of the new shared layer
- verify parity for key command and event flows

Acceptance:

- `R3AKTClient` shrinks to product-specific behavior
- `LXMF-rs` owns the reusable command runtime

## Key API Direction

The SDK should support both:

- typed built-in helpers for common flows
- raw but structured custom operation execution

Illustrative shape:

```rust
let registry = client.operation_registry()?;
let result = client.execute_envelope(Envelope::command(
    "mission.message.send",
    payload,
))?;
```

And for advanced callers:

```rust
let result = client.execute_custom(
    "vendor.example.custom",
    payload,
    CustomExecutionOptions::default(),
)?;
```

## Required Contract Work

Before or during PR 1 and PR 2:

- freeze a registry JSON shape
- freeze an envelope JSON/RPC shape
- define unknown-operation and unknown-field rules
- define extension/custom-operation naming rules

## Testing Strategy

Add release-quality coverage for:

- registry canonicalization
- alias resolution
- envelope validation
- query vs command routing
- unknown/custom operation behavior
- RPC export parity
- wrapper decoding parity

## Merge Standard

This effort is successful when:

- a new client can ship with the shared SDK app layer and little or no private Rust runtime glue
- custom commands still work through a standard extension path
- product repos no longer need to own generic Reticulum/LXMF client runtime logic
