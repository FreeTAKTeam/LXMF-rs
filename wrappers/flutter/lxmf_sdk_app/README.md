# `lxmf_sdk_app`

`lxmf_sdk_app` is the first-party Flutter/Dart package for the `sdk-app` contract in this repo.

This package currently does four things:

- mirrors the app-facing typed model from `lxmf_sdk::app`
- defines the client/binding seam the package exposes
- provides a first real `reticulumd` RPC binding for that seam
- adds a conversation-focused helper on top of that RPC binding
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
- typed operation registry and envelope execution models
- typed custom-operation helper layer with alias-aware query/command dispatch
- `AppClient` facade over an abstract `AppBinding`
- `WorkspaceClient` that groups the common app families behind one entrypoint
- `RpcBinding` for `reticulumd` over framed MessagePack HTTP RPC
- `ConversationClient` / `RpcConversationClient` for message history + live conversation updates
- identity, contact, message-history, and delivery-status helpers for RPC-backed clients
- operation catalog fetch + alias-aware envelope execution helpers for RPC-backed clients
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

Workspace-oriented flow:

```dart
final workspace = WorkspaceClient.rpc(
  RpcConnectionOptions(
    endpoint: Uri.parse('http://127.0.0.1:4243/rpc'),
  ),
);

await workspace.start(const Config(profile: Profile.desktopDefault));
final identities = await workspace.discovery.identityList();
final status = await workspace.status();

print('runtime=${status.runtimeId} identities=${identities.length}');
```

Workspace workflow flow:

```dart
final workspace = WorkspaceClient.rpc(
  RpcConnectionOptions(
    endpoint: Uri.parse('http://127.0.0.1:4243/rpc'),
  ),
);

await workspace.start(const Config(profile: Profile.desktopDefault));
final peer = await workspace.flows.ensurePeerReady('peer-demo');
final topic = await workspace.flows.ensureTopic('ops/demo');
final note = await workspace.flows.publishFieldNote(
  topicPath: 'ops/demo',
  payload: const <String, Object?>{'body': 'field note'},
);

print('peer=${peer.identity} topic=${topic.topic.topicId} note=${note.published}');
```

Workspace sync/report flow:

```dart
final sync = await workspace.flows.ensureTopicSync('ops/demo');
final report = await workspace.flows.publishAttachmentReport(
  topicPath: 'ops/reports',
  attachment: const AttachmentDraft(
    name: 'report.txt',
    contentType: 'text/plain',
    bytesBase64: 'cmVwb3J0',
  ),
  summaryPayload: const <String, Object?>{'title': 'demo report'},
);

print(
  'sync=${sync.subscribed}/${sync.telemetry.length} '
  'attachment=${report.attachment.attachmentId}',
);
```

Operation-catalog flow:

```dart
final registry = await client.operationRegistry();
final resolved = registry.resolve('sdk_identity_list_v2');
print('canonical op: ${resolved?.canonicalId}');

final response = await client.queryOperation(
  'sdk_identity_list_v2',
  const <String, Object?>{},
  correlationId: 'flutter-op-demo',
);
print('accepted=${response.accepted} op=${response.operationId}');
```

Typed custom-operation flow:

```dart
final operations = OperationClient(client);

final status = await operations.query<Map<String, Object?>>(
  OperationCall<Map<String, Object?>>(
    operationId: 'sdk_snapshot_v2',
    payload: const <String, Object?>{},
    decode: (payload) => (payload as Map<Object?, Object?>).map(
      (key, value) => MapEntry(key.toString(), value),
    ),
  ),
);

print('query ${status.operationId} accepted=${status.accepted}');
```

Typed vendor/custom-command flow:

```dart
final commands = CustomCommandClient(OperationClient(client));

final result = await commands.invoke<Map<String, Object?>>(
  CustomCommandCall<Map<String, Object?>>(
    operationId: 'vendor.example.custom',
    target: 'node-b',
    timeoutMs: 500,
    payload: const <String, Object?>{'body': 'hello'},
    decodeEcho: (payload) => (payload as Map<Object?, Object?>).map(
      (key, value) => MapEntry(key.toString(), value),
    ),
  ),
);

print('cmd=${result.command} correlation=${result.correlationId}');
```

Typed voice-signaling flow:

```dart
final voice = VoiceSessionClient(OperationClient(client));

final sessionId = await voice.open(peerId: 'node-b', codecHint: 'opus');
final state = await voice.update(
  sessionId: sessionId,
  state: VoiceSessionState.active,
);
final closed = await voice.close(sessionId);

print('voice=$sessionId state=${state.name} closed=$closed');
```

Typed topic flow:

```dart
final topics = TopicClient(OperationClient(client));

final created = await topics.create(
  topicPath: 'ops/alerts',
  metadata: const <String, Object?>{'kind': 'ops'},
);
final listed = await topics.list(limit: 10);
final published = await topics.publish(
  topicId: created.topicId,
  payload: const <String, Object?>{'message': 'hello topic'},
  correlationId: 'topic-corr-1',
);

print('topic=${created.topicId} listed=${listed.topics.length} published=$published');
```

Typed telemetry flow:

```dart
final telemetry = TelemetryClient(OperationClient(client));

final points = await telemetry.query(
  topicId: 'topic-1',
  fromTsMs: 0,
  limit: 10,
);
final subscribed = await telemetry.subscribe(
  topicId: 'topic-1',
  fromTsMs: 0,
  limit: 10,
);

print('telemetry=${points.length} subscribed=$subscribed');
```

Typed marker flow:

```dart
final markers = MarkerClient(OperationClient(client));

final created = await markers.create(
  label: 'Alpha',
  position: const GeoPoint(lat: 35.0, lon: -115.0, altM: 1200.0),
  topicId: 'topic-1',
);
final listed = await markers.list(topicId: 'topic-1', limit: 10);
final updated = await markers.updatePosition(
  markerId: created.markerId,
  expectedRevision: created.revision,
  position: const GeoPoint(lat: 36.0, lon: -116.0),
);

print('marker=${created.markerId} listed=${listed.markers.length} revision=${updated.revision}');
```

Typed attachment flow:

```dart
final attachments = AttachmentClient(OperationClient(client));

final stored = await attachments.store(
  name: 'sample.txt',
  contentType: 'text/plain',
  bytesBase64: 'aGVsbG8gd29ybGQ=',
  topicIds: const <String>['topic-1'],
);
final fetched = await attachments.get(stored.attachmentId);
final listed = await attachments.list(topicId: 'topic-1', limit: 10);
final associated = await attachments.associateTopic(
  attachmentId: stored.attachmentId,
  topicId: 'topic-2',
);

print('attachment=${fetched?.name} listed=${listed.attachments.length} associated=$associated');
```

Typed attachment streaming flow:

```dart
final attachments = AttachmentClient(OperationClient(client));
const checksum =
    '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c';

final session = await attachments.uploadStart(
  name: 'chunked.bin',
  contentType: 'application/octet-stream',
  totalSize: 11,
  checksumSha256: checksum,
);
final ack = await attachments.uploadChunk(
  uploadId: session.uploadId,
  offset: 0,
  bytesBase64: 'aGVsbG8gd29ybGQ=',
);
final committed = await attachments.uploadCommit(uploadId: session.uploadId);
final chunk = await attachments.downloadChunk(
  attachmentId: committed.attachmentId,
  offset: 0,
  maxBytes: 5,
);

print('upload=${session.uploadId} next=${ack.nextOffset} chunk=${chunk.bytesBase64}');
```

Typed discovery flow:

```dart
final discovery = DiscoveryClient(OperationClient(client));

final identities = await discovery.identityList();
final announced = await discovery.announceNow();
final presence = await discovery.presenceList(limit: 10);
final contacts = await discovery.contactList(limit: 10);
final directory = await discovery.peerDirectory(limit: 10);

print(
  'identities=${identities.length} announce=$announced '
  'presence=${presence.peers.length} contacts=${contacts.contacts.length} '
  'directory=${directory.length}',
);
```

Conversation-oriented flow:

```dart
final binding = RpcBinding(
  RpcConnectionOptions(
    endpoint: Uri.parse('http://127.0.0.1:4543/rpc'),
    pollIdleDelay: const Duration(milliseconds: 100),
  ),
);

final app = AppClient(binding);
final chat = ConversationClient(app);

await app.start(const Config(profile: Profile.desktopDefault));
final self = await chat.selfAddress();
final contacts = await app.contactList(limit: 10);
final history = await app.messageHistory();
final receipt = await chat.sendText('<peer-destination-hash>', 'hello');
final status = await app.deliveryStatus(receipt.messageId);

print('self: $self contacts=${contacts.contacts.length} history=${history.length}');
print('queued ${receipt.messageId} status=${status?.receiptStatus}');
```

If you plan to call identity or contact methods, request those capabilities at
startup:

```dart
await app.start(
  const Config(
    profile: Profile.desktopDefault,
    requestedCapabilities: <String>[
      'sdk.capability.identity_multi',
      'sdk.capability.contact_management',
    ],
  ),
);
```

Workspace flow helpers need the broader domain capabilities they compose, for
example:

```dart
await workspace.start(
  const Config(
    profile: Profile.desktopDefault,
    requestedCapabilities: <String>[
      'sdk.capability.identity_multi',
      'sdk.capability.identity_discovery',
      'sdk.capability.contact_management',
      'sdk.capability.topics',
      'sdk.capability.topic_fanout',
      'sdk.capability.markers',
      'sdk.capability.attachments',
    ],
  ),
);
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
dart run example/rpc_chat_smoke.dart http://127.0.0.1:4243/rpc <peer-destination-hash>
dart run example/rpc_operations_smoke.dart http://127.0.0.1:4243/rpc
dart run example/custom_operation_smoke.dart http://127.0.0.1:4243/rpc
dart run example/custom_vendor_command_smoke.dart http://127.0.0.1:4243/rpc
dart run example/voice_session_smoke.dart http://127.0.0.1:4243/rpc
dart run example/topic_operations_smoke.dart http://127.0.0.1:4243/rpc
dart run example/telemetry_operations_smoke.dart http://127.0.0.1:4243/rpc [topic-id]
dart run example/marker_operations_smoke.dart http://127.0.0.1:4243/rpc [topic-id]
dart run example/attachment_operations_smoke.dart http://127.0.0.1:4243/rpc [topic-id]
dart run example/attachment_streaming_smoke.dart http://127.0.0.1:4243/rpc
dart run example/discovery_operations_smoke.dart http://127.0.0.1:4243/rpc
dart run example/workspace_smoke.dart http://127.0.0.1:4243/rpc
dart run example/workspace_flows_smoke.dart http://127.0.0.1:4243/rpc
```

Repo-level smoke from the project root:

```sh
./tools/scripts/flutter-rpc-chat-smoke.sh <peer-destination-hash>
```

Minimal Flutter UI harness:

```sh
cd wrappers/flutter/lxmf_rpc_chat_app
flutter pub get
flutter run -d macos
```

This package is intentionally contract-first. The public package surface is now
backend-neutral while the implementation direction shifts to `reticulumd` RPC.
