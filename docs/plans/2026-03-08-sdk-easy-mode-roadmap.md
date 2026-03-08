# SDK Easy-Mode Roadmap

## Goal

Make the SDK easy enough that most clients and apps can integrate with minimal custom logic.

The intended default experience is:

- create a client/node
- start it with a profile preset
- send data/messages through high-level helpers
- subscribe to typed app-level events
- stop/cleanup without manual transport, retry, queue, or polling logic

## Problem Statement

The repository already has strong low-level and contract-heavy surfaces:

- `lxmf-sdk` for host-facing client APIs
- `rns-embedded-runtime` and `rns-embedded-ffi` for embedded/native node control
- contract and capability documents across `docs/contracts`

But “easy adoption” still fails if every app must write custom logic for:

- retry and reconnect policy
- queue pressure and backpressure handling
- event-gap and restart recovery
- capability negotiation
- threading and callback behavior
- persistence and offline recovery expectations

If these semantics are not frozen centrally, wrappers will drift and every client will re-implement policy differently.

## Key Decisions

1. Easy-client policy belongs in `lxmf-sdk`, not per-wrapper.
   Wrappers should be thin adapters over a single orchestration model wherever possible.

2. Contract and conformance must come before wrapper proliferation.
   The first wrapper should validate the contract, not define it.

3. Typed app-level events are part of the core easy-mode contract.
   Default consumers should not need raw poll loops, transport internals, or low-level event decoding.

4. Profile presets and delivery helpers are part of the easy-mode core.
   They should not be deferred until after wrappers ship.

5. Behavior should be semantically identical across languages by default.
   Language bindings may differ in syntax and platform integration style, but not in retry, timeout, ordering, or recovery semantics.

## Non-Goals

- Replacing the existing low-level or advanced SDK surfaces.
- Removing manual-tick or low-level transport control for advanced consumers.
- Supporting multiple first-party wrappers in parallel before the contract is proven.
- Solving every platform-specific UI/runtime concern in the first slice.

## Required Contract Coverage

The easy-mode contract must define:

- lifecycle state machine
- threading and callback delivery guarantees
- event ordering guarantees
- retry and reconnect behavior
- queue pressure behavior
- timeout behavior
- receipt terminality semantics
- persistence/offline recovery expectations
- capability/version negotiation rules
- security and identity ownership assumptions
- advanced escape hatches and when they are allowed

## Recommended PR Sequence

### PR-1: Freeze the Easy-Mode Contract

Branch: `codex/sdk-easy-contract-v1`

Create:

- `docs/contracts/sdk-easy-api-v1.md`
- `docs/contracts/sdk-easy-events-v1.md`
- `docs/contracts/sdk-easy-errors-v1.md`
- `docs/contracts/sdk-easy-profiles-v1.md`

Acceptance criteria:

- one canonical client model exists:
  - `Node`/client handle
  - lifecycle methods
  - send helpers
  - event stream
  - typed error model
- raw transport and manual polling are explicitly advanced-only
- normative semantics are written for the required coverage listed above

Suggested commits:

- `docs: define sdk easy api v1 surface`
- `docs: define sdk easy event and error contracts`
- `docs: define sdk easy profiles and policy defaults`

### PR-2: Add Conformance Foundation

Branch: `codex/sdk-easy-conformance-foundation`

Create:

- `docs/fixtures/sdk-easy-v1/*`
- `crates/libs/test-support/tests/sdk_easy_conformance.rs`
- CI gate for fixture/contract drift

Acceptance criteria:

- language-agnostic fixtures exist for core lifecycle and delivery behavior
- Rust conformance tests execute against them
- CI fails on contract drift
- unknown-field and unknown-capability behavior is explicitly tested

Minimum fixture scope:

- start/stop/restart
- event ordering
- timeout behavior
- retry/backpressure behavior
- reconnect behavior
- typed error mapping

Suggested commits:

- `test: add sdk easy conformance fixtures`
- `test: add sdk easy conformance runner`
- `ci: gate sdk easy contract and fixture drift`

### PR-3: Implement Rust Easy-Mode Facade

Branch: `codex/sdk-easy-rust-facade`

Create:

- `crates/libs/lxmf-sdk/src/easy/mod.rs`
- `crates/libs/lxmf-sdk/src/easy/node.rs`
- `crates/libs/lxmf-sdk/src/easy/events.rs`
- `crates/libs/lxmf-sdk/src/easy/errors.rs`
- `crates/libs/lxmf-sdk/src/easy/capabilities.rs`

Acceptance criteria:

- an app can start, send, subscribe, and stop without raw poll logic
- typed app-level events are exposed by default
- capability negotiation is handled internally by default
- advanced/raw access remains available behind a separate layer

Suggested commits:

- `feat(sdk): add easy node facade and lifecycle api`
- `feat(sdk): add typed event adapters`
- `feat(sdk): add capability negotiation and fallback behavior`
- `test(sdk): add easy facade integration tests`

### PR-4: Add Profile Presets and Delivery Helpers

Branch: `codex/sdk-easy-profiles-delivery`

Create/update:

- `crates/libs/lxmf-sdk/src/easy/profiles.rs`
- `crates/libs/lxmf-sdk/src/easy/delivery.rs`
- `crates/libs/lxmf-sdk/src/easy/config.rs`
- `docs/sdk/configuration-profiles.md`

Acceptance criteria:

- presets exist for:
  - `mobile_default`
  - `desktop_default`
  - `embedded_default`
  - `testing_default`
- built-in helpers exist for:
  - retry
  - timeout
  - reconnect
  - queue pressure handling
- most applications can run on presets plus minimal caller configuration

Suggested commits:

- `feat(sdk): add easy mode profile presets`
- `feat(sdk): add delivery helpers for retry timeout and pressure`
- `test(sdk): add delivery helper behavior matrix`

### PR-5: Ship One First-Party Wrapper

Branch:

- `codex/sdk-easy-first-wrapper-kotlin`
  or
- `codex/sdk-easy-first-wrapper-swift`

Recommendation:

- choose Kotlin first if Android/mobile is the leading adopter
- choose Swift first if iOS is the leading adopter

Acceptance criteria:

- wrapper matches the frozen easy-mode semantics
- wrapper exposes async event stream/callbacks and typed errors
- wrapper default API does not require manual polling or custom retry logic
- integration tests run against the reference implementation and conformance fixtures

Suggested commits:

- `feat(wrapper): scaffold easy node wrapper api`
- `feat(wrapper): add lifecycle send and typed event surface`
- `test(wrapper): add integration and conformance coverage`

### PR-6: Add Wrapper Parity CI

Branch: `codex/sdk-easy-wrapper-parity-ci`

Acceptance criteria:

- wrapper conformance is release-gated
- Rust easy-mode and the reference wrapper are checked against shared fixtures
- contract/codegen drift is also release-gated

Suggested commits:

- `test: add wrapper parity harness`
- `ci: gate releases on sdk easy conformance`

### PR-7: Add Golden Paths and Migration Docs

Branch: `codex/sdk-easy-golden-paths`

Create/update:

- `examples/sdk-easy/rust-managed/`
- `examples/sdk-easy/kotlin-mobile/` or `examples/sdk-easy/swift-mobile/`
- `docs/sdk/quickstart.md`
- `docs/sdk/migration-to-easy.md`

Acceptance criteria:

- examples are copy-pasteable
- examples follow the same contract tested by conformance fixtures
- low-level users have a migration guide to the easy-mode surface

Suggested commits:

- `docs/examples: add rust easy-mode golden path`
- `docs/examples: add wrapper golden path`
- `docs: add migration guide to sdk easy mode`

## Why This Order

This sequence is intentionally contract-first:

- wrappers should not guess semantics
- delivery helpers and presets must exist before claiming “easy mode”
- conformance must exist before multiple bindings
- examples should document a proven surface, not a moving target

## Exit Criteria For The Overall Initiative

The initiative is successful when:

- a new app can adopt the SDK through the easy-mode surface without custom retry/reconnect/queue logic
- the reference wrapper matches Rust semantics through shared fixtures
- defaults are sufficient for common mobile, desktop, and embedded-host scenarios
- low-level escape hatches remain available but are not required for normal adoption
- behavior stays stable through release-gated conformance
