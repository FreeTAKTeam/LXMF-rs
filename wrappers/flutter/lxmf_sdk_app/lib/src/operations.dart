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

bool _decodeAccepted(Object? payload) {
  final map = _payloadMap(payload);
  return map['accepted'] == true;
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
