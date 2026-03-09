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

  Envelope? lastQueryEnvelope;
  Envelope? lastCommandEnvelope;

  @override
  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) async {
    switch (envelope.kind) {
      case EnvelopeKind.query:
        lastQueryEnvelope = envelope;
        return queryResponse;
      case EnvelopeKind.command:
        lastCommandEnvelope = envelope;
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
