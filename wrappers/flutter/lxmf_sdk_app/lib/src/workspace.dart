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
