import 'dart:async';

import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';
import 'package:test/test.dart';

void main() {
  test('WorkspaceClient composes family clients over a single app binding',
      () async {
    final binding = _FakeWorkspaceBinding();
    final workspace = WorkspaceClient.fromBinding(binding);

    final handle = await workspace.start(
      const Config(profile: Profile.testingDefault),
    );
    final status = await workspace.status();
    final receipt = await workspace.send(
      const SendRequest(
        source: 'self-1',
        destination: 'peer-1',
        payload: 'hello',
      ),
    );
    final identities = await workspace.discovery.identityList();
    final selfAddress = await workspace.conversations.selfAddress();

    expect(handle.runtimeId, 'workspace-runtime');
    expect(status.state, RunState.running);
    expect(receipt.messageId, 'msg-1');
    expect(identities.single.identity, 'self-1');
    expect(selfAddress, 'self-1');
    expect(workspace.commands, isA<CustomCommandClient>());
    expect(workspace.topics, isA<TopicClient>());
    expect(workspace.telemetry, isA<TelemetryClient>());
    expect(workspace.markers, isA<MarkerClient>());
    expect(workspace.attachments, isA<AttachmentClient>());
    expect(workspace.voice, isA<VoiceSessionClient>());
  });
}

class _FakeWorkspaceBinding implements AppBinding {
  @override
  Future<Handle> start(Config config) async {
    return Handle(
      runtimeId: 'workspace-runtime',
      profile: config.profile,
      capabilities: const CapabilitySummary(
        activeContractVersion: 2,
        effectiveCapabilities: <String>[],
        effectiveLimits: <String, Object?>{},
      ),
    );
  }

  @override
  Future<void> stop() async {}

  @override
  Future<RuntimeStatus> status() async {
    return const RuntimeStatus(
      state: RunState.running,
      runtimeId: 'workspace-runtime',
      profile: Profile.testingDefault,
      capabilities: CapabilitySummary(
        activeContractVersion: 2,
        effectiveCapabilities: <String>[],
        effectiveLimits: <String, Object?>{},
      ),
    );
  }

  @override
  Future<SendReceipt> send(SendRequest request) async {
    return const SendReceipt(
      runtimeId: 'workspace-runtime',
      messageId: 'msg-1',
      profile: Profile.testingDefault,
    );
  }

  @override
  Future<SendReport> sendWithProfileDefaults(SendRequest request) async {
    return SendReport(
      receipt: await send(request),
      attempts: const <DeliveryAttempt>[],
      totalDelayMs: 0,
      plan: Profile.testingDefault.defaults(),
    );
  }

  @override
  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) async {
    return sendWithProfileDefaults(request);
  }

  @override
  Future<OperationRegistry> operationRegistry() async {
    return OperationRegistry(
      entries: const <OperationEntry>[
        OperationEntry(
          id: 'app.identity.list',
          group: 'identity',
          kind: OperationKind.query,
          transportVariant: TransportVariant.rpc,
          description: 'List identities.',
        ),
        OperationEntry(
          id: 'app.delivery.destination_hash',
          group: 'identity',
          kind: OperationKind.query,
          transportVariant: TransportVariant.rpc,
          description: 'Resolve self address.',
        ),
      ],
    );
  }

  @override
  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) async {
    return switch (envelope.operationId) {
      'app.identity.list' => const EnvelopeResponse(
          operationId: 'app.identity.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <Object?>[
            <String, Object?>{
              'identity': 'self-1',
              'public_key': 'pub',
              'display_name': 'Self',
              'capabilities': <String>[],
              'extensions': <String, Object?>{},
            },
          ],
        ),
      'app.delivery.destination_hash' => const EnvelopeResponse(
          operationId: 'app.delivery.destination_hash',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'identity_hash': 'id-1',
            'delivery_destination_hash': 'self-1',
            'running': true,
          },
        ),
      _ => EnvelopeResponse(
          operationId: envelope.operationId,
          kind: EnvelopeKind.error,
          accepted: false,
          payload: const <String, Object?>{'message': 'unsupported'},
        ),
    };
  }

  @override
  Stream<AppEvent> subscribeEvents() => const Stream<AppEvent>.empty();
}
