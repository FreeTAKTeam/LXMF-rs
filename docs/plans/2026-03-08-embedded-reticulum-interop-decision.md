# Embedded vs Reticulum Interop Decision Plan

Last updated: 2026-03-08

## Goal

Make the repository explicit about what `rns-embedded-*` is supposed to interoperate with, then choose a concrete path to external-client interoperability without leaving wrappers or downstream apps to guess.

## Problem Statement

The current embedded node stack and the current external-client track are not the same protocol surface.

- `rns-embedded-core` packet framing is a custom native format (`RNE1`) in [crates/libs/rns-embedded-core/src/packet.rs](../../crates/libs/rns-embedded-core/src/packet.rs).
- Embedded LXMF payload encoding is a custom minimal envelope (`ELX1`) in [crates/libs/rns-embedded-core/src/lxmf_min.rs](../../crates/libs/rns-embedded-core/src/lxmf_min.rs).
- External clients such as MeshChatX, Sideband, and Columba use real Reticulum identity/pathing plus real LXMF message construction and delivery behavior.

Because of that, a passing `rns-embedded-*` send/receive smoke test does not prove interoperability with MeshChatX, Sideband, or Columba.

## Required Decision

The repo must choose one of these as the normative answer:

1. `rns-embedded-*` is an embedded-native protocol family.
2. `rns-embedded-*` is intended to become Reticulum/LXMF compatible.

Until that choice is explicit, wrapper work will keep producing false confidence.

## Decision

The embedded track is intended to become a true Reticulum/LXMF peer.

The repository should therefore choose:

- `Option C: Build a Reticulum-Compatible Embedded Stack`

Implications:

- `rns-embedded-*` is not a separate long-term public protocol family
- the current `RNE1` and `ELX1` formats are temporary/internal scaffolding, not the target external interoperability contract
- wrapper work on top of `rns-embedded-*` must not claim MeshChatX, Sideband, or Columba interoperability until the embedded stack speaks the required Reticulum/LXMF semantics
- external-client interoperability is a real success criterion for the embedded track, not a nice-to-have add-on

## Immediate Repo Guidance

Until the compatibility work lands:

- local embedded-peer smoke tests are valid only as embedded-native proofs
- wrapper docs must avoid wording that implies external-client interoperability
- embedded-native transport/message artifacts should be documented as temporary or internal where they are still required

## Non-Goals

- Pretending the existing `RNE1`/`ELX1` stack is already Reticulum-compatible
- Rewriting the entire embedded stack before a design decision is made
- Freezing wrapper semantics around undefined external-client expectations

## Current Truths To Lock

### 1. Embedded-native interop exists

These are valid claims today:

- Flutter wrapper can load and drive `rns-embedded-ffi`
- wrapper can start/stop/send against the embedded node contract
- wrapper can exchange messages with another embedded-runtime peer over the embedded TCP transport

### 2. External-client interop is not yet proven

These are not valid claims today:

- Flutter wrapper can message MeshChatX
- Flutter wrapper can message Sideband
- Flutter wrapper can message Columba

### 3. Existing docs need stronger boundary language

The repository already has both of these tracks:

- native embedded planning:
  - [docs/plans/2026-03-05-native-embedded-node-mode-plan.md](./2026-03-05-native-embedded-node-mode-plan.md)
  - [docs/contracts/native-embedded-interop-profile-v1.md](../contracts/native-embedded-interop-profile-v1.md)
- external-client compatibility planning:
  - [docs/contracts/compatibility-matrix.md](../contracts/compatibility-matrix.md)
  - [docs/fixtures/interop/v1/README.md](../fixtures/interop/v1/README.md)

What is missing is the explicit statement that the current embedded-native profile is not the external-client profile.

## Options

### Option A: Stay Embedded-Native

Definition:

- `rns-embedded-*` remains a separate embedded runtime contract
- wrappers target that contract only
- external-client compatibility is handled elsewhere in the repo

Benefits:

- fastest path for mobile/host wrappers
- smallest implementation surface
- avoids binding embedded work to Python Reticulum parity immediately

Costs:

- no direct MeshChatX/Sideband/Columba interoperability
- app developers need a different stack when they need external-client compatibility

When to choose:

- if the product goal is “simple embedded node SDK”
- if time-to-wrapper-adoption matters more than ecosystem interop

### Option B: Build an Adapter Layer

Definition:

- keep the current embedded-native runtime
- add a compatibility bridge that translates between embedded-native frames/envelopes and Reticulum/LXMF semantics

Benefits:

- preserves existing embedded-native work
- creates a path to external-client interoperability without replacing the entire stack

Costs:

- highest semantic risk
- easy to get subtly wrong around identity, routing, delivery modes, receipts, propagation, and security semantics
- requires a very strong acceptance test harness

When to choose:

- if embedded-native code investment should be preserved
- if external interoperability is required but a full Reticulum implementation is too large right now

### Option C: Build a Reticulum-Compatible Embedded Stack

Definition:

- embedded runtime becomes a true Reticulum/LXMF node surface
- custom `RNE1`/`ELX1` transport/message formats stop being the normative external wire

Benefits:

- clearest long-term story
- most honest route to Sideband/MeshChatX/Columba interoperability

Costs:

- largest implementation cost
- likely touches identity, addressing, packet framing, transport, storage, and fixture strategy

When to choose:

- if external-client interoperability is a product requirement, not an optional future direction

## Decision Criteria

Choose the path based on these questions:

1. Must a Flutter/mobile client built on this repo be able to message MeshChatX, Sideband, or Columba directly?
2. Is the current embedded-native framing meant to ship as a durable public protocol, or only as an internal stepping stone?
3. Is compatibility with Python Reticulum/LXMF considered a release gate for the embedded track?
4. Are we willing to carry two separate node stacks in the repo?

For the current product direction, the answer is:

- yes, Flutter/mobile clients built on this repo are expected to interoperate with MeshChatX, Sideband, and Columba

That answer resolves the choice in favor of the Reticulum-compatible path.

## Recommended Repo Contract Changes

Regardless of the option selected, the repo should make these changes first:

1. Add an explicit contract note that `native-embedded-interop-profile-v1` is not the same thing as external-client compatibility.
2. Label wrapper docs as `embedded-native` unless they are backed by external-client proof.
3. Add an external interop acceptance section that defines what counts as “works with MeshChatX/Sideband/Columba”.
4. Add a release-gated proof requirement before any wrapper README claims external interoperability.

## Acceptance Criteria For External Interop

No branch should claim external-client interoperability until all of the following are true:

1. One real external client is started non-interactively in CI or a reproducible runbook.
2. LXMF destination discovery is automated.
3. A message sent from the Rust/FFI/wrapper path is observed by the external client through its own persisted state or API.
4. A reply from the external client is observed by the Rust/FFI/wrapper path.
5. The test artifact records exact client version, config, and proof transcript.

## Proposed Issue Sequence

### Issue 1: Document the protocol boundary

Deliverables:

- explicit note in embedded contracts/docs that `RNE1`/`ELX1` are embedded-native
- explicit note in wrapper docs that local embedded-peer proof does not equal external-client proof

### Issue 2: Choose the interoperability strategy

Deliverables:

- ADR or plan choosing one of:
  - embedded-native only
  - compatibility adapter
  - Reticulum-compatible embedded stack

### Issue 3: Define external interop acceptance tests

Deliverables:

- normative test definition for MeshChatX, Sideband, and Columba proof
- required artifacts and runbook

### Issue 4: Build the first compatibility spike

Deliverables depend on chosen strategy:

- adapter spike, or
- Reticulum-compatible embedded spike

Target:

- MeshChatX first, because it has the cleanest headless/API surface

### Issue 5: Add release-gated external-client proof

Deliverables:

- CI or reproducible harness
- transcript artifact
- explicit pass/fail ownership

## Immediate Implementation Sequence

With the decision made, the next engineering order should be:

1. Document the boundary clearly
   - close wording gaps in embedded and wrapper docs

2. Freeze external interop acceptance tests
   - define exactly what counts as MeshChatX/Sideband/Columba proof

3. Replace or bypass embedded-native wire assumptions
   - remove the implicit assumption that `RNE1`/`ELX1` is the eventual external wire
   - decide whether the migration path is direct replacement or temporary adapter-backed transition

4. Build the first compatibility spike against MeshChatX
   - because it has the cleanest headless/API surface for repeatable proof

5. Add release-gated proof before making wrapper claims
   - no public README/API claim should outrun the proof harness

## What This Plan Does Not Decide

This plan does not yet decide whether the Reticulum-compatible path is implemented by:

- directly replacing the current embedded-native wire/message scaffolding, or
- introducing a short-lived transitional adapter while the true implementation is built

That is the next architecture question, but it is now subordinate to the main decision rather than blocking it.

## Definition of Done

This decision slice is done when:

1. The repo no longer blurs embedded-native proof with external-client proof.
2. The Reticulum-compatible embedded path is explicitly chosen.
3. The next implementation work can be sequenced without ambiguity.
