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

    test('voice session helper maps typed open update and close flows',
        () async {
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
      expect(
          binding.commandEnvelopes[1].operationId, 'app.voice.session.update');
      expect(
          binding.commandEnvelopes[2].operationId, 'app.voice.session.close');
    });

    test('topic helper maps typed create list and publish flows', () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'app.topic.create',
              group: 'topics',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Create topic.',
              aliases: <String>['sdk_topic_create_v2'],
            ),
            OperationEntry(
              id: 'app.topic.get',
              group: 'topics',
              kind: OperationKind.query,
              transportVariant: TransportVariant.rpc,
              description: 'Get topic.',
              aliases: <String>['sdk_topic_get_v2'],
            ),
            OperationEntry(
              id: 'app.topic.list',
              group: 'topics',
              kind: OperationKind.query,
              transportVariant: TransportVariant.rpc,
              description: 'List topics.',
              aliases: <String>['sdk_topic_list_v2'],
            ),
            OperationEntry(
              id: 'app.topic.subscribe',
              group: 'topics',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Subscribe topic.',
              aliases: <String>['sdk_topic_subscribe_v2'],
            ),
            OperationEntry(
              id: 'app.topic.unsubscribe',
              group: 'topics',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Unsubscribe topic.',
              aliases: <String>['sdk_topic_unsubscribe_v2'],
            ),
            OperationEntry(
              id: 'app.topic.publish',
              group: 'topics',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Publish topic.',
              aliases: <String>['sdk_topic_publish_v2'],
            ),
          ],
        ),
      );
      binding.commandResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.topic.create',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'topic_id': 'topic-1',
            'topic_path': 'ops/alerts',
            'created_ts_ms': 700,
            'metadata': <String, Object?>{'kind': 'ops'},
            'extensions': <String, Object?>{},
          },
        ),
        const EnvelopeResponse(
          operationId: 'app.topic.subscribe',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true, 'topic_id': 'topic-1'},
        ),
        const EnvelopeResponse(
          operationId: 'app.topic.publish',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true},
        ),
        const EnvelopeResponse(
          operationId: 'app.topic.unsubscribe',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true, 'topic_id': 'topic-1'},
        ),
      ];
      binding.queryResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.topic.get',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'topic_id': 'topic-1',
            'topic_path': 'ops/alerts',
            'created_ts_ms': 700,
            'metadata': <String, Object?>{'kind': 'ops'},
            'extensions': <String, Object?>{},
          },
        ),
        const EnvelopeResponse(
          operationId: 'app.topic.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
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
        ),
      ];

      final topics = TopicClient(OperationClient(AppClient(binding)));
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
      expect(listed.nextCursor, 'topic:1');
      expect(subscribed, isTrue);
      expect(published, isTrue);
      expect(unsubscribed, isTrue);
      expect(binding.commandEnvelopes[0].operationId, 'app.topic.create');
      expect(binding.queryEnvelopes[0].operationId, 'app.topic.get');
      expect(binding.queryEnvelopes[1].operationId, 'app.topic.list');
      expect(binding.commandEnvelopes[1].operationId, 'app.topic.subscribe');
      expect(binding.commandEnvelopes[2].operationId, 'app.topic.publish');
      expect(binding.commandEnvelopes[3].operationId, 'app.topic.unsubscribe');
    });

    test('telemetry helper maps typed query and subscribe flows', () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'app.telemetry.query',
              group: 'telemetry',
              kind: OperationKind.query,
              transportVariant: TransportVariant.rpc,
              description: 'Query telemetry.',
              aliases: <String>['sdk_telemetry_query_v2'],
            ),
            OperationEntry(
              id: 'app.telemetry.subscribe',
              group: 'telemetry',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Subscribe telemetry.',
              aliases: <String>['sdk_telemetry_subscribe_v2'],
            ),
          ],
        ),
      );
      binding.queryResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.telemetry.query',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <Object?>[
            <String, Object?>{
              'ts_ms': 900,
              'key': 'topic_publish',
              'value': <String, Object?>{'message': 'hello topic'},
              'unit': null,
              'tags': <String, Object?>{
                'topic_id': 'topic-1',
                'peer_id': 'node-b',
              },
              'extensions': <String, Object?>{},
            },
          ],
        ),
      ];
      binding.commandResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.telemetry.subscribe',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true},
        ),
      ];

      final telemetry = TelemetryClient(OperationClient(AppClient(binding)));
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
      expect(points.first.tags['topic_id'], 'topic-1');
      expect(subscribed, isTrue);
      expect(binding.queryEnvelopes[0].operationId, 'app.telemetry.query');
      expect(
          binding.commandEnvelopes[0].operationId, 'app.telemetry.subscribe');
    });

    test('marker helper maps typed create list update and delete flows',
        () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'app.marker.create',
              group: 'markers',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Create marker.',
              aliases: <String>['sdk_marker_create_v2'],
            ),
            OperationEntry(
              id: 'app.marker.list',
              group: 'markers',
              kind: OperationKind.query,
              transportVariant: TransportVariant.rpc,
              description: 'List markers.',
              aliases: <String>['sdk_marker_list_v2'],
            ),
            OperationEntry(
              id: 'app.marker.update_position',
              group: 'markers',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Update marker position.',
              aliases: <String>['sdk_marker_update_position_v2'],
            ),
            OperationEntry(
              id: 'app.marker.delete',
              group: 'markers',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Delete marker.',
              aliases: <String>['sdk_marker_delete_v2'],
            ),
          ],
        ),
      );
      binding.commandResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.marker.create',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
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
        ),
        const EnvelopeResponse(
          operationId: 'app.marker.update_position',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
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
        ),
        const EnvelopeResponse(
          operationId: 'app.marker.delete',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true, 'marker_id': 'marker-1'},
        ),
      ];
      binding.queryResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.marker.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
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
        ),
      ];

      final markers = MarkerClient(OperationClient(AppClient(binding)));
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
      expect(binding.commandEnvelopes[0].operationId, 'app.marker.create');
      expect(binding.queryEnvelopes[0].operationId, 'app.marker.list');
      expect(binding.commandEnvelopes[1].operationId,
          'app.marker.update_position');
      expect(binding.commandEnvelopes[2].operationId, 'app.marker.delete');
    });

    test(
        'attachment helper maps typed store get list associate and delete flows',
        () async {
      final binding = _FakeBinding(
        registry: OperationRegistry(
          entries: const <OperationEntry>[
            OperationEntry(
              id: 'app.attachment.store',
              group: 'attachments',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Store attachment.',
              aliases: <String>['sdk_attachment_store_v2'],
            ),
            OperationEntry(
              id: 'app.attachment.get',
              group: 'attachments',
              kind: OperationKind.query,
              transportVariant: TransportVariant.rpc,
              description: 'Get attachment.',
              aliases: <String>['sdk_attachment_get_v2'],
            ),
            OperationEntry(
              id: 'app.attachment.list',
              group: 'attachments',
              kind: OperationKind.query,
              transportVariant: TransportVariant.rpc,
              description: 'List attachments.',
              aliases: <String>['sdk_attachment_list_v2'],
            ),
            OperationEntry(
              id: 'app.attachment.associate_topic',
              group: 'attachments',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Associate attachment topic.',
              aliases: <String>['sdk_attachment_associate_topic_v2'],
            ),
            OperationEntry(
              id: 'app.attachment.delete',
              group: 'attachments',
              kind: OperationKind.command,
              transportVariant: TransportVariant.rpc,
              description: 'Delete attachment.',
              aliases: <String>['sdk_attachment_delete_v2'],
            ),
          ],
        ),
      );
      binding.commandResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.attachment.store',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
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
        ),
        const EnvelopeResponse(
          operationId: 'app.attachment.associate_topic',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true},
        ),
        const EnvelopeResponse(
          operationId: 'app.attachment.delete',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true},
        ),
      ];
      binding.queryResponses = <EnvelopeResponse>[
        const EnvelopeResponse(
          operationId: 'app.attachment.get',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
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
        ),
        const EnvelopeResponse(
          operationId: 'app.attachment.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
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
            'next_cursor': 'attachment:1',
          },
        ),
      ];

      final attachments = AttachmentClient(OperationClient(AppClient(binding)));
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
      expect(listed.nextCursor, 'attachment:1');
      expect(associated, isTrue);
      expect(deleted, isTrue);
      expect(binding.commandEnvelopes[0].operationId, 'app.attachment.store');
      expect(binding.queryEnvelopes[0].operationId, 'app.attachment.get');
      expect(binding.queryEnvelopes[1].operationId, 'app.attachment.list');
      expect(binding.commandEnvelopes[1].operationId,
          'app.attachment.associate_topic');
      expect(binding.commandEnvelopes[2].operationId, 'app.attachment.delete');
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
  List<EnvelopeResponse> queryResponses = <EnvelopeResponse>[];
  List<EnvelopeResponse> commandResponses = <EnvelopeResponse>[];

  Envelope? lastQueryEnvelope;
  Envelope? lastCommandEnvelope;
  final List<Envelope> queryEnvelopes = <Envelope>[];
  final List<Envelope> commandEnvelopes = <Envelope>[];

  @override
  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) async {
    switch (envelope.kind) {
      case EnvelopeKind.query:
        lastQueryEnvelope = envelope;
        queryEnvelopes.add(envelope);
        if (queryResponses.isNotEmpty) {
          return queryResponses.removeAt(0);
        }
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
