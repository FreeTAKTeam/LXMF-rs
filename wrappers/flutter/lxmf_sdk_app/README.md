# `lxmf_sdk_app`

`lxmf_sdk_app` is the first-party Flutter/Dart package for the `sdk-app` contract in this repo.

This initial scaffold does three things:

- mirrors the app-facing typed model from `lxmf_sdk::app`
- defines the client/binding seam that the native bridge must implement
- gives Flutter consumers a stable Dart surface before native transport glue is added

Current scope:

- typed `Config`, `Profile`, `SendRequest`, `SendReceipt`, `RuntimeStatus`, `Event`, `AppError`
- typed delivery-plan and delivery-helper models
- `AppClient` facade over an abstract `AppBinding`
- placeholder FFI bridge seam for the embedded/native node boundary

Planned next steps:

1. implement the native bridge for Android/iOS/macOS host builds
2. map `rns-embedded-ffi` lifecycle/send/subscription calls into the Dart binding
3. add fixture-backed contract tests against `docs/fixtures/sdk-app-v1`
4. add an example Flutter app as a smoke harness

This package is intentionally contract-first. It does not yet claim end-to-end mobile BLE support.
