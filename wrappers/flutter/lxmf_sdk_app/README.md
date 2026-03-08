# `lxmf_sdk_app`

`lxmf_sdk_app` is the first-party Flutter/Dart package for the `sdk-app` contract in this repo.

This package currently does three things:

- mirrors the app-facing typed model from `lxmf_sdk::app`
- defines the client/binding seam the package exposes
- validates the wrapper vocabulary against `docs/fixtures/sdk-app-v1`

Current supported direction:

- Flutter/Dart clients are being scoped around the Reticulum/LXMF host path
  backed by `reticulumd` RPC.
- The embedded FFI bridge from earlier work is no longer the intended public
  backend for this package.
- ESP32 and other constrained-device flows stay in the separate
  `rns-embedded-*` track for now.

Current package scope:

- typed `Config`, `Profile`, `SendRequest`, `SendReceipt`, `RuntimeStatus`, `Event`, `AppError`
- typed delivery-plan and delivery-helper models
- `AppClient` facade over an abstract `AppBinding`
- fixture-backed contract tests for shared `sdk-app` scenarios

Planned next steps:

1. add the `reticulumd` RPC backend for `AppBinding`
2. add a small Flutter example against the RPC backend
3. validate wrapper behavior against external-client proof runs
4. keep embedded-specific bridge code internal until the ESP32 track is ready to
   become a supported app backend

What is intentionally not supported right now:

- public Flutter support for the embedded FFI backend
- BLE/mobile device packaging for the embedded runtime
- claiming that the embedded bridge is the same thing as external-client
  interoperability

Quick package checks from this directory:

```sh
dart analyze
dart test
```

This package is intentionally contract-first. The public package surface is now
backend-neutral while the implementation direction shifts to `reticulumd` RPC.
