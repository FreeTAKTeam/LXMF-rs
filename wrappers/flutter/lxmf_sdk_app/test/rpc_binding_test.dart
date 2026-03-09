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

    test('custom command helper roundtrips daemon envelope payload shape',
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
          final response = switch (method) {
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
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
                    'operation_id': 'vendor.example.custom',
                    'kind': 'result',
                    'accepted': true,
                    'payload': <String, Object?>{
                      'correlation_id': 'cmd-9',
                      'command': 'vendor.example.custom',
                      'target': 'node-b',
                      'echo': params['payload'],
                      'timeout_ms': params['timeout_ms'],
                    },
                    'extensions': <String, Object?>{'via': 'rpc-test'},
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

      final app = AppClient(
        RpcBinding(
          RpcConnectionOptions(
            endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
          ),
        ),
      );
      final commands = CustomCommandClient(OperationClient(app));

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
      expect(result.correlationId, 'cmd-9');
      expect(result.timeoutMs, 500);
      expect(result.echo['body'], 'hello');
      expect(result.extensions['via'], 'rpc-test');

      final envelopeCall = calls.singleWhere(
        (call) => call['method'] == 'sdk_envelope_execute_v2',
      );
      final callParams = envelopeCall['params'] as Map<String, Object?>;
      expect(callParams['operation_id'], 'vendor.example.custom');
      expect(callParams['target'], 'node-b');
      expect(callParams['timeout_ms'], 500);
    });

    test('voice session helper roundtrips canonical voice operations',
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
          final response = switch (method) {
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
                      <String, Object?>{
                        'id': 'app.voice.session.open',
                        'group': 'voice',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description':
                            'Open a voice signaling session for a peer.',
                        'aliases': <String>['sdk_voice_session_open_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.voice_signaling',
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.voice.session.update',
                        'group': 'voice',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description':
                            'Advance the state of a voice signaling session.',
                        'aliases': <String>['sdk_voice_session_update_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.voice_signaling',
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.voice.session.close',
                        'group': 'voice',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Close a voice signaling session.',
                        'aliases': <String>['sdk_voice_session_close_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.voice_signaling',
                        ],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': switch (params['operation_id']) {
                    'app.voice.session.open' => <String, Object?>{
                        'operation_id': 'app.voice.session.open',
                        'kind': 'result',
                        'accepted': true,
                        'payload': 'voice-1',
                        'extensions': <String, Object?>{},
                      },
                    'app.voice.session.update' => <String, Object?>{
                        'operation_id': 'app.voice.session.update',
                        'kind': 'result',
                        'accepted': true,
                        'payload': 'active',
                        'extensions': <String, Object?>{},
                      },
                    'app.voice.session.close' => <String, Object?>{
                        'operation_id': 'app.voice.session.close',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'session_id': 'voice-1',
                        },
                        'extensions': <String, Object?>{},
                      },
                    _ => <String, Object?>{},
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

      final voice = VoiceSessionClient(
        OperationClient(
          AppClient(
            RpcBinding(
              RpcConnectionOptions(
                endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
              ),
            ),
          ),
        ),
      );

      final sessionId = await voice.open(peerId: 'node-b', codecHint: 'opus');
      final nextState = await voice.update(
        sessionId: sessionId,
        state: VoiceSessionState.active,
      );
      final closed = await voice.close(sessionId);

      expect(sessionId, 'voice-1');
      expect(nextState, VoiceSessionState.active);
      expect(closed, isTrue);

      final envelopeCalls = calls
          .where((call) => call['method'] == 'sdk_envelope_execute_v2')
          .toList(growable: false);
      expect(
          envelopeCalls.map((call) =>
              (call['params'] as Map<String, Object?>)['operation_id']),
          [
            'app.voice.session.open',
            'app.voice.session.update',
            'app.voice.session.close',
          ]);
    });

    test('topic helper roundtrips canonical topic operations', () async {
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
          final response = switch (method) {
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
                      <String, Object?>{
                        'id': 'app.topic.create',
                        'group': 'topics',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Create topic.',
                        'aliases': <String>['sdk_topic_create_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.topics'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.topic.get',
                        'group': 'topics',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'Get topic.',
                        'aliases': <String>['sdk_topic_get_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.topics'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.topic.list',
                        'group': 'topics',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'List topics.',
                        'aliases': <String>['sdk_topic_list_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.topics'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.topic.subscribe',
                        'group': 'topics',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Subscribe topic.',
                        'aliases': <String>['sdk_topic_subscribe_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.topic_subscriptions',
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.topic.unsubscribe',
                        'group': 'topics',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Unsubscribe topic.',
                        'aliases': <String>['sdk_topic_unsubscribe_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.topic_subscriptions',
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.topic.publish',
                        'group': 'topics',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Publish topic.',
                        'aliases': <String>['sdk_topic_publish_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.topic_fanout'
                        ],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': switch (params['operation_id']) {
                    'app.topic.create' => <String, Object?>{
                        'operation_id': 'app.topic.create',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'topic_id': 'topic-1',
                          'topic_path': 'ops/alerts',
                          'created_ts_ms': 700,
                          'metadata': <String, Object?>{'kind': 'ops'},
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.topic.get' => <String, Object?>{
                        'operation_id': 'app.topic.get',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'topic_id': 'topic-1',
                          'topic_path': 'ops/alerts',
                          'created_ts_ms': 700,
                          'metadata': <String, Object?>{'kind': 'ops'},
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.topic.list' => <String, Object?>{
                        'operation_id': 'app.topic.list',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'topics': <Object?>[
                            <String, Object?>{
                              'topic_id': 'topic-1',
                              'topic_path': 'ops/alerts',
                              'created_ts_ms': 700,
                              'metadata': <String, Object?>{'kind': 'ops'},
                              'extensions': <String, Object?>{},
                            },
                          ],
                          'next_cursor': 'topic:1',
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.topic.subscribe' => <String, Object?>{
                        'operation_id': 'app.topic.subscribe',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'topic_id': 'topic-1'
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.topic.unsubscribe' => <String, Object?>{
                        'operation_id': 'app.topic.unsubscribe',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'topic_id': 'topic-1'
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.topic.publish' => <String, Object?>{
                        'operation_id': 'app.topic.publish',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{'accepted': true},
                        'extensions': <String, Object?>{},
                      },
                    _ => <String, Object?>{},
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

      final topics = TopicClient(
        OperationClient(
          AppClient(
            RpcBinding(
              RpcConnectionOptions(
                endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
              ),
            ),
          ),
        ),
      );

      final created = await topics.create(
        topicPath: 'ops/alerts',
        metadata: const <String, Object?>{'kind': 'ops'},
      );
      final fetched = await topics.get('topic-1');
      final listed = await topics.list(limit: 10);
      final subscribed = await topics.subscribe('topic-1');
      final published = await topics.publish(
        topicId: 'topic-1',
        payload: const <String, Object?>{'message': 'hello topic'},
        correlationId: 'topic-corr-1',
      );
      final unsubscribed = await topics.unsubscribe('topic-1');

      expect(created.topicId, 'topic-1');
      expect(fetched?.topicPath, 'ops/alerts');
      expect(listed.topics, hasLength(1));
      expect(subscribed, isTrue);
      expect(published, isTrue);
      expect(unsubscribed, isTrue);

      final envelopeCalls = calls
          .where((call) => call['method'] == 'sdk_envelope_execute_v2')
          .toList(growable: false);
      expect(
          envelopeCalls.map((call) =>
              (call['params'] as Map<String, Object?>)['operation_id']),
          [
            'app.topic.create',
            'app.topic.get',
            'app.topic.list',
            'app.topic.subscribe',
            'app.topic.publish',
            'app.topic.unsubscribe',
          ]);
    });

    test('telemetry helper roundtrips canonical telemetry operations',
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
          final response = switch (method) {
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
                      <String, Object?>{
                        'id': 'app.telemetry.query',
                        'group': 'telemetry',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'Query telemetry.',
                        'aliases': <String>['sdk_telemetry_query_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.telemetry_query',
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.telemetry.subscribe',
                        'group': 'telemetry',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Subscribe telemetry.',
                        'aliases': <String>['sdk_telemetry_subscribe_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.telemetry_stream',
                        ],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': switch (params['operation_id']) {
                    'app.telemetry.query' => <String, Object?>{
                        'operation_id': 'app.telemetry.query',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <Object?>[
                          <String, Object?>{
                            'ts_ms': 900,
                            'key': 'topic_publish',
                            'value': <String, Object?>{
                              'message': 'hello topic'
                            },
                            'unit': null,
                            'tags': <String, Object?>{
                              'topic_id': 'topic-1',
                              'peer_id': 'node-b',
                            },
                            'extensions': <String, Object?>{},
                          },
                        ],
                        'extensions': <String, Object?>{},
                      },
                    'app.telemetry.subscribe' => <String, Object?>{
                        'operation_id': 'app.telemetry.subscribe',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{'accepted': true},
                        'extensions': <String, Object?>{},
                      },
                    _ => <String, Object?>{},
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

      final telemetry = TelemetryClient(
        OperationClient(
          AppClient(
            RpcBinding(
              RpcConnectionOptions(
                endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
              ),
            ),
          ),
        ),
      );

      final points = await telemetry.query(
        topicId: 'topic-1',
        peerId: 'node-b',
        fromTsMs: 100,
        limit: 10,
      );
      final subscribed = await telemetry.subscribe(
        topicId: 'topic-1',
        fromTsMs: 100,
        limit: 20,
      );

      expect(points, hasLength(1));
      expect(points.first.key, 'topic_publish');
      expect(points.first.tags['peer_id'], 'node-b');
      expect(subscribed, isTrue);

      final envelopeCalls = calls
          .where((call) => call['method'] == 'sdk_envelope_execute_v2')
          .toList(growable: false);
      expect(
          envelopeCalls.map((call) =>
              (call['params'] as Map<String, Object?>)['operation_id']),
          [
            'app.telemetry.query',
            'app.telemetry.subscribe',
          ]);
    });

    test('marker helper roundtrips canonical marker operations', () async {
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
          final response = switch (method) {
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
                      <String, Object?>{
                        'id': 'app.marker.create',
                        'group': 'markers',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Create marker.',
                        'aliases': <String>['sdk_marker_create_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.markers'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.marker.list',
                        'group': 'markers',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'List markers.',
                        'aliases': <String>['sdk_marker_list_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.markers'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.marker.update_position',
                        'group': 'markers',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Update marker position.',
                        'aliases': <String>['sdk_marker_update_position_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.markers'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.marker.delete',
                        'group': 'markers',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Delete marker.',
                        'aliases': <String>['sdk_marker_delete_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.markers'
                        ],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': switch (params['operation_id']) {
                    'app.marker.create' => <String, Object?>{
                        'operation_id': 'app.marker.create',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'marker_id': 'marker-1',
                          'label': 'Alpha',
                          'position': <String, Object?>{
                            'lat': 35.0,
                            'lon': -115.0,
                            'alt_m': 1200.0,
                          },
                          'topic_id': 'topic-1',
                          'revision': 1,
                          'updated_ts_ms': 950,
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.marker.list' => <String, Object?>{
                        'operation_id': 'app.marker.list',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'markers': <Object?>[
                            <String, Object?>{
                              'marker_id': 'marker-1',
                              'label': 'Alpha',
                              'position': <String, Object?>{
                                'lat': 35.0,
                                'lon': -115.0,
                                'alt_m': 1200.0,
                              },
                              'topic_id': 'topic-1',
                              'revision': 1,
                              'updated_ts_ms': 950,
                              'extensions': <String, Object?>{},
                            },
                          ],
                          'next_cursor': null,
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.marker.update_position' => <String, Object?>{
                        'operation_id': 'app.marker.update_position',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'marker_id': 'marker-1',
                          'label': 'Alpha',
                          'position': <String, Object?>{
                            'lat': 36.0,
                            'lon': -116.0,
                            'alt_m': null,
                          },
                          'topic_id': 'topic-1',
                          'revision': 2,
                          'updated_ts_ms': 970,
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.marker.delete' => <String, Object?>{
                        'operation_id': 'app.marker.delete',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'marker_id': 'marker-1'
                        },
                        'extensions': <String, Object?>{},
                      },
                    _ => <String, Object?>{},
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

      final markers = MarkerClient(
        OperationClient(
          AppClient(
            RpcBinding(
              RpcConnectionOptions(
                endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
              ),
            ),
          ),
        ),
      );

      final created = await markers.create(
        label: 'Alpha',
        position: const GeoPoint(lat: 35.0, lon: -115.0, altM: 1200.0),
        topicId: 'topic-1',
      );
      final listed = await markers.list(topicId: 'topic-1', limit: 10);
      final updated = await markers.updatePosition(
        markerId: 'marker-1',
        expectedRevision: 1,
        position: const GeoPoint(lat: 36.0, lon: -116.0),
      );
      final deleted = await markers.delete(
        markerId: 'marker-1',
        expectedRevision: 2,
      );

      expect(created.markerId, 'marker-1');
      expect(listed.markers, hasLength(1));
      expect(updated.position.lat, 36.0);
      expect(updated.revision, 2);
      expect(deleted, isTrue);

      final envelopeCalls = calls
          .where((call) => call['method'] == 'sdk_envelope_execute_v2')
          .toList(growable: false);
      expect(
          envelopeCalls.map((call) =>
              (call['params'] as Map<String, Object?>)['operation_id']),
          [
            'app.marker.create',
            'app.marker.list',
            'app.marker.update_position',
            'app.marker.delete',
          ]);
    });

    test('attachment helper roundtrips canonical attachment operations',
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
          final response = switch (method) {
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
                      <String, Object?>{
                        'id': 'app.attachment.store',
                        'group': 'attachments',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Store attachment.',
                        'aliases': <String>['sdk_attachment_store_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachments'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.attachment.get',
                        'group': 'attachments',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'Get attachment.',
                        'aliases': <String>['sdk_attachment_get_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachments'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.attachment.list',
                        'group': 'attachments',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'List attachments.',
                        'aliases': <String>['sdk_attachment_list_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachments'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.attachment.associate_topic',
                        'group': 'attachments',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Associate attachment topic.',
                        'aliases': <String>[
                          'sdk_attachment_associate_topic_v2'
                        ],
                        'required_capabilities': <String>[
                          'sdk.capability.attachments'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.attachment.delete',
                        'group': 'attachments',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Delete attachment.',
                        'aliases': <String>['sdk_attachment_delete_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachment_delete'
                        ],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': switch (params['operation_id']) {
                    'app.attachment.store' => <String, Object?>{
                        'operation_id': 'app.attachment.store',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'attachment_id': 'attachment-1',
                          'name': 'sample.txt',
                          'content_type': 'text/plain',
                          'byte_len': 11,
                          'checksum_sha256':
                              '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c',
                          'created_ts_ms': 650,
                          'expires_ts_ms': null,
                          'topic_ids': <String>['topic-1'],
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.attachment.get' => <String, Object?>{
                        'operation_id': 'app.attachment.get',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'attachment_id': 'attachment-1',
                          'name': 'sample.txt',
                          'content_type': 'text/plain',
                          'byte_len': 11,
                          'checksum_sha256':
                              '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c',
                          'created_ts_ms': 651,
                          'expires_ts_ms': null,
                          'topic_ids': <String>['topic-1'],
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.attachment.list' => <String, Object?>{
                        'operation_id': 'app.attachment.list',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'attachments': <Object?>[
                            <String, Object?>{
                              'attachment_id': 'attachment-1',
                              'name': 'sample.txt',
                              'content_type': 'text/plain',
                              'byte_len': 11,
                              'checksum_sha256':
                                  '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c',
                              'created_ts_ms': 652,
                              'expires_ts_ms': null,
                              'topic_ids': <String>['topic-1'],
                              'extensions': <String, Object?>{},
                            },
                          ],
                          'next_cursor': null,
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.attachment.associate_topic' => <String, Object?>{
                        'operation_id': 'app.attachment.associate_topic',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'attachment_id': 'attachment-1',
                          'topic_id': 'topic-2',
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.attachment.delete' => <String, Object?>{
                        'operation_id': 'app.attachment.delete',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'attachment_id': 'attachment-1',
                        },
                        'extensions': <String, Object?>{},
                      },
                    _ => <String, Object?>{},
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

      final attachments = AttachmentClient(
        OperationClient(
          AppClient(
            RpcBinding(
              RpcConnectionOptions(
                endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
              ),
            ),
          ),
        ),
      );

      final stored = await attachments.store(
        name: 'sample.txt',
        contentType: 'text/plain',
        bytesBase64: 'aGVsbG8gd29ybGQ=',
        topicIds: const <String>['topic-1'],
      );
      final fetched = await attachments.get('attachment-1');
      final listed = await attachments.list(topicId: 'topic-1', limit: 10);
      final associated = await attachments.associateTopic(
        attachmentId: 'attachment-1',
        topicId: 'topic-2',
      );
      final deleted = await attachments.delete('attachment-1');

      expect(stored.attachmentId, 'attachment-1');
      expect(fetched?.name, 'sample.txt');
      expect(listed.attachments, hasLength(1));
      expect(associated, isTrue);
      expect(deleted, isTrue);

      final envelopeCalls = calls
          .where((call) => call['method'] == 'sdk_envelope_execute_v2')
          .toList(growable: false);
      expect(
          envelopeCalls.map((call) =>
              (call['params'] as Map<String, Object?>)['operation_id']),
          [
            'app.attachment.store',
            'app.attachment.get',
            'app.attachment.list',
            'app.attachment.associate_topic',
            'app.attachment.delete',
          ]);
    });

    test('attachment helper roundtrips canonical streaming operations',
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
          final response = switch (method) {
            'sdk_operation_registry_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'registry': <String, Object?>{
                    'entries': <Object?>[
                      <String, Object?>{
                        'id': 'app.attachment.upload_start',
                        'group': 'attachments',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Upload start.',
                        'aliases': <String>['sdk_attachment_upload_start_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachment_streaming'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.attachment.upload_chunk',
                        'group': 'attachments',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Upload chunk.',
                        'aliases': <String>['sdk_attachment_upload_chunk_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachment_streaming'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.attachment.upload_commit',
                        'group': 'attachments',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Upload commit.',
                        'aliases': <String>['sdk_attachment_upload_commit_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachment_streaming'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.attachment.download_chunk',
                        'group': 'attachments',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'Download chunk.',
                        'aliases': <String>['sdk_attachment_download_chunk_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.attachment_streaming'
                        ],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': switch (params['operation_id']) {
                    'app.attachment.upload_start' => <String, Object?>{
                        'operation_id': 'app.attachment.upload_start',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'upload_id': 'upload-1',
                          'attachment_id': 'attachment-2',
                          'chunk_size_hint': 65536,
                          'next_offset': 0,
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.attachment.upload_chunk' => <String, Object?>{
                        'operation_id': 'app.attachment.upload_chunk',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'next_offset': 5,
                          'complete': false,
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.attachment.upload_commit' => <String, Object?>{
                        'operation_id': 'app.attachment.upload_commit',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'attachment_id': 'attachment-2',
                          'name': 'chunked.bin',
                          'content_type': 'application/octet-stream',
                          'byte_len': 11,
                          'checksum_sha256':
                              '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c',
                          'created_ts_ms': 653,
                          'topic_ids': <String>['topic-1'],
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.attachment.download_chunk' => <String, Object?>{
                        'operation_id': 'app.attachment.download_chunk',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'attachment_id': 'attachment-2',
                          'offset': 0,
                          'next_offset': 5,
                          'total_size': 11,
                          'done': false,
                          'checksum_sha256':
                              '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c',
                          'bytes_base64': 'aGVsbG8=',
                        },
                        'extensions': <String, Object?>{},
                      },
                    _ => <String, Object?>{},
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

      final attachments = AttachmentClient(
        OperationClient(
          AppClient(
            RpcBinding(
              RpcConnectionOptions(
                endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
              ),
            ),
          ),
        ),
      );

      final session = await attachments.uploadStart(
        name: 'chunked.bin',
        contentType: 'application/octet-stream',
        totalSize: 11,
        checksumSha256:
            '64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c',
      );
      final ack = await attachments.uploadChunk(
        uploadId: session.uploadId,
        offset: 0,
        bytesBase64: 'aGVsbG8=',
      );
      final committed =
          await attachments.uploadCommit(uploadId: session.uploadId);
      final downloaded = await attachments.downloadChunk(
        attachmentId: committed.attachmentId,
        offset: 0,
        maxBytes: 5,
      );

      expect(session.uploadId, 'upload-1');
      expect(ack.nextOffset, 5);
      expect(committed.attachmentId, 'attachment-2');
      expect(downloaded.bytesBase64, 'aGVsbG8=');

      final envelopeCalls = calls
          .where((call) => call['method'] == 'sdk_envelope_execute_v2')
          .toList(growable: false);
      expect(
          envelopeCalls.map((call) =>
              (call['params'] as Map<String, Object?>)['operation_id']),
          [
            'app.attachment.upload_start',
            'app.attachment.upload_chunk',
            'app.attachment.upload_commit',
            'app.attachment.download_chunk',
          ]);
    });

    test('discovery helper roundtrips canonical discovery operations',
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
          final params = frame['params'] is Map<String, Object?>
              ? frame['params'] as Map<String, Object?>
              : const <String, Object?>{};
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
                        'description': 'List identities.',
                        'aliases': <String>['sdk_identity_list_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.identity_multi'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.identity.announce',
                        'group': 'identity',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Announce identity.',
                        'aliases': <String>['sdk_identity_announce_now_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.identity_discovery'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.identity.presence.list',
                        'group': 'identity',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'List presence.',
                        'aliases': <String>['sdk_identity_presence_list_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.identity_discovery'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.contact.list',
                        'group': 'identity',
                        'kind': 'query',
                        'transport_variant': 'rpc',
                        'description': 'List contacts.',
                        'aliases': <String>['sdk_identity_contact_list_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.contact_management'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.contact.update',
                        'group': 'identity',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Update contact.',
                        'aliases': <String>['sdk_identity_contact_update_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.contact_management'
                        ],
                      },
                      <String, Object?>{
                        'id': 'app.identity.bootstrap',
                        'group': 'identity',
                        'kind': 'command',
                        'transport_variant': 'rpc',
                        'description': 'Bootstrap identity.',
                        'aliases': <String>['sdk_identity_bootstrap_v2'],
                        'required_capabilities': <String>[
                          'sdk.capability.contact_management'
                        ],
                      },
                    ],
                  },
                },
                'error': null,
              },
            'sdk_envelope_execute_v2' => <String, Object?>{
                'id': id,
                'result': <String, Object?>{
                  'response': switch (params['operation_id']) {
                    'app.identity.list' => <String, Object?>{
                        'operation_id': 'app.identity.list',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <Object?>[
                          <String, Object?>{
                            'identity': 'alice',
                            'public_key': 'pubkey',
                            'display_name': 'Alice',
                            'capabilities': <String>['chat'],
                            'extensions': <String, Object?>{},
                          },
                        ],
                        'extensions': <String, Object?>{},
                      },
                    'app.identity.announce' => <String, Object?>{
                        'operation_id': 'app.identity.announce',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'accepted': true,
                          'announce_id': 42,
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.identity.presence.list' => <String, Object?>{
                        'operation_id': 'app.identity.presence.list',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'peers': <Object?>[
                            <String, Object?>{
                              'peer_id': 'bob',
                              'last_seen_ts_ms': 200,
                              'first_seen_ts_ms': 120,
                              'seen_count': 3,
                              'name': 'Bob Relay',
                              'name_source': 'announce',
                              'trust_level': 'trusted',
                              'bootstrap': true,
                              'extensions': <String, Object?>{},
                            },
                          ],
                          'next_cursor': null,
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.contact.list' => <String, Object?>{
                        'operation_id': 'app.contact.list',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'contacts': <Object?>[
                            <String, Object?>{
                              'identity': 'bob',
                              'display_name': 'Bob',
                              'trust_level': 'trusted',
                              'bootstrap': true,
                              'updated_ts_ms': 500,
                              'metadata': <String, Object?>{
                                'nickname': 'relay'
                              },
                              'extensions': <String, Object?>{},
                            },
                          ],
                          'next_cursor': null,
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.contact.update' => <String, Object?>{
                        'operation_id': 'app.contact.update',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'identity': 'charlie',
                          'display_name': 'Charlie',
                          'trust_level': 'trusted',
                          'bootstrap': true,
                          'updated_ts_ms': 501,
                          'metadata': <String, Object?>{'team': 'ops'},
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    'app.identity.bootstrap' => <String, Object?>{
                        'operation_id': 'app.identity.bootstrap',
                        'kind': 'result',
                        'accepted': true,
                        'payload': <String, Object?>{
                          'identity': 'delta',
                          'display_name': null,
                          'trust_level': 'trusted',
                          'bootstrap': true,
                          'updated_ts_ms': 600,
                          'metadata': <String, Object?>{},
                          'extensions': <String, Object?>{},
                        },
                        'extensions': <String, Object?>{},
                      },
                    _ => <String, Object?>{},
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

      final discovery = DiscoveryClient(
        OperationClient(
          AppClient(
            RpcBinding(
              RpcConnectionOptions(
                endpoint: Uri.parse('http://127.0.0.1:${server.port}/rpc'),
              ),
            ),
          ),
        ),
      );

      final identities = await discovery.identityList();
      final announced = await discovery.announceNow();
      final presence = await discovery.presenceList(limit: 10);
      final contacts = await discovery.contactList(limit: 10);
      final updated = await discovery.updateContact(
        identity: 'charlie',
        displayName: 'Charlie',
        trustLevel: TrustLevel.trusted,
        bootstrap: true,
        metadata: const <String, Object?>{'team': 'ops'},
      );
      final bootstrapped = await discovery.bootstrapIdentity(identity: 'delta');

      expect(identities.single.identity, 'alice');
      expect(announced, isTrue);
      expect(presence.peers.single.peerId, 'bob');
      expect(contacts.contacts.single.identity, 'bob');
      expect(updated.identity, 'charlie');
      expect(bootstrapped.identity, 'delta');

      final envelopeCalls = calls
          .where((call) => call['method'] == 'sdk_envelope_execute_v2')
          .toList(growable: false);
      expect(
          envelopeCalls.map((call) =>
              (call['params'] as Map<String, Object?>)['operation_id']),
          [
            'app.identity.list',
            'app.identity.announce',
            'app.identity.presence.list',
            'app.contact.list',
            'app.contact.update',
            'app.identity.bootstrap',
          ]);
    });
  });
}
