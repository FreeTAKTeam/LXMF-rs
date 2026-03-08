# `lxmf_sdk_app`

`lxmf_sdk_app` is the first-party Flutter/Dart package for the `sdk-app` contract in this repo.

This package currently does four things:

- mirrors the app-facing typed model from `lxmf_sdk::app`
- defines the client/binding seam the package exposes
- implements a first native Dart FFI bridge for the `rns-embedded-ffi` v1 node-centric API
- validates the wrapper vocabulary against `docs/fixtures/sdk-app-v1`

Current scope:

- typed `Config`, `Profile`, `SendRequest`, `SendReceipt`, `RuntimeStatus`, `Event`, `AppError`
- typed delivery-plan and delivery-helper models
- `AppClient` facade over an abstract `AppBinding`
- `EmbeddedNodeBridge` for start/stop/status/send/subscribe over `rns-embedded-ffi`
- fixture-backed contract tests for shared `sdk-app` scenarios
- a minimal host-side smoke example in `example/embedded_node_smoke.dart`
- typed transport selection on `Config` via `TransportMode`

Planned next steps:

1. add richer wrapper-facing tests for real event/error/capability translation
2. add platform packaging/loading guidance for Android/iOS/macOS host builds
3. add an example Flutter app as a smoke harness
4. expand the bridge beyond the initial embedded-node lifecycle/send path

Quick smoke run from this package directory:

```sh
export RNS_EMBEDDED_FFI_LIB=/absolute/path/to/librns_embedded_ffi.dylib
dart analyze
dart test
dart run example/embedded_node_smoke.dart
```

The example loads the native `rns-embedded-ffi` library from `RNS_EMBEDDED_FFI_LIB` when set, then falls back to the platform loader search path.

This package is intentionally contract-first. It now has a real FFI bridge, but it does not yet claim end-to-end mobile BLE support.
