import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';
import 'package:test/test.dart';

void main() {
  group('OperationClient', () {
    test('resolves aliases and decodes query results', () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: <OperationEntry>[
            const OperationEntry(
              id: 'app.identity.list',
              group: 'identity',
              kind: OperationKind.query,
              transportVariant: TransportVariant.rpc,
              description: 'List identities.',
              aliases: <String>['sdk_identity_list_v2'],
            ),
          ],
        ),
        queryResponse: const EnvelopeResponse(
          operationId: 'app.identity.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <Object?>['id-1', 'id-2'],
          correlationId: 'corr-1',
        ),
      );

      final client = OperationClient(AppClient(binding));
      final result = await client.query<List<String>>(
        OperationCall<List<String>>(
          operationId: 'sdk_identity_list_v2',
          payload: const <String, Object?>{},
          correlationId: 'corr-1',
          decode: (payload) => (payload as List<Object?>)
              .map((item) => item.toString())
              .toList(),
        ),
      );

      expect(result.operationId, 'app.identity.list');
      expect(result.alias, 'sdk_identity_list_v2');
      expect(result.payload, <String>['id-1', 'id-2']);
      expect(binding.lastQueryEnvelope?.operationId, 'app.identity.list');
      expect(binding.lastQueryEnvelope?.correlationId, 'corr-1');
    });

    test('rejects kind mismatches before dispatch', () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'vendor.example.custom',
              group: 'vendor',
              kind: OperationKind.command,
              transportVariant: TransportVariant.extension,
              description: 'Custom command.',
            ),
          ],
        ),
      );

      final client = OperationClient(AppClient(binding));

      await expectLater(
        () => client.query<String>(
          OperationCall<String>(
            operationId: 'vendor.example.custom',
            payload: const <String, Object?>{'body': 'hello'},
            decode: (payload) => payload.toString(),
          ),
        ),
        throwsA(
          isA<AppError>().having(
            (error) => error.code,
            'code',
            ErrorCode.validationInvalidArgument,
          ),
        ),
      );
    });

    test('dispatches commands with typed payload decoding', () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'vendor.example.custom',
              group: 'vendor',
              kind: OperationKind.command,
              transportVariant: TransportVariant.extension,
              description: 'Custom command.',
              aliases: <String>['vendor.alias'],
            ),
          ],
        ),
        commandResponse: const EnvelopeResponse(
          operationId: 'vendor.example.custom',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'ok': true, 'ticket': '1234'},
          correlationId: 'cmd-1',
          extensions: <String, Object?>{'source': 'test'},
        ),
      );

      final client = OperationClient(AppClient(binding));
      final result = await client.command<Map<String, Object?>>(
        OperationCall<Map<String, Object?>>(
          operationId: 'vendor.alias',
          payload: const <String, Object?>{'body': 'hello'},
          correlationId: 'cmd-1',
          decode: (payload) => (payload as Map<Object?, Object?>).map(
            (key, value) => MapEntry(key.toString(), value),
          ),
        ),
      );

      expect(result.operationId, 'vendor.example.custom');
      expect(result.alias, 'vendor.alias');
      expect(result.accepted, isTrue);
      expect(result.payload['ticket'], '1234');
      expect(result.extensions['source'], 'test');
      expect(binding.lastCommandEnvelope?.operationId, 'vendor.example.custom');
      expect(binding.lastCommandEnvelope?.correlationId, 'cmd-1');
    });

    test('custom command helper decodes daemon echo payloads', () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'vendor.example.custom',
              group: 'vendor',
              kind: OperationKind.command,
              transportVariant: TransportVariant.extension,
              description: 'Custom command.',
              aliases: <String>['vendor.alias'],
            ),
          ],
        ),
        commandResponse: const EnvelopeResponse(
          operationId: 'vendor.example.custom',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'correlation_id': 'cmd-42',
            'command': 'vendor.example.custom',
            'target': 'node-b',
            'echo': <String, Object?>{'body': 'hello'},
            'timeout_ms': 500,
          },
          extensions: <String, Object?>{'via': 'rpc'},
        ),
      );

      final commands = CustomCommandClient(OperationClient(AppClient(binding)));
      final result = await commands.invoke<Map<String, Object?>>(
        CustomCommandCall<Map<String, Object?>>(
          operationId: 'vendor.alias',
          target: 'node-b',
          timeoutMs: 500,
          payload: const <String, Object?>{'body': 'hello'},
          decodeEcho: (payload) => (payload as Map<Object?, Object?>).map(
            (key, value) => MapEntry(key.toString(), value),
          ),
        ),
      );

      expect(result.operationId, 'vendor.example.custom');
      expect(result.alias, 'vendor.alias');
      expect(result.command, 'vendor.example.custom');
      expect(result.target, 'node-b');
      expect(result.correlationId, 'cmd-42');
      expect(result.timeoutMs, 500);
      expect(result.echo['body'], 'hello');
      expect(result.extensions['via'], 'rpc');
    });

    test('voice session helper maps typed open update and close flows', () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'app.voice.session.open',
              group: 'voice',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Open voice session.',
              aliases: <String>['sdk_voice_session_open_v2'],
            ),
            OperationEntry(
              id: 'app.voice.session.update',
              group: 'voice',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Update voice session.',
              aliases: <String>['sdk_voice_session_update_v2'],
            ),
            OperationEntry(
              id: 'app.voice.session.close',
              group: 'voice',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Close voice session.',
              aliases: <String>['sdk_voice_session_close_v2'],
            ),
          ],
        ),
      );
      binding.commandResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.voice.session.open',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: 'voice-1',
        ),
        const EnvelopeResponse(
          operationId: 'app.voice.session.update',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: 'active',
        ),
        const EnvelopeResponse(
          operationId: 'app.voice.session.close',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true, 'session_id': 'voice-1'},
        ),
      ];

      final voice = VoiceSessionClient(OperationClient(AppClient(binding)));
      final sessionId = await voice.open(peerId: 'node-b', codecHint: 'opus');
      final nextState = await voice.update(
        sessionId: sessionId,
        state: VoiceSessionState.active,
      );
      final closed = await voice.close(sessionId);

      expect(sessionId, 'voice-1');
      expect(nextState, VoiceSessionState.active);
      expect(closed, isTrue);
      expect(binding.commandEnvelopes[0].operationId, 'app.voice.session.open');
      expect(binding.commandEnvelopes[1].operationId, 'app.voice.session.update');
      expect(binding.commandEnvelopes[2].operationId, 'app.voice.session.close');
    });
  });
}

final class _FakeBinding implements AppBinding {
  _FakeBinding({
    required this.registry,
    this.queryResponse = const EnvelopeResponse(
      operationId: 'noop',
      kind: EnvelopeKind.result,
      accepted: true,
      payload: null,
    ),
    this.commandResponse = const EnvelopeResponse(
      operationId: 'noop',
      kind: EnvelopeKind.result,
      accepted: true,
      payload: null,
    ),
  });

  final OperationRegistry registry;
  final EnvelopeResponse queryResponse;
  final EnvelopeResponse commandResponse;
  List<EnvelopeResponse> commandResponses = <EnvelopeResponse>[];

  Envelope? lastQueryEnvelope;
  Envelope? lastCommandEnvelope;
  final List<Envelope> commandEnvelopes = <Envelope>[];

  @override
  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) async {
    switch (envelope.kind) {
      case EnvelopeKind.query:
        lastQueryEnvelope = envelope;
        return queryResponse;
      case EnvelopeKind.command:
        lastCommandEnvelope = envelope;
        commandEnvelopes.add(envelope);
        if (commandResponses.isNotEmpty) {
          return commandResponses.removeAt(0);
        }
        return commandResponse;
      case EnvelopeKind.result:
      case EnvelopeKind.error:
        throw UnimplementedError();
    }
  }

  @override
  Future<OperationRegistry> operationRegistry() async => registry;

  @override
  Future<Handle> start(Config config) {
    throw UnimplementedError();
  }

  @override
  Future<RuntimeStatus> status() {
    throw UnimplementedError();
  }

  @override
  Future<void> stop() async {}

  @override
  Stream<AppEvent> subscribeEvents() => const Stream<AppEvent>.empty();

  @override
  Future<SendReceipt> send(SendRequest request) {
    throw UnimplementedError();
  }

  @override
  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) {
    throw UnimplementedError();
  }

  @override
  Future<SendReport> sendWithProfileDefaults(SendRequest request) {
    throw UnimplementedError();
  }
}
