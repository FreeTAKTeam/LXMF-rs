import 'client.dart';
import 'models.dart';
import 'operations.dart';
import 'rpc/binding.dart';
import 'rpc/chat.dart';

class PeerReadyResult {
  const PeerReadyResult({
    required this.identity,
    required this.contact,
    required this.wasCreated,
    required this.announced,
  });

  final String identity;
  final ContactRecord contact;
  final bool wasCreated;
  final bool announced;
}

class TopicReadyResult {
  const TopicReadyResult({
    required this.topic,
    required this.wasCreated,
  });

  final TopicRecord topic;
  final bool wasCreated;
}

class FieldNoteResult {
  const FieldNoteResult({
    required this.topic,
    required this.published,
    this.marker,
    this.attachment,
  });

  final TopicRecord topic;
  final bool published;
  final MarkerRecord? marker;
  final AttachmentRecord? attachment;
}

class TopicSyncResult {
  const TopicSyncResult({
    required this.topic,
    required this.wasCreated,
    required this.subscribed,
    required this.telemetry,
  });

  final TopicRecord topic;
  final bool wasCreated;
  final bool subscribed;
  final List<TelemetryPointRecord> telemetry;
}

class AttachmentReportResult {
  const AttachmentReportResult({
    required this.topic,
    required this.attachment,
    required this.published,
  });

  final TopicRecord topic;
  final AttachmentRecord attachment;
  final bool published;
}

class MissionUpdateDraft {
  const MissionUpdateDraft({
    required this.peerIdentity,
    required this.content,
    this.topicPath,
    this.attachments = const <AttachmentDraft>[],
    this.metadata = const <String, Object?>{},
    this.correlationId,
    this.idempotencyKey,
  });

  final String peerIdentity;
  final String content;
  final String? topicPath;
  final List<AttachmentDraft> attachments;
  final Map<String, Object?> metadata;
  final String? correlationId;
  final String? idempotencyKey;
}

class MissionUpdateResult {
  const MissionUpdateResult({
    required this.peer,
    required this.receipt,
    this.topic,
    this.attachments = const <AttachmentRecord>[],
  });

  final PeerReadyResult peer;
  final SendReceipt receipt;
  final TopicRecord? topic;
  final List<AttachmentRecord> attachments;
}

class ConversationReadyResult {
  const ConversationReadyResult({
    required this.peer,
    required this.snapshot,
  });

  final PeerReadyResult peer;
  final ConversationSnapshot snapshot;
}

class ConversationSendResult {
  const ConversationSendResult({
    required this.peer,
    required this.receipt,
  });

  final PeerReadyResult peer;
  final SendReceipt receipt;
}

class WorkspaceClient {
  WorkspaceClient(this.app);

  factory WorkspaceClient.fromBinding(AppBinding binding) {
    return WorkspaceClient(AppClient(binding));
  }

  factory WorkspaceClient.rpc(RpcConnectionOptions options) {
    return WorkspaceClient.fromBinding(RpcBinding(options));
  }

  final AppClient app;

  late final OperationClient operations = OperationClient(app);
  late final DiscoveryClient discovery = DiscoveryClient(operations);
  late final CustomCommandClient commands = CustomCommandClient(operations);
  late final RemoteCommandClient remoteCommands = RemoteCommandClient(app);
  late final ConversationClient conversations = ConversationClient(app);
  late final VoiceSessionClient voice = VoiceSessionClient(operations);
  late final TopicClient topics = TopicClient(operations);
  late final TelemetryClient telemetry = TelemetryClient(operations);
  late final MarkerClient markers = MarkerClient(operations);
  late final AttachmentClient attachments = AttachmentClient(operations);
  late final WorkspaceFlows flows = WorkspaceFlows(this);

  Future<Handle> start(Config config) => app.start(config);

  Future<void> stop() => app.stop();

  Future<RuntimeStatus> status() => app.status();

  Future<SendReceipt> send(SendRequest request) => app.send(request);

  Future<SendReport> sendWithProfileDefaults(SendRequest request) {
    return app.sendWithProfileDefaults(request);
  }

  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) {
    return app.sendWithOptions(request, options);
  }

  Stream<AppEvent> subscribeEvents() => app.subscribeEvents();
}

class WorkspaceFlows {
  WorkspaceFlows(this._workspace);

  final WorkspaceClient _workspace;
  static const Set<String> _missionReservedMetadataKeys = <String>{
    'content',
    'topic_id',
    'group_id',
    'file_attachments',
  };

  Future<PeerReadyResult> ensurePeerReady(
    String identity, {
    String? displayName,
    TrustLevel trustLevel = TrustLevel.trusted,
    bool bootstrap = true,
    bool announce = true,
    Map<String, Object?> metadata = const <String, Object?>{},
  }) async {
    final existing = await _findContact(identity);
    final announced =
        announce ? await _workspace.discovery.announceNow() : false;
    if (existing != null) {
      return PeerReadyResult(
        identity: identity,
        contact: existing,
        wasCreated: false,
        announced: announced,
      );
    }

    final contact = bootstrap
        ? await _workspace.discovery.bootstrapIdentity(identity: identity)
        : await _workspace.discovery.updateContact(
            identity: identity,
            displayName: displayName,
            trustLevel: trustLevel,
            bootstrap: false,
            metadata: metadata,
          );

    return PeerReadyResult(
      identity: identity,
      contact: contact,
      wasCreated: true,
      announced: announced,
    );
  }

  Future<TopicReadyResult> ensureTopic(
    String topicPath, {
    Map<String, Object?> metadata = const <String, Object?>{},
    int pageSize = 100,
  }) async {
    final existing = await _findTopicByPath(topicPath, pageSize: pageSize);
    if (existing != null) {
      return TopicReadyResult(topic: existing, wasCreated: false);
    }

    final created = await _workspace.topics.create(
      topicPath: topicPath,
      metadata: metadata,
    );
    return TopicReadyResult(topic: created, wasCreated: true);
  }

  Future<FieldNoteResult> publishFieldNote({
    required String topicPath,
    required Object? payload,
    String? correlationId,
    String? markerLabel,
    GeoPoint? markerPosition,
    AttachmentDraft? attachment,
    Map<String, Object?> topicMetadata = const <String, Object?>{},
  }) async {
    final topicResult = await ensureTopic(topicPath, metadata: topicMetadata);
    final published = await _workspace.topics.publish(
      topicId: topicResult.topic.topicId,
      payload: payload,
      correlationId: correlationId,
    );

    MarkerRecord? marker;
    if (markerLabel != null && markerPosition != null) {
      marker = await _workspace.markers.create(
        label: markerLabel,
        position: markerPosition,
        topicId: topicResult.topic.topicId,
      );
    }

    AttachmentRecord? storedAttachment;
    if (attachment != null) {
      storedAttachment = await _workspace.attachments.store(
        name: attachment.name,
        contentType: attachment.contentType,
        bytesBase64: attachment.bytesBase64,
        topicIds: <String>[topicResult.topic.topicId],
      );
    }

    return FieldNoteResult(
      topic: topicResult.topic,
      published: published,
      marker: marker,
      attachment: storedAttachment,
    );
  }

  Future<List<PeerDirectoryEntry>> discoverPeers({
    int limit = 50,
    bool onlineOnly = false,
    bool bootstrapOnly = false,
  }) async {
    final peers = await _workspace.discovery.peerDirectory();
    final filtered = peers.where((peer) {
      if (onlineOnly && !peer.online) {
        return false;
      }
      if (bootstrapOnly && !peer.bootstrap) {
        return false;
      }
      return true;
    }).toList(growable: false);
    if (filtered.length <= limit) {
      return filtered;
    }
    return filtered.take(limit).toList(growable: false);
  }

  Future<ConversationReadyResult> ensureConversation(
    String identity, {
    String? displayName,
    TrustLevel trustLevel = TrustLevel.trusted,
    bool bootstrap = true,
    bool announce = true,
    Map<String, Object?> metadata = const <String, Object?>{},
  }) async {
    final peer = await ensurePeerReady(
      identity,
      displayName: displayName,
      trustLevel: trustLevel,
      bootstrap: bootstrap,
      announce: announce,
      metadata: metadata,
    );
    final snapshot = await _workspace.conversations.loadConversation(identity);
    return ConversationReadyResult(peer: peer, snapshot: snapshot);
  }

  Future<ConversationSendResult> sendConversationText(
    String identity,
    String content, {
    String? correlationId,
    String? idempotencyKey,
    String? displayName,
    TrustLevel trustLevel = TrustLevel.trusted,
    bool bootstrap = true,
    bool announce = true,
    Map<String, Object?> metadata = const <String, Object?>{},
  }) async {
    final peer = await ensurePeerReady(
      identity,
      displayName: displayName,
      trustLevel: trustLevel,
      bootstrap: bootstrap,
      announce: announce,
      metadata: metadata,
    );
    final receipt = await _workspace.conversations.sendText(
      identity,
      content,
      correlationId: correlationId,
      idempotencyKey: idempotencyKey,
    );
    return ConversationSendResult(peer: peer, receipt: receipt);
  }

  Future<TopicSyncResult> ensureTopicSync(
    String topicPath, {
    Map<String, Object?> metadata = const <String, Object?>{},
    int telemetryLimit = 100,
  }) async {
    final topicResult = await ensureTopic(topicPath, metadata: metadata);
    final subscribed =
        await _workspace.topics.subscribe(topicResult.topic.topicId);
    final telemetry = await _workspace.telemetry.query(
      topicId: topicResult.topic.topicId,
      limit: telemetryLimit,
    );
    return TopicSyncResult(
      topic: topicResult.topic,
      wasCreated: topicResult.wasCreated,
      subscribed: subscribed,
      telemetry: telemetry,
    );
  }

  Future<AttachmentReportResult> publishAttachmentReport({
    required String topicPath,
    required AttachmentDraft attachment,
    Object? summaryPayload,
    String? correlationId,
    Map<String, Object?> topicMetadata = const <String, Object?>{},
  }) async {
    final topicResult = await ensureTopic(topicPath, metadata: topicMetadata);
    final stored = await _workspace.attachments.store(
      name: attachment.name,
      contentType: attachment.contentType,
      bytesBase64: attachment.bytesBase64,
      topicIds: <String>[topicResult.topic.topicId],
    );
    final published = await _workspace.topics.publish(
      topicId: topicResult.topic.topicId,
      correlationId: correlationId,
      payload: <String, Object?>{
        if (summaryPayload != null) 'summary': summaryPayload,
        'attachment_id': stored.attachmentId,
        'attachment_name': stored.name,
        'content_type': stored.contentType,
      },
    );
    return AttachmentReportResult(
      topic: topicResult.topic,
      attachment: stored,
      published: published,
    );
  }

  Future<MissionUpdateResult> sendMissionUpdate(
    MissionUpdateDraft draft, {
    String? displayName,
    TrustLevel trustLevel = TrustLevel.trusted,
    bool bootstrap = true,
    bool announce = true,
  }) async {
    final conflictingMetadataKeys = draft.metadata.keys
        .where(_missionReservedMetadataKeys.contains)
        .toList(growable: false);
    if (conflictingMetadataKeys.isNotEmpty) {
      throw ArgumentError.value(
        conflictingMetadataKeys,
        'draft.metadata',
        'mission metadata cannot override reserved fields',
      );
    }

    final peer = await ensurePeerReady(
      draft.peerIdentity,
      displayName: displayName,
      trustLevel: trustLevel,
      bootstrap: bootstrap,
      announce: announce,
      metadata: draft.metadata,
    );

    TopicRecord? topic;
    if (draft.topicPath != null) {
      topic = (await ensureTopic(draft.topicPath!)).topic;
    }

    final storedAttachments = <AttachmentRecord>[];
    for (final attachment in draft.attachments) {
      storedAttachments.add(
        await _workspace.attachments.store(
          name: attachment.name,
          contentType: attachment.contentType,
          bytesBase64: attachment.bytesBase64,
          topicIds: topic == null ? const <String>[] : <String>[topic.topicId],
        ),
      );
    }

    final receipt = await _workspace.app.send(
      SendRequest(
        source: await _workspace.conversations.selfAddress(),
        destination: draft.peerIdentity,
        payload: <String, Object?>{
          'content': draft.content,
          if (topic != null) 'topic_id': topic.topicId,
          if (topic != null) 'group_id': topic.topicId,
          if (storedAttachments.isNotEmpty)
            'file_attachments': storedAttachments
                .map(
                  (attachment) => <String, Object?>{
                    'attachment_id': attachment.attachmentId,
                    'name': attachment.name,
                    'content_type': attachment.contentType,
                    'byte_len': attachment.byteLen,
                  },
                )
                .toList(growable: false),
          ...Map<String, Object?>.from(draft.metadata),
        },
        correlationId: draft.correlationId,
        idempotencyKey: draft.idempotencyKey,
      ),
    );

    return MissionUpdateResult(
      peer: peer,
      receipt: receipt,
      topic: topic,
      attachments: List<AttachmentRecord>.unmodifiable(storedAttachments),
    );
  }

  Future<ContactRecord?> _findContact(String identity) async {
    String? cursor;
    do {
      final page =
          await _workspace.discovery.contactList(cursor: cursor, limit: 100);
      for (final contact in page.contacts) {
        if (contact.identity == identity) {
          return contact;
        }
      }
      cursor = page.nextCursor;
    } while (cursor != null);
    return null;
  }

  Future<TopicRecord?> _findTopicByPath(
    String topicPath, {
    required int pageSize,
  }) async {
    String? cursor;
    do {
      final page =
          await _workspace.topics.list(cursor: cursor, limit: pageSize);
      for (final topic in page.topics) {
        if (topic.topicPath == topicPath) {
          return topic;
        }
      }
      cursor = page.nextCursor;
    } while (cursor != null);
    return null;
  }
}

class AttachmentDraft {
  const AttachmentDraft({
    required this.name,
    required this.contentType,
    required this.bytesBase64,
  });

  final String name;
  final String contentType;
  final String bytesBase64;
}
