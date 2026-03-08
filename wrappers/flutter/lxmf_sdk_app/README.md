# `lxmf_sdk_app`

`lxmf_sdk_app` is the first-party Flutter/Dart package for the `sdk-app` contract in this repo.

This package currently does three things:

- mirrors the app-facing typed model from `lxmf_sdk::app`
- defines the client/binding seam the package exposes
- provides a first real `reticulumd` RPC binding for that seam
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
- `RpcBinding` for `reticulumd` over framed MessagePack HTTP RPC
- fixture-backed contract tests for shared `sdk-app` scenarios

Important current constraint:

- `RpcBinding` does not accept embedded transport settings like `transportMode`,
  `tcpHost`, `tcpPort`, or `tcpListenPort`. Those remain part of the shared
  app-model types, but they are not a supported Flutter backend path right now.

Quick start:

```dart
import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';

final client = AppClient(
  RpcBinding(
    RpcConnectionOptions(
      endpoint: Uri.parse('http://127.0.0.1:4243/rpc'),
    ),
  ),
);

final handle = await client.start(
  const Config(profile: Profile.desktopDefault),
);
final receipt = await client.send(
  const SendRequest(
    source: 'flutter-src',
    destination: 'flutter-dst',
    payload: 'hello',
  ),
);
print('${handle.runtimeId} accepted ${receipt.messageId}');
```

Current next steps:

1. validate the RPC binding against local `reticulumd` smoke runs
2. validate wrapper behavior against external-client proof runs
3. keep embedded-specific bridge code internal until the ESP32 track is ready to
   become a supported app backend

What is intentionally not supported right now:

- public Flutter support for the embedded FFI backend
- BLE/mobile device packaging for the embedded runtime
- claiming that the embedded bridge is the same thing as external-client
  interoperability

Quick package checks from this directory:

```sh
dart pub get
dart analyze
dart test
dart run example/rpc_smoke.dart http://127.0.0.1:4243/rpc
```

This package is intentionally contract-first. The public package surface is now
backend-neutral while the implementation direction shifts to `reticulumd` RPC.
