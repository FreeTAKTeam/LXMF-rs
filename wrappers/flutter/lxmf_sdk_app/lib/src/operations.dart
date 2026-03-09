import 'client.dart';
import 'models.dart';

typedef OperationPayloadDecoder<T> = T Function(Object? payload);

class OperationCall<T> {
  const OperationCall({
    required this.operationId,
    required this.payload,
    required this.decode,
    this.target,
    this.correlationId,
    this.timeoutMs,
    this.extensions = const <String, Object?>{},
  });

  final String operationId;
  final Object? payload;
  final OperationPayloadDecoder<T> decode;
  final String? target;
  final String? correlationId;
  final int? timeoutMs;
  final Map<String, Object?> extensions;
}

class OperationResult<T> {
  const OperationResult({
    required this.operationId,
    required this.accepted,
    required this.payload,
    this.alias,
    this.correlationId,
    this.extensions = const <String, Object?>{},
  });

  final String operationId;
  final bool accepted;
  final T payload;
  final String? alias;
  final String? correlationId;
  final Map<String, Object?> extensions;
}

class OperationClient {
  OperationClient(this._appClient);

  final AppClient _appClient;
  Future<OperationRegistry>? _registryFuture;

  Future<OperationRegistry> registry() {
    return _registryFuture ??= _appClient.operationRegistry();
  }

  Future<ResolvedOperation> resolve(String operationId) async {
    final resolved = (await registry()).resolve(operationId);
    if (resolved == null) {
      throw AppError(
        code: ErrorCode.validationInvalidArgument,
        category: ErrorCategory.validation,
        message: 'unknown operation id: $operationId',
        userActionRequired: true,
      );
    }
    return resolved;
  }

  Future<OperationResult<T>> query<T>(OperationCall<T> call) async {
    final resolved = await resolve(call.operationId);
    if (!resolved.entry.acceptsEnvelopeKind(EnvelopeKind.query)) {
      throw AppError(
        code: ErrorCode.validationInvalidArgument,
        category: ErrorCategory.validation,
        message:
            'operation ${resolved.canonicalId} does not accept query envelopes',
        userActionRequired: true,
      );
    }
    final response = await _appClient.queryOperation(
      resolved.canonicalId,
      call.payload,
      target: call.target,
      correlationId: call.correlationId,
      timeoutMs: call.timeoutMs,
      extensions: call.extensions,
    );
    return OperationResult<T>(
      operationId: response.operationId,
      accepted: response.accepted,
      payload: call.decode(response.payload),
      alias: resolved.alias,
      correlationId: response.correlationId,
      extensions: response.extensions,
    );
  }

  Future<OperationResult<T>> command<T>(OperationCall<T> call) async {
    final resolved = await resolve(call.operationId);
    if (!resolved.entry.acceptsEnvelopeKind(EnvelopeKind.command)) {
      throw AppError(
        code: ErrorCode.validationInvalidArgument,
        category: ErrorCategory.validation,
        message:
            'operation ${resolved.canonicalId} does not accept command envelopes',
        userActionRequired: true,
      );
    }
    final response = await _appClient.commandOperation(
      resolved.canonicalId,
      call.payload,
      target: call.target,
      correlationId: call.correlationId,
      timeoutMs: call.timeoutMs,
      extensions: call.extensions,
    );
    return OperationResult<T>(
      operationId: response.operationId,
      accepted: response.accepted,
      payload: call.decode(response.payload),
      alias: resolved.alias,
      correlationId: response.correlationId,
      extensions: response.extensions,
    );
  }
}

class CustomCommandCall<T> {
  const CustomCommandCall({
    required this.operationId,
    required this.payload,
    required this.decodeEcho,
    this.target,
    this.correlationId,
    this.timeoutMs,
    this.extensions = const <String, Object?>{},
  });

  final String operationId;
  final Object? payload;
  final OperationPayloadDecoder<T> decodeEcho;
  final String? target;
  final String? correlationId;
  final int? timeoutMs;
  final Map<String, Object?> extensions;
}

class CustomCommandResult<T> {
  const CustomCommandResult({
    required this.operationId,
    required this.accepted,
    required this.echo,
    this.alias,
    this.command,
    this.target,
    this.correlationId,
    this.timeoutMs,
    this.extensions = const <String, Object?>{},
  });

  final String operationId;
  final bool accepted;
  final T echo;
  final String? alias;
  final String? command;
  final String? target;
  final String? correlationId;
  final int? timeoutMs;
  final Map<String, Object?> extensions;
}

class CustomCommandClient {
  CustomCommandClient(this._operations);

  final OperationClient _operations;

  Future<CustomCommandResult<T>> invoke<T>(CustomCommandCall<T> call) async {
    final result = await _operations.command<Map<String, Object?>>(
      OperationCall<Map<String, Object?>>(
        operationId: call.operationId,
        payload: call.payload,
        target: call.target,
        correlationId: call.correlationId,
        timeoutMs: call.timeoutMs,
        extensions: call.extensions,
        decode: (payload) => _payloadMap(payload),
      ),
    );

    return CustomCommandResult<T>(
      operationId: result.operationId,
      accepted: result.accepted,
      echo: call.decodeEcho(result.payload['echo']),
      alias: result.alias,
      command: result.payload['command']?.toString(),
      target: result.payload['target']?.toString(),
      correlationId: result.payload['correlation_id']?.toString(),
      timeoutMs: (result.payload['timeout_ms'] as num?)?.toInt(),
      extensions: result.extensions,
    );
  }

  static Map<String, Object?> _payloadMap(Object? payload) {
    if (payload is Map<String, Object?>) {
      return payload;
    }
    if (payload is Map) {
      return payload.map((key, value) => MapEntry(key.toString(), value));
    }
    throw const AppError(
      code: ErrorCode.internalUnexpectedFailure,
      category: ErrorCategory.internal,
      message: 'custom command payload was not an object',
    );
  }
}

class DiscoveryClient {
  DiscoveryClient(this._operations);

  final OperationClient _operations;

  Future<List<IdentityBundle>> identityList() async {
    final result = await _operations.query<List<IdentityBundle>>(
      OperationCall<List<IdentityBundle>>(
        operationId: 'app.identity.list',
        payload: const <String, Object?>{},
        decode: (payload) => (payload as List<Object?>? ?? const <Object?>[])
            .map(_decodeIdentityBundle)
            .toList(growable: false),
      ),
    );
    return result.payload;
  }

  Future<bool> announceNow() async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.identity.announce',
        payload: const <String, Object?>{},
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }

  Future<PresencePage> presenceList({
    String? cursor,
    int? limit,
  }) async {
    final result = await _operations.query<PresencePage>(
      OperationCall<PresencePage>(
        operationId: 'app.identity.presence.list',
        payload: <String, Object?>{
          if (cursor != null) 'cursor': cursor,
          if (limit != null) 'limit': limit,
        },
        decode: _decodePresencePage,
      ),
    );
    return result.payload;
  }

  Future<ContactListPage> contactList({
    String? cursor,
    int? limit,
  }) async {
    final result = await _operations.query<ContactListPage>(
      OperationCall<ContactListPage>(
        operationId: 'app.contact.list',
        payload: <String, Object?>{
          if (cursor != null) 'cursor': cursor,
          if (limit != null) 'limit': limit,
        },
        decode: _decodeContactListPage,
      ),
    );
    return result.payload;
  }

  Future<ContactRecord> updateContact({
    required String identity,
    String? displayName,
    TrustLevel? trustLevel,
    bool? bootstrap,
    Map<String, Object?> metadata = const <String, Object?>{},
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<ContactRecord>(
      OperationCall<ContactRecord>(
        operationId: 'app.contact.update',
        payload: <String, Object?>{
          'identity': identity,
          if (displayName != null) 'display_name': displayName,
          if (trustLevel != null) 'trust_level': _trustLevelToWire(trustLevel),
          if (bootstrap != null) 'bootstrap': bootstrap,
          'metadata': metadata,
          'extensions': extensions,
        },
        decode: _decodeContactRecord,
      ),
    );
    return result.payload;
  }

  Future<ContactRecord> bootstrapIdentity({
    required String identity,
    bool autoSync = true,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<ContactRecord>(
      OperationCall<ContactRecord>(
        operationId: 'app.identity.bootstrap',
        payload: <String, Object?>{
          'identity': identity,
          'auto_sync': autoSync,
          'extensions': extensions,
        },
        decode: _decodeContactRecord,
      ),
    );
    return result.payload;
  }

  Future<List<PeerDirectoryEntry>> peerDirectory({int? limit}) async {
    final contacts = await _drainContactPages(limit: limit);
    final presence = await _drainPresencePages(limit: limit);
    return _mergePeerDirectory(contacts, presence, limit: limit);
  }

  Future<List<ContactRecord>> _drainContactPages({int? limit}) async {
    final contacts = <ContactRecord>[];
    String? cursor;
    do {
      final previousCursor = cursor;
      final page = await contactList(cursor: cursor, limit: limit);
      contacts.addAll(page.contacts);
      if (limit != null && contacts.length >= limit) {
        return contacts.take(limit).toList(growable: false);
      }
      cursor = page.nextCursor;
      if (cursor != null && cursor == previousCursor) {
        break;
      }
    } while (cursor != null);
    return contacts;
  }

  Future<List<PresenceRecord>> _drainPresencePages({int? limit}) async {
    final peers = <PresenceRecord>[];
    String? cursor;
    do {
      final previousCursor = cursor;
      final page = await presenceList(cursor: cursor, limit: limit);
      peers.addAll(page.peers);
      if (limit != null && peers.length >= limit) {
        return peers.take(limit).toList(growable: false);
      }
      cursor = page.nextCursor;
      if (cursor != null && cursor == previousCursor) {
        break;
      }
    } while (cursor != null);
    return peers;
  }
}

class VoiceSessionClient {
  VoiceSessionClient(this._operations);

  final OperationClient _operations;

  Future<String> open({
    required String peerId,
    String? codecHint,
  }) async {
    final result = await _operations.command<String>(
      OperationCall<String>(
        operationId: 'app.voice.session.open',
        payload: <String, Object?>{
          'peer_id': peerId,
          if (codecHint != null) 'codec_hint': codecHint,
        },
        decode: (payload) => payload.toString(),
      ),
    );
    return result.payload;
  }

  Future<VoiceSessionState> update({
    required String sessionId,
    required VoiceSessionState state,
  }) async {
    final result = await _operations.command<VoiceSessionState>(
      OperationCall<VoiceSessionState>(
        operationId: 'app.voice.session.update',
        payload: <String, Object?>{
          'session_id': sessionId,
          'state': _voiceStateToWire(state),
        },
        decode: (payload) => _voiceStateFromWire(payload?.toString()),
      ),
    );
    return result.payload;
  }

  Future<bool> close(String sessionId) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.voice.session.close',
        payload: sessionId,
        decode: (payload) {
          if (payload is Map<Object?, Object?>) {
            return payload['accepted'] == true;
          }
          if (payload is Map<String, Object?>) {
            return payload['accepted'] == true;
          }
          return false;
        },
      ),
    );
    return result.payload;
  }

  static VoiceSessionState _voiceStateFromWire(String? value) {
    return switch (value) {
      'new' => VoiceSessionState.newState,
      'ringing' => VoiceSessionState.ringing,
      'active' => VoiceSessionState.active,
      'holding' => VoiceSessionState.holding,
      'closed' => VoiceSessionState.closed,
      'failed' => VoiceSessionState.failed,
      _ => VoiceSessionState.unknown,
    };
  }

  static String _voiceStateToWire(VoiceSessionState state) {
    return switch (state) {
      VoiceSessionState.newState => 'new',
      VoiceSessionState.ringing => 'ringing',
      VoiceSessionState.active => 'active',
      VoiceSessionState.holding => 'holding',
      VoiceSessionState.closed => 'closed',
      VoiceSessionState.failed => 'failed',
      VoiceSessionState.unknown => 'unknown',
    };
  }
}

class AttachmentClient {
  AttachmentClient(this._operations);

  final OperationClient _operations;

  Future<AttachmentRecord> store({
    required String name,
    required String contentType,
    required String bytesBase64,
    int? expiresTsMs,
    List<String> topicIds = const <String>[],
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<AttachmentRecord>(
      OperationCall<AttachmentRecord>(
        operationId: 'app.attachment.store',
        payload: <String, Object?>{
          'name': name,
          'content_type': contentType,
          'bytes_base64': bytesBase64,
          if (expiresTsMs != null) 'expires_ts_ms': expiresTsMs,
          'topic_ids': topicIds,
          'extensions': extensions,
        },
        decode: _decodeAttachmentRecord,
      ),
    );
    return result.payload;
  }

  Future<AttachmentRecord?> get(String attachmentId) async {
    final result = await _operations.query<AttachmentRecord?>(
      OperationCall<AttachmentRecord?>(
        operationId: 'app.attachment.get',
        payload: attachmentId,
        decode: (payload) =>
            payload == null ? null : _decodeAttachmentRecord(payload),
      ),
    );
    return result.payload;
  }

  Future<AttachmentListPage> list({
    String? topicId,
    String? cursor,
    int? limit,
  }) async {
    final result = await _operations.query<AttachmentListPage>(
      OperationCall<AttachmentListPage>(
        operationId: 'app.attachment.list',
        payload: <String, Object?>{
          if (topicId != null) 'topic_id': topicId,
          if (cursor != null) 'cursor': cursor,
          if (limit != null) 'limit': limit,
        },
        decode: _decodeAttachmentListPage,
      ),
    );
    return result.payload;
  }

  Future<bool> associateTopic({
    required String attachmentId,
    required String topicId,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.attachment.associate_topic',
        payload: <String, Object?>{
          'attachment_id': attachmentId,
          'topic_id': topicId,
          'extensions': extensions,
        },
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }

  Future<bool> delete(
    String attachmentId, {
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.attachment.delete',
        payload: attachmentId,
        extensions: extensions,
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }

  Future<AttachmentUploadSession> uploadStart({
    required String name,
    required String contentType,
    required int totalSize,
    required String checksumSha256,
    int? expiresTsMs,
    List<String> topicIds = const <String>[],
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<AttachmentUploadSession>(
      OperationCall<AttachmentUploadSession>(
        operationId: 'app.attachment.upload_start',
        payload: <String, Object?>{
          'name': name,
          'content_type': contentType,
          'total_size': totalSize,
          'checksum_sha256': checksumSha256,
          if (expiresTsMs != null) 'expires_ts_ms': expiresTsMs,
          'topic_ids': topicIds,
          'extensions': extensions,
        },
        decode: _decodeAttachmentUploadSession,
      ),
    );
    return result.payload;
  }

  Future<AttachmentUploadChunkAck> uploadChunk({
    required String uploadId,
    required int offset,
    required String bytesBase64,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<AttachmentUploadChunkAck>(
      OperationCall<AttachmentUploadChunkAck>(
        operationId: 'app.attachment.upload_chunk',
        payload: <String, Object?>{
          'upload_id': uploadId,
          'offset': offset,
          'bytes_base64': bytesBase64,
          'extensions': extensions,
        },
        decode: _decodeAttachmentUploadChunkAck,
      ),
    );
    return result.payload;
  }

  Future<AttachmentRecord> uploadCommit({
    required String uploadId,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<AttachmentRecord>(
      OperationCall<AttachmentRecord>(
        operationId: 'app.attachment.upload_commit',
        payload: <String, Object?>{
          'upload_id': uploadId,
          'extensions': extensions,
        },
        decode: _decodeAttachmentRecord,
      ),
    );
    return result.payload;
  }

  Future<AttachmentDownloadChunk> downloadChunk({
    required String attachmentId,
    required int offset,
    required int maxBytes,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.query<AttachmentDownloadChunk>(
      OperationCall<AttachmentDownloadChunk>(
        operationId: 'app.attachment.download_chunk',
        payload: <String, Object?>{
          'attachment_id': attachmentId,
          'offset': offset,
          'max_bytes': maxBytes,
          'extensions': extensions,
        },
        decode: _decodeAttachmentDownloadChunk,
      ),
    );
    return result.payload;
  }
}

class TopicClient {
  TopicClient(this._operations);

  final OperationClient _operations;

  Future<TopicRecord> create({
    String? topicPath,
    Map<String, Object?> metadata = const <String, Object?>{},
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<TopicRecord>(
      OperationCall<TopicRecord>(
        operationId: 'app.topic.create',
        payload: <String, Object?>{
          if (topicPath != null) 'topic_path': topicPath,
          'metadata': metadata,
          'extensions': extensions,
        },
        decode: _decodeTopicRecord,
      ),
    );
    return result.payload;
  }

  Future<TopicRecord?> get(String topicId) async {
    final result = await _operations.query<TopicRecord?>(
      OperationCall<TopicRecord?>(
        operationId: 'app.topic.get',
        payload: topicId,
        decode: (payload) {
          if (payload == null) {
            return null;
          }
          return _decodeTopicRecord(payload);
        },
      ),
    );
    return result.payload;
  }

  Future<TopicListPage> list({
    String? cursor,
    int? limit,
  }) async {
    final result = await _operations.query<TopicListPage>(
      OperationCall<TopicListPage>(
        operationId: 'app.topic.list',
        payload: <String, Object?>{
          if (cursor != null) 'cursor': cursor,
          if (limit != null) 'limit': limit,
        },
        decode: _decodeTopicListPage,
      ),
    );
    return result.payload;
  }

  Future<bool> subscribe(String topicId, {String? cursor}) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.topic.subscribe',
        payload: <String, Object?>{
          'topic_id': topicId,
          if (cursor != null) 'cursor': cursor,
        },
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }

  Future<bool> unsubscribe(String topicId) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.topic.unsubscribe',
        payload: topicId,
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }

  Future<bool> publish({
    required String topicId,
    required Object? payload,
    String? correlationId,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.topic.publish',
        payload: <String, Object?>{
          'topic_id': topicId,
          'payload': payload,
          if (correlationId != null) 'correlation_id': correlationId,
          'extensions': extensions,
        },
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }
}

class TelemetryClient {
  TelemetryClient(this._operations);

  final OperationClient _operations;

  Future<List<TelemetryPointRecord>> query({
    String? peerId,
    String? topicId,
    int? fromTsMs,
    int? toTsMs,
    int? limit,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.query<List<TelemetryPointRecord>>(
      OperationCall<List<TelemetryPointRecord>>(
        operationId: 'app.telemetry.query',
        payload: <String, Object?>{
          if (peerId != null) 'peer_id': peerId,
          if (topicId != null) 'topic_id': topicId,
          if (fromTsMs != null) 'from_ts_ms': fromTsMs,
          if (toTsMs != null) 'to_ts_ms': toTsMs,
          if (limit != null) 'limit': limit,
          'extensions': extensions,
        },
        decode: (payload) => (payload as List<Object?>? ?? const <Object?>[])
            .map(_decodeTelemetryPoint)
            .toList(growable: false),
      ),
    );
    return result.payload;
  }

  Future<bool> subscribe({
    String? peerId,
    String? topicId,
    int? fromTsMs,
    int? toTsMs,
    int? limit,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.telemetry.subscribe',
        payload: <String, Object?>{
          if (peerId != null) 'peer_id': peerId,
          if (topicId != null) 'topic_id': topicId,
          if (fromTsMs != null) 'from_ts_ms': fromTsMs,
          if (toTsMs != null) 'to_ts_ms': toTsMs,
          if (limit != null) 'limit': limit,
          'extensions': extensions,
        },
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }
}

class MarkerClient {
  MarkerClient(this._operations);

  final OperationClient _operations;

  Future<MarkerRecord> create({
    required String label,
    required GeoPoint position,
    String? topicId,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<MarkerRecord>(
      OperationCall<MarkerRecord>(
        operationId: 'app.marker.create',
        payload: <String, Object?>{
          'label': label,
          'position': _encodeGeoPoint(position),
          if (topicId != null) 'topic_id': topicId,
          'extensions': extensions,
        },
        decode: _decodeMarkerRecord,
      ),
    );
    return result.payload;
  }

  Future<MarkerListPage> list({
    String? topicId,
    String? cursor,
    int? limit,
  }) async {
    final result = await _operations.query<MarkerListPage>(
      OperationCall<MarkerListPage>(
        operationId: 'app.marker.list',
        payload: <String, Object?>{
          if (topicId != null) 'topic_id': topicId,
          if (cursor != null) 'cursor': cursor,
          if (limit != null) 'limit': limit,
        },
        decode: _decodeMarkerListPage,
      ),
    );
    return result.payload;
  }

  Future<MarkerRecord> updatePosition({
    required String markerId,
    required int expectedRevision,
    required GeoPoint position,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<MarkerRecord>(
      OperationCall<MarkerRecord>(
        operationId: 'app.marker.update_position',
        payload: <String, Object?>{
          'marker_id': markerId,
          'expected_revision': expectedRevision,
          'position': _encodeGeoPoint(position),
          'extensions': extensions,
        },
        decode: _decodeMarkerRecord,
      ),
    );
    return result.payload;
  }

  Future<bool> delete({
    required String markerId,
    required int expectedRevision,
    Map<String, Object?> extensions = const <String, Object?>{},
  }) async {
    final result = await _operations.command<bool>(
      OperationCall<bool>(
        operationId: 'app.marker.delete',
        payload: <String, Object?>{
          'marker_id': markerId,
          'expected_revision': expectedRevision,
          'extensions': extensions,
        },
        decode: _decodeAccepted,
      ),
    );
    return result.payload;
  }
}

TopicRecord _decodeTopicRecord(Object? payload) {
  final map = _payloadMap(payload);
  return TopicRecord(
    topicId: map['topic_id']?.toString() ?? '',
    topicPath: map['topic_path']?.toString(),
    createdTsMs: (map['created_ts_ms'] as num?)?.toInt() ?? 0,
    metadata: _payloadMap(map['metadata']),
    extensions: _payloadMap(map['extensions']),
  );
}

TopicListPage _decodeTopicListPage(Object? payload) {
  final map = _payloadMap(payload);
  final topics = (map['topics'] as List<Object?>? ?? const <Object?>[])
      .map(_decodeTopicRecord)
      .toList(growable: false);
  return TopicListPage(
    topics: topics,
    nextCursor: map['next_cursor']?.toString(),
  );
}

IdentityBundle _decodeIdentityBundle(Object? payload) {
  final map = _payloadMap(payload);
  final capabilities =
      (map['capabilities'] as List<Object?>? ?? const <Object?>[])
          .map((value) => value.toString())
          .toList(growable: false);
  return IdentityBundle(
    identity: map['identity']?.toString() ?? '',
    publicKey: map['public_key']?.toString() ?? '',
    displayName: map['display_name']?.toString(),
    capabilities: capabilities,
    extensions: _payloadMap(map['extensions']),
  );
}

ContactRecord _decodeContactRecord(Object? payload) {
  final map = _payloadMap(payload);
  return ContactRecord(
    identity: map['identity']?.toString() ?? '',
    displayName: map['display_name']?.toString(),
    trustLevel: _trustLevelFromWire(map['trust_level']?.toString()),
    bootstrap: map['bootstrap'] == true,
    updatedTsMs: (map['updated_ts_ms'] as num?)?.toInt() ?? 0,
    metadata: _payloadMap(map['metadata']),
    extensions: _payloadMap(map['extensions']),
  );
}

ContactListPage _decodeContactListPage(Object? payload) {
  final map = _payloadMap(payload);
  final contacts = (map['contacts'] as List<Object?>? ?? const <Object?>[])
      .map(_decodeContactRecord)
      .toList(growable: false);
  return ContactListPage(
    contacts: contacts,
    nextCursor: map['next_cursor']?.toString(),
  );
}

PresenceRecord _decodePresenceRecord(Object? payload) {
  final map = _payloadMap(payload);
  return PresenceRecord(
    peerId: map['peer_id']?.toString() ?? '',
    lastSeenTsMs: (map['last_seen_ts_ms'] as num?)?.toInt() ?? 0,
    firstSeenTsMs: (map['first_seen_ts_ms'] as num?)?.toInt() ?? 0,
    seenCount: (map['seen_count'] as num?)?.toInt() ?? 0,
    displayName: map['name']?.toString(),
    nameSource: map['name_source']?.toString(),
    trustLevel: map['trust_level'] == null
        ? null
        : _trustLevelFromWire(map['trust_level']?.toString()),
    bootstrap: map['bootstrap'] as bool?,
    extensions: _payloadMap(map['extensions']),
  );
}

PresencePage _decodePresencePage(Object? payload) {
  final map = _payloadMap(payload);
  final peers = (map['peers'] as List<Object?>? ?? const <Object?>[])
      .map(_decodePresenceRecord)
      .toList(growable: false);
  return PresencePage(
    peers: peers,
    nextCursor: map['next_cursor']?.toString(),
  );
}

TelemetryPointRecord _decodeTelemetryPoint(Object? payload) {
  final map = _payloadMap(payload);
  final tags = map['tags'] is Map
      ? (map['tags'] as Map).map(
          (key, value) => MapEntry(key.toString(), value.toString()),
        )
      : const <String, String>{};
  return TelemetryPointRecord(
    tsMs: (map['ts_ms'] as num?)?.toInt() ?? 0,
    key: map['key']?.toString() ?? '',
    value: map['value'],
    unit: map['unit']?.toString(),
    tags: tags,
    extensions: _payloadMap(map['extensions']),
  );
}

MarkerRecord _decodeMarkerRecord(Object? payload) {
  final map = _payloadMap(payload);
  return MarkerRecord(
    markerId: map['marker_id']?.toString() ?? '',
    label: map['label']?.toString() ?? '',
    position: _decodeGeoPoint(map['position']),
    topicId: map['topic_id']?.toString(),
    revision: (map['revision'] as num?)?.toInt() ?? 0,
    updatedTsMs: (map['updated_ts_ms'] as num?)?.toInt() ?? 0,
    extensions: _payloadMap(map['extensions']),
  );
}

AttachmentRecord _decodeAttachmentRecord(Object? payload) {
  final map = _payloadMap(payload);
  final topicIds = (map['topic_ids'] as List<Object?>? ?? const <Object?>[])
      .map((value) => value.toString())
      .toList(growable: false);
  return AttachmentRecord(
    attachmentId: map['attachment_id']?.toString() ?? '',
    name: map['name']?.toString() ?? '',
    contentType: map['content_type']?.toString() ?? '',
    byteLen: (map['byte_len'] as num?)?.toInt() ?? 0,
    checksumSha256: map['checksum_sha256']?.toString() ?? '',
    createdTsMs: (map['created_ts_ms'] as num?)?.toInt() ?? 0,
    expiresTsMs: (map['expires_ts_ms'] as num?)?.toInt(),
    topicIds: topicIds,
    extensions: _payloadMap(map['extensions']),
  );
}

AttachmentUploadSession _decodeAttachmentUploadSession(Object? payload) {
  final map = _payloadMap(payload);
  return AttachmentUploadSession(
    uploadId: map['upload_id']?.toString() ?? '',
    attachmentId: map['attachment_id']?.toString() ?? '',
    chunkSizeHint: (map['chunk_size_hint'] as num?)?.toInt() ?? 0,
    nextOffset: (map['next_offset'] as num?)?.toInt() ?? 0,
  );
}

AttachmentUploadChunkAck _decodeAttachmentUploadChunkAck(Object? payload) {
  final map = _payloadMap(payload);
  return AttachmentUploadChunkAck(
    accepted: map['accepted'] == true,
    nextOffset: (map['next_offset'] as num?)?.toInt() ?? 0,
    complete: map['complete'] == true,
  );
}

AttachmentDownloadChunk _decodeAttachmentDownloadChunk(Object? payload) {
  final map = _payloadMap(payload);
  return AttachmentDownloadChunk(
    attachmentId: map['attachment_id']?.toString() ?? '',
    offset: (map['offset'] as num?)?.toInt() ?? 0,
    nextOffset: (map['next_offset'] as num?)?.toInt() ?? 0,
    totalSize: (map['total_size'] as num?)?.toInt() ?? 0,
    done: map['done'] == true,
    checksumSha256: map['checksum_sha256']?.toString() ?? '',
    bytesBase64: map['bytes_base64']?.toString() ?? '',
  );
}

AttachmentListPage _decodeAttachmentListPage(Object? payload) {
  final map = _payloadMap(payload);
  final attachments =
      (map['attachments'] as List<Object?>? ?? const <Object?>[])
          .map(_decodeAttachmentRecord)
          .toList(growable: false);
  return AttachmentListPage(
    attachments: attachments,
    nextCursor: map['next_cursor']?.toString(),
  );
}

MarkerListPage _decodeMarkerListPage(Object? payload) {
  final map = _payloadMap(payload);
  final markers = (map['markers'] as List<Object?>? ?? const <Object?>[])
      .map(_decodeMarkerRecord)
      .toList(growable: false);
  return MarkerListPage(
    markers: markers,
    nextCursor: map['next_cursor']?.toString(),
  );
}

GeoPoint _decodeGeoPoint(Object? payload) {
  final map = _payloadMap(payload);
  return GeoPoint(
    lat: (map['lat'] as num?)?.toDouble() ?? 0,
    lon: (map['lon'] as num?)?.toDouble() ?? 0,
    altM: (map['alt_m'] as num?)?.toDouble(),
  );
}

Map<String, Object?> _encodeGeoPoint(GeoPoint position) {
  return <String, Object?>{
    'lat': position.lat,
    'lon': position.lon,
    'alt_m': position.altM,
  };
}

bool _decodeAccepted(Object? payload) {
  final map = _payloadMap(payload);
  return map['accepted'] == true;
}

TrustLevel _trustLevelFromWire(String? value) {
  return switch (value) {
    'trusted' => TrustLevel.trusted,
    'untrusted' => TrustLevel.untrusted,
    'blocked' => TrustLevel.blocked,
    _ => TrustLevel.unknown,
  };
}

String _trustLevelToWire(TrustLevel value) {
  return switch (value) {
    TrustLevel.trusted => 'trusted',
    TrustLevel.untrusted => 'untrusted',
    TrustLevel.blocked => 'blocked',
    TrustLevel.unknown => 'unknown',
  };
}

List<PeerDirectoryEntry> _mergePeerDirectory(
  List<ContactRecord> contacts,
  List<PresenceRecord> presence, {
  int? limit,
}) {
  final byPeer = <String, PeerDirectoryEntry>{};

  for (final contact in contacts) {
    byPeer[contact.identity] = PeerDirectoryEntry(
      peerId: contact.identity,
      displayName: contact.displayName,
      nameSource: null,
      trustLevel: contact.trustLevel,
      bootstrap: contact.bootstrap,
      online: false,
      lastSeenTsMs: null,
      firstSeenTsMs: null,
      seenCount: 0,
      metadata: contact.metadata,
      extensions: contact.extensions,
    );
  }

  for (final peer in presence) {
    final existing = byPeer[peer.peerId];
    byPeer[peer.peerId] = PeerDirectoryEntry(
      peerId: peer.peerId,
      displayName: existing?.displayName ?? peer.displayName,
      nameSource: existing?.nameSource ?? peer.nameSource,
      trustLevel: existing?.trustLevel ?? peer.trustLevel,
      bootstrap: existing?.bootstrap ?? peer.bootstrap ?? false,
      online: true,
      lastSeenTsMs: peer.lastSeenTsMs,
      firstSeenTsMs: peer.firstSeenTsMs,
      seenCount: peer.seenCount,
      metadata: existing?.metadata ?? const <String, Object?>{},
      extensions: {
        ...(existing?.extensions ?? const <String, Object?>{}),
        ...peer.extensions,
      },
    );
  }

  final entries = byPeer.values.toList(growable: false)
    ..sort((left, right) {
      final leftSeen = left.lastSeenTsMs ?? -1;
      final rightSeen = right.lastSeenTsMs ?? -1;
      final seenCmp = rightSeen.compareTo(leftSeen);
      if (seenCmp != 0) {
        return seenCmp;
      }
      return left.peerId.compareTo(right.peerId);
    });
  if (limit == null || entries.length <= limit) {
    return entries;
  }
  return entries.take(limit).toList(growable: false);
}

Map<String, Object?> _payloadMap(Object? payload) {
  if (payload == null) {
    return const <String, Object?>{};
  }
  if (payload is Map<String, Object?>) {
    return payload;
  }
  if (payload is Map) {
    return payload.map((key, value) => MapEntry(key.toString(), value));
  }
  throw const AppError(
    code: ErrorCode.internalUnexpectedFailure,
    category: ErrorCategory.internal,
    message: 'operation payload was not an object',
  );
}
