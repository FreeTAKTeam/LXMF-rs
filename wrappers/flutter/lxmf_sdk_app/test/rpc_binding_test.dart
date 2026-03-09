import 'dart:async';
import 'dart:io';

import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';
import 'package:lxmf_sdk_app/src/rpc/codec.dart';
import 'package:test/test.dart';

void main() {
  group('RpcBinding', () {
    late HttpServer server;
    late List<Map<String, Object?>> calls;

    setUp(() async {
      calls = <Map<String, Object?>>[];
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    });

    tearDown(() async {
      await server.close(force: true);
    });

    test('start, status, send, subscribe, and stop map onto rpc methods', () async {
      final outboundEvent = <String, Object?>{
        'event_id': 'evt-2',
        'runtime_id': 'rpc-test-runtime',
        'stream_id': 'sdk-events',
        'seq_no': 2,
        'contract_version': 2,
        'ts_ms': 1710000000000,
        'event_type': 'outbound',
        'severity': 'info',
        'source_component': 'rns-rpc',
        'payload': <String, Object?>{
          'message': <String, Object?>{
            'id': 'msg-1',
            'receipt_status': 'sent: direct',
          },
        },
      };

      unawaited(() async {
        await for (final request in server) {
          final body = await request.fold<List<int>>(<int>[], (all, chunk) {
            all.addAll(chunk);
            return all;
          });
          final frame = decodeRpcFrame(body);
          calls.add(frame);
          final id = frame['id'] as int;
          final method = frame['method'] as String;
          final response = switch (method) {
            'sdk_negotiate_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-test-runtime',
                  'active_contract_version': 2,
                  'effective_capabilities': <String>[
                    'sdk.capability.cursor_replay',
                    'sdk.capability.async_events',
                  ],
                  'effective_limits': <String, Object?>{'max_poll_events': 64},
                },
                'error': null,
              },
            'sdk_snapshot_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-test-runtime',
                  'state': 'running',
                  'config_revision': 1,
                  'event_stream_position': 2,
                  'queued_messages': 1,
                  'in_flight_messages': 0,
                },
                'error': null,
              },
            'sdk_configure_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{'accepted': true, 'revision': 1},
                'error': null,
              },
            'sdk_send_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{'message_id': 'msg-1'},
                'error': null,
              },
            'sdk_poll_events_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-test-runtime',
                  'stream_id': 'sdk-events',
                  'events': <Object?>[outboundEvent],
                  'next_cursor': 'cursor-2',
                  'dropped_count': 0,
                },
                'error': null,
              },
            'sdk_shutdown_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{'accepted': true, 'mode': 'graceful'},
                'error': null,
              },
            _ => <String, Object?>{
                'id': id,
                'result': null,
                'error': <String, Object?>{
                  'code': 'SDK_VALIDATION_INVALID_ARGUMENT',
                  'message': 'unknown method',
                },
              },
          };
          request.response.headers.contentType = ContentType('application', 'msgpack');
          request.response.add(encodeRpcFrame(response));
          await request.response.close();
        }
      }());

      final client = AppClient(
        RpcBinding(
          RpcConnectionOptions(
            endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
            pollIdleDelay: const Duration(milliseconds: 5),
          ),
        ),
      );

      final handle = await client.start(
        const Config(profile: Profile.desktopDefault, eventBatchSize: 32),
      );
      expect(handle.runtimeId, 'rpc-test-runtime');
      expect(handle.capabilities.activeContractVersion, 2);

      final status = await client.status();
      expect(status.state, RunState.running);
      expect(status.queuedMessages, 1);

      final receipt = await client.send(
        const SendRequest(source: 'src', destination: 'dst', payload: 'hello'),
      );
      expect(receipt.messageId, 'msg-1');

      final event = await client.subscribeEvents().first.timeout(
            const Duration(seconds: 1),
          );
      expect(event.kind, EventKind.messageSent);
      expect(event.metadata.messageId, 'msg-1');
      expect(event.metadata.occurredAtMs, 1710000000000);

      await client.stop();

      final methods = calls.map((call) => call['method']).toList();
      expect(
        methods,
        containsAll(<Object?>[
          'sdk_negotiate_v2',
          'sdk_snapshot_v2',
          'sdk_configure_v2',
          'sdk_send_v2',
          'sdk_poll_events_v2',
          'sdk_shutdown_v2',
        ]),
      );
    });

    test('embedded profile is rejected by the rpc binding', () async {
      final client = AppClient(
        RpcBinding(
          RpcConnectionOptions(
            endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
          ),
        ),
      );

      expect(
        () => client.start(const Config(profile: Profile.embeddedDefault)),
        throwsA(
          isA<AppError>().having(
            (error) => error.code,
            'code',
            ErrorCode.capabilityUnsupportedProfile,
          ),
        ),
      );
    });

    test('embedded transport config is rejected by the rpc binding', () async {
      final client = AppClient(
        RpcBinding(
          RpcConnectionOptions(
            endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
          ),
        ),
      );

      expect(
        () => client.start(
          const Config(
            profile: Profile.desktopDefault,
            transportMode: TransportMode.tcpClient,
            tcpHost: '127.0.0.1',
            tcpPort: 4242,
          ),
        ),
        throwsA(
          isA<AppError>().having(
            (error) => error.code,
            'code',
            ErrorCode.configInvalid,
          ),
        ),
      );
    });

    test('subscribeEvents closes when stop is called', () async {
      unawaited(() async {
        await for (final request in server) {
          final body = await request.fold<List<int>>(<int>[], (all, chunk) {
            all.addAll(chunk);
            return all;
          });
          final frame = decodeRpcFrame(body);
          final id = frame['id'] as int;
          final method = frame['method'] as String;
          final response = switch (method) {
            'sdk_negotiate_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-test-runtime',
                  'active_contract_version': 2,
                  'effective_capabilities': <String>['sdk.capability.async_events'],
                  'effective_limits': <String, Object?>{'max_poll_events': 64},
                },
                'error': null,
              },
            'sdk_snapshot_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-test-runtime',
                  'state': 'running',
                  'config_revision': 1,
                  'event_stream_position': 0,
                  'queued_messages': 0,
                  'in_flight_messages': 0,
                },
                'error': null,
              },
            'sdk_shutdown_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{'accepted': true, 'mode': 'graceful'},
                'error': null,
              },
            'sdk_poll_events_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-test-runtime',
                  'stream_id': 'sdk-events',
                  'events': <Object?>[],
                  'next_cursor': 'cursor-0',
                  'dropped_count': 0,
                },
                'error': null,
              },
            _ => <String, Object?>{
                'id': id,
                'result': <String, Object?>{},
                'error': null,
              },
          };
          request.response.headers.contentType = ContentType('application', 'msgpack');
          request.response.add(encodeRpcFrame(response));
          await request.response.close();
        }
      }());

      final client = AppClient(
        RpcBinding(
          RpcConnectionOptions(
            endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
            pollIdleDelay: const Duration(milliseconds: 5),
          ),
        ),
      );

      await client.start(const Config(profile: Profile.testingDefault));
      final done = client.subscribeEvents().drain<void>();
      await Future<void>.delayed(const Duration(milliseconds: 20));
      await client.stop();
      await expectLater(done.timeout(const Duration(seconds: 1)), completes);
    });
  });
}
