# Shared Bluetooth implementation status

Updated 2026-10-15. **Incomplete Stage 0 foundation increment.** Production defaults
are unchanged. No consumer uses the new package or companion yet.

## Implemented

- Protocol-neutral Rust GATT client/server and Classic stream contracts, explicit
  owner/session generations, native deadlines, typed errors and bounded events.
- First-party Android GATT client and Classic stream mechanics, isolated leases,
  tagged notifications, terminal/EOF handling, late-handle cleanup and native
  deadline accounting. JVM tests cover close/open/write races and a single-worker
  Classic executor with final-data-before-EOF.
- Opt-in lxmf-sdk mobile-ble-adapter. Existing mobile DTOs, trait signatures,
  validators, RNode readiness checks and Android/iOS fixtures are unchanged.
  Connection plus selected subscriptions is one backend cancellation operation.
  Request channel projection does not own native connection/admission state.
- Explicit Rust 1.85 / edition 2024 package; normalized package and independent
  consumer lockfiles; CI jobs for MSRV, existing mobile contracts and companion.
- Exact SDK dependency edge, feature isolation, 500-line companion source gate,
  amended backend policy, publication order and artifact-pair verification tooling.

## Local evidence

- Rust 1.85.0 normalized-package core/android-feature/bluez-feature tests,
  aarch64-linux-android target build, and independent consumer execution pass.
- Rust 1.88.0 all-feature leaf build passes (integration evidence only).
- SDK mapping/reconnect/concurrent-open tests and eight existing mobile contract
  tests pass; Android and iOS fixture conformance tests pass.
- Android companion JVM tests, strict lint, debug and release AAR assembly pass.
- Artifact verifier synthetic tests cover correct pair, wrong Rust package from
  the same revision, tampering, native-library duplication and dirty metadata.
- Boundary, source-size, license/package notices and publication order checks pass.

The BlueZ feature currently builds optional dependencies only: **there is no
implemented Rust BlueZ backend**. The Rust Android feature currently checks the
bridge version only: **there is no Rust JNI transport implementation**. Its target
build must not be described as bridge runtime acceptance. No musl packaging or
D-Bus coexistence runtime acceptance has been performed. No whole-workspace
baseline or hosted CI results are claimed. Making GitHub checks required in branch
protection has not been performed.

## Remaining software work

Stage 0 is not accepted: finish native Rust bridges/backends, Linux packaging and
feature tests, complete consumer-compatible artifact distribution/enforcement,
and establish the curated goTenna Git baseline. The original goTenna sources
contain embedded device/location evidence; they have not been bulk imported.

Stage 1/2 REM JNI pilot integration is pending. The isolated REM worktree has no
changes. Existing btleplug implementations and original dirty LXMF-rs/ATN working
trees remain untouched. The old REM implementations have not been displaced.

Stage 3 extraction, bluer client/server, reference harness, matched RCH daemon
pinning, Android peripheral/advertising and REM integration are pending. The
reference remains torlando-tech/ble-reticulum at
271b4ab8ba45a8c6a7864aef5919e77f07227fe7; no interoperability result is claimed.

Stage 4 ATN dependency/transport migration and negotiated REM Binder draft-map
API are pending. Stage 5 goTenna plugin and durable receive/advancement integration
are pending. No Git source pins, AAR pins or existing public APIs in these
consumers have changed.

## Promotion gates

No Bluetooth discovery, device connection, transmission, queue advancement or
hardware test was performed. All exact-device, permission/restart, messaging and
receipt, BLE-only REM/RCH, Binder/map confirmation, physical-semantic/display and
RNode-plus-plugin coexistence gates remain open. Missing hardware prevents support
claims and promotion; it is not the reason the remaining software is incomplete.

Do not close #614 or #650, change defaults, remove old transports, or publish a
support claim on the strength of this foundation increment.
