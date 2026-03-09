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

    test('start, status, send, subscribe, and stop map onto rpc methods',
        () async {
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
                'result': <String, Object?>{
                  'accepted': true,
                  'mode': 'graceful'
                },
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
          request.response.headers.contentType =
              ContentType('application', 'msgpack');
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

    test('identity, contact, and delivery status helpers decode rpc payloads',
        () async {
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
            'sdk_identity_list_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'identities': <Object?>[
                    <String, Object?>{
                      'identity': 'id-1',
                      'public_key': 'pub-1',
                      'display_name': 'Primary',
                      'capabilities': <String>['chat'],
                      'extensions': <String, Object?>{},
                    },
                  ],
                },
                'error': null,
              },
            'sdk_identity_contact_list_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'contact_list': <String, Object?>{
                    'contacts': <Object?>[
                      <String, Object?>{
                        'identity': 'peer-1',
                        'display_name': 'Peer One',
                        'trust_level': 'trusted',
                        'bootstrap': true,
                        'updated_ts_ms': 1710000000000,
                        'metadata': <String, Object?>{'nickname': 'p1'},
                        'extensions': <String, Object?>{},
                      },
                    ],
                    'next_cursor': 'contact:1',
                  },
                },
                'error': null,
              },
            'list_messages' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'messages': <Object?>[
                    <String, Object?>{
                      'id': 'msg-7',
                      'source': 'self-1',
                      'destination': 'peer-1',
                      'title': '',
                      'content': 'hello',
                      'timestamp': 1710000000,
                      'direction': 'out',
                      'fields': <String, Object?>{},
                      'receipt_status': 'sent: direct',
                    },
                  ],
                },
                'error': null,
              },
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
            'sdk_status_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'message': <String, Object?>{
                    'id': 'msg-7',
                    'source': 'self-1',
                    'destination': 'peer-1',
                    'title': '',
                    'content': 'hello',
                    'timestamp': 1710000000,
                    'direction': 'out',
                    'fields': <String, Object?>{},
                    'receipt_status': 'sent: direct',
                  },
                },
                'error': null,
              },
            'sdk_poll_events_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'runtime_id': 'rpc-test-runtime',
                  'stream_id': 'sdk-events',
                  'events': <Object?>[
                    <String, Object?>{
                      'event_id': 'evt-delivered',
                      'runtime_id': 'rpc-test-runtime',
                      'stream_id': 'sdk-events',
                      'seq_no': 3,
                      'contract_version': 2,
                      'ts_ms': 1710000002000,
                      'event_type': 'outbound',
                      'severity': 'info',
                      'source_component': 'rns-rpc',
                      'payload': <String, Object?>{
                        'message': <String, Object?>{
                          'id': 'msg-7',
                          'source': 'self-1',
                          'destination': 'peer-1',
                          'title': '',
                          'content': 'hello',
                          'timestamp': 1710000002,
                          'direction': 'out',
                          'fields': <String, Object?>{},
                          'receipt_status': 'delivered',
                        },
                      },
                    },
                  ],
                  'next_cursor': 'cursor-1',
                  'dropped_count': 0,
                },
                'error': null,
              },
            'sdk_shutdown_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'accepted': true,
                  'mode': 'graceful'
                },
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
          request.response.headers.contentType =
              ContentType('application', 'msgpack');
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
      final client = AppClient(binding);

      await client.start(const Config(profile: Profile.testingDefault));

      final identities = await binding.identityList();
      expect(identities, hasLength(1));
      expect(identities.first.displayName, 'Primary');

      final contacts = await binding.contactList(limit: 10);
      expect(contacts.contacts, hasLength(1));
      expect(contacts.contacts.first.trustLevel, TrustLevel.trusted);
      expect(contacts.nextCursor, 'contact:1');

      final messages = await binding.messageHistory();
      expect(messages, hasLength(1));
      expect(messages.first.id, 'msg-7');
      expect(messages.first.destination, 'peer-1');

      final initial = await binding.deliveryStatus('msg-7');
      expect(initial, isNotNull);
      expect(initial!.receiptStatus, 'sent: direct');
      expect(initial.isTerminal, isFalse);

      final watched = await binding.watchMessageStatus('msg-7').last.timeout(
            const Duration(seconds: 1),
          );
      expect(watched.receiptStatus, 'delivered');
      expect(watched.isTerminal, isTrue);

      await client.stop();
    });

    test(
        'operation registry and envelope execution roundtrip through rpc helpers',
        () async {
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
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
                      <String, Object?>{
                        'id': 'app.identity.list',
                        'group': 'identity',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description':
                            'List identities visible to the runtime.',
                        'aliases': <String>['sdk_identity_list_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.identity_multi',
                        ],
                      },
                      <String, Object?>{
                        'id': 'vendor.example.custom',
                        'group': 'vendor',
                        'kind': 'command',
                        'transport_variant': 'extension',
                        'description': 'Custom extension operation.',
                        'aliases': <String>['vendor.alias'],
                        'required_capabilities': <String>[],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': <String, Object?>{
                    'operation_id': 'app.identity.list',
                    'kind': 'result',
                    'accepted': true,
                    'correlation_id': 'corr-1',
                    'payload': <Object?>[
                      <String, Object?>{
                        'identity': 'id-1',
                        'public_key': 'pub-1',
                      },
                    ],
                    'extensions': <String, Object?>{'source': 'rpc-test'},
                  },
                },
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
          request.response.headers.contentType =
              ContentType('application', 'msgpack');
          request.response.add(encodeRpcFrame(response));
          await request.response.close();
        }
      }());

      final client = AppClient(
        RpcBinding(
          RpcConnectionOptions(
            endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
          ),
        ),
      );

      final registry = await client.operationRegistry();
      expect(registry.entries, hasLength(2));
      expect(registry.supports('sdk_identity_list_v2'), isTrue);
      expect(
          registry.canonicalize('sdk_identity_list_v2'), 'app.identity.list');
      expect(
        registry.resolve('vendor.alias')!.entry.transportFamily,
        TransportFamily.extension,
      );
      expect(registry.entriesByGroup()['identity'], hasLength(1));

      final response = await client.queryOperation(
        'sdk_identity_list_v2',
        const <String, Object?>{},
        correlationId: 'corr-1',
      );
      expect(response.operationId, 'app.identity.list');
      expect(response.kind, EnvelopeKind.result);
      expect(response.accepted, isTrue);
      expect(response.correlationId, 'corr-1');
      expect(response.payload, isA<List<Object?>>());
      expect(response.extensions['source'], 'rpc-test');

      final envelopeCall = calls.singleWhere(
        (call) => call['method'] == 'sdk_envelope_execute_v2',
      );
      final params = envelopeCall['params'] as Map<String, Object?>;
      expect(params['operation_id'], 'sdk_identity_list_v2');
      expect(params['kind'], 'query');
      expect(params['correlation_id'], 'corr-1');
    });
  });
}
