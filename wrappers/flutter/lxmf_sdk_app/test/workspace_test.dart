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

  test('WorkspaceFlows bootstrap peers create topics and publish field notes',
      () async {
    final binding = _FakeWorkspaceBinding();
    final workspace = WorkspaceClient.fromBinding(binding);

    final existingPeer = await workspace.flows.ensurePeerReady('peer-known');
    final bootstrappedPeer = await workspace.flows.ensurePeerReady('peer-new');
    final existingTopic = await workspace.flows.ensureTopic('ops/alerts');
    final createdTopic = await workspace.flows.ensureTopic('ops/notes');
    final note = await workspace.flows.publishFieldNote(
      topicPath: 'ops/notes',
      payload: const <String, Object?>{'body': 'observe'},
      correlationId: 'field-note-1',
      markerLabel: 'Alpha',
      markerPosition: const GeoPoint(lat: 35.0, lon: -115.0),
      attachment: const AttachmentDraft(
        name: 'note.txt',
        contentType: 'text/plain',
        bytesBase64: 'aGVsbG8=',
      ),
    );

    expect(existingPeer.wasCreated, isFalse);
    expect(existingPeer.contact.identity, 'peer-known');
    expect(existingPeer.announced, isTrue);

    expect(bootstrappedPeer.wasCreated, isTrue);
    expect(bootstrappedPeer.contact.identity, 'peer-new');

    expect(existingTopic.wasCreated, isFalse);
    expect(existingTopic.topic.topicId, 'topic-existing');

    expect(createdTopic.wasCreated, isTrue);
    expect(createdTopic.topic.topicPath, 'ops/notes');

    expect(note.published, isTrue);
    expect(note.topic.topicId, 'topic-created');
    expect(note.marker?.markerId, 'marker-1');
    expect(note.attachment?.attachmentId, 'attachment-1');
  });

  test('WorkspaceFlows discover peers and prepare conversations', () async {
    final binding = _FakeWorkspaceBinding();
    final workspace = WorkspaceClient.fromBinding(binding);

    final allPeers = await workspace.flows.discoverPeers(limit: 10);
    final onlinePeers =
        await workspace.flows.discoverPeers(limit: 10, onlineOnly: true);
    final ready = await workspace.flows.ensureConversation('peer-known');
    final sent = await workspace.flows.sendConversationText(
      'peer-new',
      'hello there',
      correlationId: 'conv-1',
    );

    expect(allPeers, hasLength(2));
    expect(onlinePeers.map((peer) => peer.peerId), contains('peer-known'));
    expect(onlinePeers.map((peer) => peer.peerId),
        isNot(contains('peer-offline')));

    expect(ready.peer.identity, 'peer-known');
    expect(ready.snapshot.selfAddress, 'self-1');
    expect(ready.snapshot.peerAddress, 'peer-known');
    expect(ready.snapshot.messages.single.content, 'previous hello');

    expect(sent.peer.identity, 'peer-new');
    expect(sent.receipt.messageId, 'msg-1');
  });

  test('WorkspaceFlows discoverPeers filters before applying the limit',
      () async {
    final binding = _FilteredPeerWorkspaceBinding();
    final workspace = WorkspaceClient.fromBinding(binding);

    final peers =
        await workspace.flows.discoverPeers(limit: 1, bootstrapOnly: true);

    expect(peers, hasLength(1));
    expect(peers.single.peerId, 'peer-bootstrap');
  });

  test('WorkspaceFlows sync topics and publish attachment reports', () async {
    final binding = _FakeWorkspaceBinding();
    final workspace = WorkspaceClient.fromBinding(binding);

    final sync = await workspace.flows.ensureTopicSync(
      'ops/alerts',
      telemetryLimit: 10,
    );
    final report = await workspace.flows.publishAttachmentReport(
      topicPath: 'ops/reports',
      attachment: const AttachmentDraft(
        name: 'report.txt',
        contentType: 'text/plain',
        bytesBase64: 'cmVwb3J0',
      ),
      summaryPayload: const <String, Object?>{'title': 'daily report'},
      correlationId: 'report-1',
    );

    expect(sync.topic.topicId, 'topic-existing');
    expect(sync.wasCreated, isFalse);
    expect(sync.subscribed, isTrue);
    expect(sync.telemetry.single.key, 'battery');

    expect(report.topic.topicId, 'topic-created');
    expect(report.attachment.attachmentId, 'attachment-1');
    expect(report.published, isTrue);
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
        OperationEntry(
          id: 'app.identity.announce',
          group: 'identity',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Announce identity.',
        ),
        OperationEntry(
          id: 'app.contact.list',
          group: 'identity',
          kind: OperationKind.query,
          transportVariant: TransportVariant.rpc,
          description: 'List contacts.',
        ),
        OperationEntry(
          id: 'app.identity.bootstrap',
          group: 'identity',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Bootstrap identity.',
        ),
        OperationEntry(
          id: 'app.contact.update',
          group: 'identity',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Update contact.',
        ),
        OperationEntry(
          id: 'app.topic.list',
          group: 'topics',
          kind: OperationKind.query,
          transportVariant: TransportVariant.rpc,
          description: 'List topics.',
        ),
        OperationEntry(
          id: 'app.identity.presence.list',
          group: 'identity',
          kind: OperationKind.query,
          transportVariant: TransportVariant.rpc,
          description: 'List peer presence.',
        ),
        OperationEntry(
          id: 'app.message.history.list',
          group: 'messaging',
          kind: OperationKind.query,
          transportVariant: TransportVariant.rpc,
          description: 'List message history.',
        ),
        OperationEntry(
          id: 'app.topic.create',
          group: 'topics',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Create topic.',
        ),
        OperationEntry(
          id: 'app.topic.publish',
          group: 'topics',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Publish topic payload.',
        ),
        OperationEntry(
          id: 'app.topic.subscribe',
          group: 'topics',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Subscribe to topic.',
        ),
        OperationEntry(
          id: 'app.telemetry.query',
          group: 'telemetry',
          kind: OperationKind.query,
          transportVariant: TransportVariant.rpc,
          description: 'Query telemetry.',
        ),
        OperationEntry(
          id: 'app.marker.create',
          group: 'markers',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Create marker.',
        ),
        OperationEntry(
          id: 'app.attachment.store',
          group: 'attachments',
          kind: OperationKind.command,
          transportVariant: TransportVariant.rpc,
          description: 'Store attachment.',
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
      'app.identity.announce' => const EnvelopeResponse(
          operationId: 'app.identity.announce',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true},
        ),
      'app.contact.list' => EnvelopeResponse(
          operationId: 'app.contact.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: switch (
              (envelope.payload as Map<Object?, Object?>)['cursor']) {
            'after-known' => const <String, Object?>{
                'contacts': <Object?>[],
                'next_cursor': null,
              },
            _ => const <String, Object?>{
                'contacts': <Object?>[
                  <String, Object?>{
                    'identity': 'peer-known',
                    'display_name': 'Known Peer',
                    'trust_level': 'trusted',
                    'bootstrap': true,
                    'updated_ts_ms': 10,
                    'metadata': <String, Object?>{},
                    'extensions': <String, Object?>{},
                  },
                  <String, Object?>{
                    'identity': 'peer-offline',
                    'display_name': 'Offline Peer',
                    'trust_level': 'unknown',
                    'bootstrap': false,
                    'updated_ts_ms': 11,
                    'metadata': <String, Object?>{},
                    'extensions': <String, Object?>{},
                  },
                ],
                'next_cursor': 'after-known',
              },
          },
        ),
      'app.identity.bootstrap' => EnvelopeResponse(
          operationId: 'app.identity.bootstrap',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'identity': (envelope.payload as Map<Object?, Object?>)['identity'],
            'display_name': null,
            'trust_level': 'trusted',
            'bootstrap': true,
            'updated_ts_ms': 20,
            'metadata': <String, Object?>{},
            'extensions': <String, Object?>{},
          },
        ),
      'app.identity.presence.list' => const EnvelopeResponse(
          operationId: 'app.identity.presence.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'peers': <Object?>[
              <String, Object?>{
                'peer_id': 'peer-known',
                'last_seen_ts_ms': 200,
                'first_seen_ts_ms': 100,
                'seen_count': 4,
                'name': 'Known Peer',
                'name_source': 'announce',
                'trust_level': 'trusted',
                'bootstrap': true,
                'extensions': <String, Object?>{},
              },
            ],
            'next_cursor': null,
          },
        ),
      'app.message.history.list' => EnvelopeResponse(
          operationId: 'app.message.history.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'messages': <Object?>[
              <String, Object?>{
                'id': 'msg-prev',
                'source': 'peer-known',
                'destination': 'self-1',
                'title': '',
                'content': 'previous hello',
                'timestamp': 1710000000,
                'direction': 'in',
                'fields': null,
                'receipt_status': null,
              },
            ],
          },
        ),
      'app.contact.update' => EnvelopeResponse(
          operationId: 'app.contact.update',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'identity': (envelope.payload as Map<Object?, Object?>)['identity'],
            'display_name':
                (envelope.payload as Map<Object?, Object?>)['display_name'],
            'trust_level':
                (envelope.payload as Map<Object?, Object?>)['trust_level'] ??
                    'trusted',
            'bootstrap':
                (envelope.payload as Map<Object?, Object?>)['bootstrap'] ??
                    false,
            'updated_ts_ms': 30,
            'metadata':
                (envelope.payload as Map<Object?, Object?>)['metadata'] ??
                    const <String, Object?>{},
            'extensions': <String, Object?>{},
          },
        ),
      'app.topic.list' => EnvelopeResponse(
          operationId: 'app.topic.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: switch (
              (envelope.payload as Map<Object?, Object?>)['cursor']) {
            'after-existing' => const <String, Object?>{
                'topics': <Object?>[],
                'next_cursor': null,
              },
            _ => const <String, Object?>{
                'topics': <Object?>[
                  <String, Object?>{
                    'topic_id': 'topic-existing',
                    'topic_path': 'ops/alerts',
                    'created_ts_ms': 100,
                    'metadata': <String, Object?>{},
                    'extensions': <String, Object?>{},
                  },
                ],
                'next_cursor': 'after-existing',
              },
          },
        ),
      'app.topic.create' => EnvelopeResponse(
          operationId: 'app.topic.create',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'topic_id': 'topic-created',
            'topic_path':
                (envelope.payload as Map<Object?, Object?>)['topic_path'],
            'created_ts_ms': 200,
            'metadata':
                (envelope.payload as Map<Object?, Object?>)['metadata'] ??
                    const <String, Object?>{},
            'extensions': <String, Object?>{},
          },
        ),
      'app.topic.publish' => const EnvelopeResponse(
          operationId: 'app.topic.publish',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true},
        ),
      'app.topic.subscribe' => const EnvelopeResponse(
          operationId: 'app.topic.subscribe',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{'accepted': true},
        ),
      'app.telemetry.query' => const EnvelopeResponse(
          operationId: 'app.telemetry.query',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <Object?>[
            <String, Object?>{
              'ts_ms': 1000,
              'key': 'battery',
              'value': 87,
              'unit': 'percent',
              'tags': <String, Object?>{'peer': 'self-1'},
              'extensions': <String, Object?>{},
            },
          ],
        ),
      'app.marker.create' => EnvelopeResponse(
          operationId: 'app.marker.create',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'marker_id': 'marker-1',
            'label': (envelope.payload as Map<Object?, Object?>)['label'],
            'position': (envelope.payload as Map<Object?, Object?>)['position'],
            'topic_id': (envelope.payload as Map<Object?, Object?>)['topic_id'],
            'revision': 1,
            'updated_ts_ms': 300,
            'extensions': <String, Object?>{},
          },
        ),
      'app.attachment.store' => EnvelopeResponse(
          operationId: 'app.attachment.store',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'attachment_id': 'attachment-1',
            'name': (envelope.payload as Map<Object?, Object?>)['name'],
            'content_type':
                (envelope.payload as Map<Object?, Object?>)['content_type'],
            'byte_len': 5,
            'checksum_sha256': 'checksum',
            'created_ts_ms': 400,
            'topic_ids':
                (envelope.payload as Map<Object?, Object?>)['topic_ids'],
            'extensions': <String, Object?>{},
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

class _FilteredPeerWorkspaceBinding extends _FakeWorkspaceBinding {
  @override
  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) async {
    switch (envelope.operationId) {
      case 'app.contact.list':
        return EnvelopeResponse(
          operationId: 'app.contact.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: switch ((envelope.payload as Map<Object?, Object?>)['cursor']) {
            'after-page-1' => const <String, Object?>{
                'contacts': <Object?>[
                  <String, Object?>{
                    'identity': 'peer-bootstrap',
                    'display_name': 'Bootstrap Peer',
                    'trust_level': 'trusted',
                    'bootstrap': true,
                    'updated_ts_ms': 20,
                    'metadata': <String, Object?>{},
                    'extensions': <String, Object?>{},
                  },
                ],
                'next_cursor': null,
              },
            _ => const <String, Object?>{
                'contacts': <Object?>[
                  <String, Object?>{
                    'identity': 'peer-non-bootstrap',
                    'display_name': 'Non Bootstrap Peer',
                    'trust_level': 'unknown',
                    'bootstrap': false,
                    'updated_ts_ms': 10,
                    'metadata': <String, Object?>{},
                    'extensions': <String, Object?>{},
                  },
                ],
                'next_cursor': 'after-page-1',
              },
          },
        );
      case 'app.identity.presence.list':
        return const EnvelopeResponse(
          operationId: 'app.identity.presence.list',
          kind: EnvelopeKind.result,
          accepted: true,
          payload: <String, Object?>{
            'peers': <Object?>[],
            'next_cursor': null,
          },
        );
      default:
        return super.executeEnvelope(envelope);
    }
  }
}
