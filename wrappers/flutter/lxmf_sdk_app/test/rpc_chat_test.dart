import 'dart:async';
import 'dart:io';

import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';
import 'package:lxmf_sdk_app/src/rpc/codec.dart';
import 'package:test/test.dart';

void main() {
  group('RpcConversationClient', () {
    late HttpServer server;
    late List<Map<String, Object?>> calls;

    setUp(() async {
      calls = <Map<String, Object?>>[];
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    });

    tearDown(() async {
      await server.close(force: true);
    });

    test('loads history, resolves self address, and streams inbound updates', () async {
      final outboundEvent = <String, Object?>{
        'event_id': 'evt-3',
        'runtime_id': 'rpc-chat-runtime',
        'stream_id': 'sdk-events',
        'seq_no': 3,
        'contract_version': 2,
        'ts_ms': 1710000003000,
        'event_type': 'inbound',
        'severity': 'info',
        'source_component': 'rns-rpc',
        'payload': <String, Object?>{
          'message': <String, Object?>{
            'id': 'msg-2',
            'source': 'peer-1',
            'destination': 'self-1',
            'title': '',
            'content': 'reply',
            'timestamp': 1710000003,
            'direction': 'in',
            'fields': null,
            'receipt_status': null,
          },
        },
      };

      var pollCount = 0;
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
                  'runtime_id': 'rpc-chat-runtime',
                  'active_contract_version': 2,
                  'effective_capabilities': <String>['sdk.capability.async_events'],
                  'effective_limits': <String, Object?>{'max_poll_events': 64},
                },
                'error': null,
              },
            'sdk_snapshot_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-chat-runtime',
                  'state': 'running',
                  'config_revision': 1,
                  'event_stream_position': 2,
                  'queued_messages': 0,
                  'in_flight_messages': 0,
                },
                'error': null,
              },
            'status' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'identity_hash': 'identity-1',
                  'delivery_destination_hash': 'self-1',
                  'running': true,
                },
                'error': null,
              },
            'list_messages' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'messages': <Object?>[
                    <String, Object?>{
                      'id': 'msg-0',
                      'source': 'self-1',
                      'destination': 'peer-1',
                      'title': '',
                      'content': 'hello',
                      'timestamp': 1710000000,
                      'direction': 'out',
                      'fields': null,
                      'receipt_status': 'sent: direct',
                    },
                  ],
                },
                'error': null,
              },
            'sdk_poll_events_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-chat-runtime',
                  'stream_id': 'sdk-events',
                  'events': pollCount++ == 0 ? <Object?>[outboundEvent] : <Object?>[],
                  'next_cursor': 'cursor-$pollCount',
                  'dropped_count': 0,
                },
                'error': null,
              },
            'sdk_send_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{'message_id': 'msg-send'},
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

      final binding = RpcBinding(
        RpcConnectionOptions(
          endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
          pollIdleDelay: const Duration(milliseconds: 5),
        ),
      );
      final app = AppClient(binding);
      final chat = RpcConversationClient(binding);

      await app.start(const Config(profile: Profile.testingDefault));
      expect(await chat.selfAddress(), 'self-1');

      final snapshot = await chat.loadConversation('peer-1');
      expect(snapshot.messages, hasLength(1));
      expect(snapshot.messages.first.content, 'hello');
      expect(snapshot.messages.first.direction, ChatDirection.outbound);

      final update = await chat.watchConversation('peer-1').skip(1).first.timeout(
            const Duration(seconds: 1),
          );
      expect(update.appendedMessage, isNotNull);
      expect(update.appendedMessage!.content, 'reply');
      expect(update.appendedMessage!.direction, ChatDirection.inbound);

      final receipt = await chat.sendText('peer-1', 'new message');
      expect(receipt.messageId, 'msg-send');

      await app.stop();

      final methods = calls.map((call) => call['method']).toList();
      expect(methods, containsAll(<Object?>['status', 'list_messages', 'sdk_send_v2']));
    });

    test('surfaces initial load failures as stream errors', () async {
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
            'status' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'identity_hash': 'identity-1',
                  'delivery_destination_hash': 'self-1',
                  'running': true,
                },
                'error': null,
              },
            'list_messages' => <String, Object?>{
                'id': id,
                'result': null,
                'error': <String, Object?>{
                  'code': 'SDK_VALIDATION_INVALID_ARGUMENT',
                  'message': 'history unavailable',
                },
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

      final binding = RpcBinding(
        RpcConnectionOptions(
          endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
          pollIdleDelay: const Duration(milliseconds: 5),
        ),
      );
      final chat = RpcConversationClient(binding);

      await expectLater(
        chat.watchConversation('peer-1'),
        emitsError(isA<AppError>()),
      );
    });
  });
}
