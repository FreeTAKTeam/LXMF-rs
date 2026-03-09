import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import '../client.dart';
import '../models.dart';
import 'codec.dart';

final class RpcConnectionOptions {
  const RpcConnectionOptions({
    required this.endpoint,
    this.authToken,
    this.requestTimeout = const Duration(seconds: 10),
    this.pollIdleDelay = const Duration(milliseconds: 250),
  });

  final Uri endpoint;
  final String? authToken;
  final Duration requestTimeout;
  final Duration pollIdleDelay;
}

final class RpcBinding implements AppBinding {
  RpcBinding(this._options, {HttpClient? httpClient})
      : _httpClient = httpClient ?? HttpClient();

  final RpcConnectionOptions _options;
  final HttpClient _httpClient;
  int _nextRequestId = 1;

  Config? _activeConfig;
  Handle? _activeHandle;
  bool _shutdownRequested = false;

  @override
  Future<Handle> start(Config config) async {
    if (config.profile == Profile.embeddedDefault) {
      throw const AppError(
        code: ErrorCode.capabilityUnsupportedProfile,
        category: ErrorCategory.capability,
        message: 'embedded_default is not supported by the Flutter RPC binding',
        userActionRequired: true,
      );
    }
    if (config.transportMode != TransportMode.bleOnly ||
        config.tcpHost != null ||
        config.tcpPort != null ||
        config.tcpListenPort != null) {
      throw const AppError(
        code: ErrorCode.configInvalid,
        category: ErrorCategory.config,
        message: 'rpc binding does not accept embedded transport configuration',
        userActionRequired: true,
      );
    }

    if (_activeConfig != null) {
      if (_configEquals(_activeConfig!, config) && _activeHandle != null) {
        return _activeHandle!;
      }
      throw const AppError(
        code: ErrorCode.runtimeAlreadyRunningDifferentConfig,
        category: ErrorCategory.runtime,
        message: 'rpc binding is already started with a different config',
      );
    }

    final negotiate = await _call(
      'sdk_negotiate_v2',
      <String, Object?>{
        'supported_contract_versions': config.supportedContractVersions,
        'requested_capabilities': config.requestedCapabilities,
        'config': _runtimeConfigFor(config),
      },
    );
    final snapshot = await _snapshot(includeCounts: false);

    final patch = _runtimePatchFor(config);
    if (patch.isNotEmpty) {
      await _call('sdk_configure_v2', <String, Object?>{
        'expected_revision':
            (snapshot['config_revision'] as num?)?.toInt() ?? 0,
        'patch': patch,
      });
    }

    final runtime = await _snapshot(includeCounts: true);
    final handle = Handle(
      runtimeId: _stringAt(runtime, 'runtime_id') ??
          _stringAt(negotiate, 'runtime_id') ??
          'rpc-runtime',
      profile: config.profile,
      capabilities: CapabilitySummary(
        activeContractVersion:
            (_numberAt(negotiate, 'active_contract_version') ?? 2).toInt(),
        effectiveCapabilities:
            _stringListAt(negotiate, 'effective_capabilities'),
        effectiveLimits: _mapAt(negotiate, 'effective_limits'),
      ),
    );

    _activeConfig = config;
    _activeHandle = handle;
    _shutdownRequested = false;
    return handle;
  }

  @override
  Future<void> stop() async {
    if (_activeHandle == null) {
      return;
    }
    _shutdownRequested = true;
    try {
      await _call('sdk_shutdown_v2', <String, Object?>{
        'mode': 'graceful',
      });
    } finally {
      _activeConfig = null;
      _activeHandle = null;
    }
  }

  @override
  Future<RuntimeStatus> status() async {
    if (_activeHandle == null) {
      return const RuntimeStatus(state: RunState.stopped);
    }
    final snapshot = await _snapshot(includeCounts: true);
    return RuntimeStatus(
      runtimeId: _stringAt(snapshot, 'runtime_id'),
      state: _mapRunState(_stringAt(snapshot, 'state')),
      profile: _activeConfig?.profile,
      capabilities: _activeHandle?.capabilities,
      queuedMessages: (_numberAt(snapshot, 'queued_messages') ?? 0).toInt(),
      inFlightMessages:
          (_numberAt(snapshot, 'in_flight_messages') ?? 0).toInt(),
      eventStreamPosition:
          (_numberAt(snapshot, 'event_stream_position') ?? 0).toInt(),
      configRevision: (_numberAt(snapshot, 'config_revision') ?? 0).toInt(),
    );
  }

  @override
  Future<SendReceipt> send(SendRequest request) async {
    final handle = _requireHandle();
    final response = await _call('sdk_send_v2', <String, Object?>{
      'id': _messageIdFor(request),
      'source': request.source,
      'destination': request.destination,
      'title': '',
      'content': _payloadAsContent(request.payload),
      if (request.extensions.isNotEmpty) 'fields': request.extensions,
    });
    return SendReceipt(
      runtimeId: handle.runtimeId,
      messageId: _stringAt(response, 'message_id') ?? _messageIdFor(request),
      profile: handle.profile,
      correlationId: request.correlationId,
    );
  }

  @override
  Future<SendReport> sendWithProfileDefaults(SendRequest request) {
    return sendWithOptions(request, const DeliveryOptions());
  }

  @override
  Future<SendReport> sendWithOptions(
    SendRequest request,
    DeliveryOptions options,
  ) async {
    final config = _requireConfig();
    final plan = config.deliveryPlan();
    final maxAttempts = options.maxAttempts ?? plan.retry.maxAttempts;
    final queuePressureStrategy =
        options.queuePressureStrategy ?? plan.queuePressure.strategy;
    final attempts = <DeliveryAttempt>[];
    var totalDelayMs = 0;

    for (var attempt = 1; attempt <= maxAttempts; attempt++) {
      try {
        final receipt = await send(request);
        return SendReport(
          receipt: receipt,
          attempts: attempts,
          totalDelayMs: totalDelayMs,
          plan: plan,
        );
      } on AppError catch (error) {
        final isQueuePressure = error.code == ErrorCode.deliveryQueuePressure;
        final canRetry = (isQueuePressure &&
                queuePressureStrategy == QueuePressureStrategy.retry) ||
            (!isQueuePressure && error.retryable);
        if (!canRetry || attempt >= maxAttempts) {
          if (error.retryable && !isQueuePressure) {
            throw AppError(
              code: ErrorCode.deliveryRetryExhausted,
              category: ErrorCategory.delivery,
              message: 'delivery helper exhausted retry policy',
              causeCode: error.causeCode ?? error.code.wireName,
            );
          }
          rethrow;
        }

        final delayMs = isQueuePressure
            ? _delayForAttempt(plan.queuePressure.backoff, attempt)
            : _delayForAttempt(plan.retry.backoff, attempt);
        attempts.add(
          DeliveryAttempt(
            attempt: attempt,
            disposition: AttemptDisposition.retried,
            errorCode: error.code.wireName,
            retryable: error.retryable,
            queuePressure: isQueuePressure,
            scheduledDelayMs: delayMs,
          ),
        );
        totalDelayMs += delayMs;
        if (delayMs > 0) {
          await Future<void>.delayed(Duration(milliseconds: delayMs));
        }
      }
    }

    throw const AppError(
      code: ErrorCode.deliveryRetryExhausted,
      category: ErrorCategory.delivery,
      message: 'delivery helper exhausted retry policy',
    );
  }

  @override
  Future<OperationRegistry> operationRegistry() async {
    final result =
        await _call('sdk_operation_registry_v2', const <String, Object?>{});
    final registryMap = result['registry'];
    if (registryMap is! Map) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'sdk_operation_registry_v2 did not return a registry object',
      );
    }
    final entries = registryMap['entries'];
    if (entries is! List) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'sdk_operation_registry_v2 did not return an entries array',
      );
    }
    return OperationRegistry(
      entries: entries
          .whereType<Map>()
          .map(
            (entry) => _operationEntryFromMap(
              entry.map((key, value) => MapEntry(key.toString(), value)),
            ),
          )
          .toList(growable: false),
    );
  }

  @override
  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) async {
    final result = await _call(
      'sdk_envelope_execute_v2',
      _envelopeParams(envelope),
    );
    final response = result['response'];
    if (response is! Map) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'sdk_envelope_execute_v2 did not return a response object',
      );
    }
    return _envelopeResponseFromMap(
      response.map((key, value) => MapEntry(key.toString(), value)),
    );
  }

  @override
  Stream<AppEvent> subscribeEvents() {
    final handle = _requireHandle();
    final config = _requireConfig();
    final pollMax =
        config.eventBatchSize ?? config.deliveryPlan().defaultEventBatchSize;
    final idleDelay = _options.pollIdleDelay;

    return Stream<AppEvent>.multi((controller) {
      var cursor = _shutdownRequested ? null : null;
      var active = true;
      Future<void>(() async {
        while (active && !_shutdownRequested) {
          try {
            final result = await _call('sdk_poll_events_v2', <String, Object?>{
              'cursor': cursor,
              'max': pollMax,
            });
            final events = (_mapAt(result, 'events')['events'] ??
                result['events']) as List?;
            cursor = _stringAt(result, 'next_cursor');
            if (events == null || events.isEmpty) {
              await Future<void>.delayed(idleDelay);
              continue;
            }
            for (final raw in events.cast<Object?>()) {
              if (!active) {
                break;
              }
              final event = _mapEvent(raw, handle, config.profile);
              controller.add(event);
            }
          } on AppError catch (error) {
            if (error.code == ErrorCode.runtimeStreamDegraded) {
              controller.add(_streamGapEvent(handle, config.profile));
              cursor = null;
              await Future<void>.delayed(idleDelay);
              continue;
            }
            if (error.code == ErrorCode.connectivityDisconnected ||
                error.code == ErrorCode.connectivityReconnectFailed) {
              await Future<void>.delayed(idleDelay);
              continue;
            }
            controller.addError(error);
            active = false;
            await controller.close();
          }
        }
      });

      controller.onCancel = () async {
        active = false;
      };
    });
  }

  Future<String> deliveryDestinationHash() async {
    final result = await _callLegacy('status', const <String, Object?>{});
    final hash = _stringAt(result, 'delivery_destination_hash');
    if (hash == null || hash.isEmpty) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'rpc status did not expose delivery_destination_hash',
      );
    }
    return hash;
  }

  Future<List<MessageRecord>> messageHistory() async {
    final result =
        await _callLegacy('list_messages', const <String, Object?>{});
    final messages = result['messages'];
    if (messages is! List) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'list_messages response did not contain a messages array',
      );
    }
    return messages
        .whereType<Map>()
        .map(
          (entry) => messageRecordFromMap(
            entry.map((key, value) => MapEntry(key.toString(), value)),
          ),
        )
        .toList(growable: false);
  }

  Future<List<IdentityBundle>> identityList() async {
    final result =
        await _call('sdk_identity_list_v2', const <String, Object?>{});
    final identities =
        result['identities'] ?? (_mapAt(result, 'identity_list')['identities']);
    if (identities is! List) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'sdk_identity_list_v2 did not return an identities array',
      );
    }
    return identities
        .whereType<Map>()
        .map(
          (entry) => _identityBundleFromMap(
            entry.map((key, value) => MapEntry(key.toString(), value)),
          ),
        )
        .toList(growable: false);
  }

  Future<ContactListPage> contactList({
    String? cursor,
    int? limit,
  }) async {
    final result =
        await _call('sdk_identity_contact_list_v2', <String, Object?>{
      if (cursor != null) 'cursor': cursor,
      if (limit != null) 'limit': limit,
    });
    final payload = _mapAt(result, 'contact_list');
    final contactsValue = payload['contacts'];
    if (contactsValue is! List) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'sdk_identity_contact_list_v2 did not return a contacts array',
      );
    }
    final contacts = contactsValue
        .whereType<Map>()
        .map(
          (entry) => _contactRecordFromMap(
            entry.map((key, value) => MapEntry(key.toString(), value)),
          ),
        )
        .toList(growable: false);
    return ContactListPage(
      contacts: contacts,
      nextCursor: payload['next_cursor']?.toString(),
    );
  }

  Future<DeliveryStatus?> deliveryStatus(String messageId) async {
    final result = await _call('sdk_status_v2', <String, Object?>{
      'message_id': messageId,
    });
    final message = result['message'];
    if (message == null) {
      return null;
    }
    if (message is! Map) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'sdk_status_v2 returned an invalid message payload',
      );
    }
    return _deliveryStatusFromMap(
      message.map((key, value) => MapEntry(key.toString(), value)),
    );
  }

  Stream<DeliveryStatus> watchMessageStatus(String messageId) {
    return Stream<DeliveryStatus>.multi((controller) {
      StreamSubscription<AppEvent>? subscription;
      var cancelled = false;

      Future<void>(() async {
        final initial = await deliveryStatus(messageId);
        if (cancelled) {
          return;
        }
        if (initial != null) {
          controller.add(initial);
          if (initial.isTerminal) {
            await controller.close();
            return;
          }
        }

        subscription = subscribeEvents().listen(
          (event) {
            final status = _deliveryStatusFromEvent(event);
            if (status == null || status.messageId != messageId) {
              return;
            }
            controller.add(status);
            if (status.isTerminal) {
              controller.close();
            }
          },
          onError: controller.addError,
        );
      });

      controller.onCancel = () async {
        cancelled = true;
        await subscription?.cancel();
      };
    });
  }

  Future<Map<String, Object?>> _snapshot({required bool includeCounts}) async {
    return _call(
      'sdk_snapshot_v2',
      <String, Object?>{'include_counts': includeCounts},
    );
  }

  Future<Map<String, Object?>> _callLegacy(
    String method,
    Map<String, Object?> params,
  ) {
    return _call(method, params);
  }

  Future<Map<String, Object?>> _call(
    String method,
    Map<String, Object?> params,
  ) async {
    final request = <String, Object?>{
      'id': _nextRequestId++,
      'method': method,
      'params': params,
    };
    final payload = encodeRpcFrame(request);
    final response = await _post(payload);
    final decoded = decodeRpcFrame(response);
    final errorValue = decoded['error'];
    if (errorValue is Map<String, Object?>) {
      throw _mapRpcError(errorValue);
    }
    final result = decoded['result'];
    if (result is Map<String, Object?>) {
      return result;
    }
    if (result == null) {
      return const <String, Object?>{};
    }
    throw const AppError(
      code: ErrorCode.internalUnexpectedFailure,
      category: ErrorCategory.internal,
      message: 'rpc response result must be an object',
    );
  }

  Future<List<int>> _post(List<int> payload) async {
    try {
      final request = await _httpClient
          .postUrl(_options.endpoint)
          .timeout(_options.requestTimeout);
      request.headers.contentType = ContentType('application', 'msgpack');
      request.contentLength = payload.length;
      if (_options.authToken case final token? when token.isNotEmpty) {
        request.headers.set(HttpHeaders.authorizationHeader, 'Bearer $token');
      }
      request.add(payload);
      final response = await request.close().timeout(_options.requestTimeout);
      final bytes = await response.fold<BytesBuilder>(
        BytesBuilder(copy: false),
        (builder, chunk) => builder..add(chunk),
      );
      final body = bytes.takeBytes();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        throw AppError(
          code: ErrorCode.connectivityDisconnected,
          category: ErrorCategory.connectivity,
          message: 'rpc endpoint returned HTTP ${response.statusCode}',
          retryable: true,
          terminal: false,
        );
      }
      return body;
    } on SocketException catch (error) {
      throw AppError(
        code: ErrorCode.connectivityDisconnected,
        category: ErrorCategory.connectivity,
        message: error.message,
        retryable: true,
        terminal: false,
      );
    } on HttpException catch (error) {
      throw AppError(
        code: ErrorCode.connectivityDisconnected,
        category: ErrorCategory.connectivity,
        message: error.message,
        retryable: true,
        terminal: false,
      );
    }
  }

  AppEvent _mapEvent(Object? raw, Handle handle, Profile profile) {
    final map =
        (raw as Map).map((key, value) => MapEntry(key.toString(), value));
    final eventType = (map['event_type'] ?? 'unknown').toString();
    final payload = map['payload'] is Map
        ? (map['payload'] as Map)
            .map((key, value) => MapEntry(key.toString(), value))
        : const <String, Object?>{};
    final message = payload['message'];
    final messageMap = message is Map
        ? message.map((key, value) => MapEntry(key.toString(), value))
        : const <String, Object?>{};
    final receiptStatus = (messageMap['receipt_status'] ?? '').toString();
    final kind = switch (eventType) {
      'StreamGap' => EventKind.streamGapDetected,
      'inbound' => EventKind.inboundMessageReceived,
      'delivery_cancelled' => EventKind.messageCancelled,
      'runtime_shutdown_requested' => EventKind.runtimeStopped,
      'config_updated' => EventKind.runtimeRecovered,
      'outbound' when receiptStatus.startsWith('sent') => EventKind.messageSent,
      'outbound' when receiptStatus == 'delivered' =>
        EventKind.messageDelivered,
      'outbound' when receiptStatus == 'cancelled' =>
        EventKind.messageCancelled,
      'outbound' when receiptStatus.startsWith('failed') =>
        EventKind.messageFailed,
      _ => EventKind.unknown,
    };

    final seqNo = (map['seq_no'] as num?)?.toInt() ?? 0;
    final occurredAtMs = (map['ts_ms'] as num?)?.toInt() ?? 0;
    final severity = _mapSeverity((map['severity'] ?? 'unknown').toString());
    final gap = eventType == 'StreamGap'
        ? StreamGapDetails(
            expectedSeqNo: (payload['expected_seq_no'] as num?)?.toInt(),
            observedSeqNo: (payload['observed_seq_no'] as num?)?.toInt(),
            droppedCount: (payload['dropped_count'] as num?)?.toInt() ?? 0,
          )
        : null;

    return AppEvent(
      metadata: EventMetadata(
        eventId: (map['event_id'] ?? 'evt-$seqNo').toString(),
        runtimeId: (map['runtime_id'] ?? handle.runtimeId).toString(),
        seqNo: seqNo,
        occurredAtMs: occurredAtMs,
        severity: severity,
        profileId: profile.id,
        messageId:
            messageMap['id']?.toString() ?? payload['message_id']?.toString(),
      ),
      kind: kind,
      rawEventType: eventType,
      details: payload,
      streamGap: gap,
    );
  }

  AppEvent _streamGapEvent(Handle handle, Profile profile) {
    final now = DateTime.now().millisecondsSinceEpoch;
    return AppEvent(
      metadata: EventMetadata(
        eventId: 'rpc-gap-$now',
        runtimeId: handle.runtimeId,
        seqNo: 0,
        occurredAtMs: now,
        severity: Severity.warn,
        profileId: profile.id,
      ),
      kind: EventKind.streamGapDetected,
      rawEventType: 'StreamGap',
      streamGap: const StreamGapDetails(recoveryRequired: true),
    );
  }

  Handle _requireHandle() {
    final handle = _activeHandle;
    if (handle == null) {
      throw const AppError(
        code: ErrorCode.runtimeNotStarted,
        category: ErrorCategory.runtime,
        message: 'rpc binding is not started',
      );
    }
    return handle;
  }

  Config _requireConfig() {
    final config = _activeConfig;
    if (config == null) {
      throw const AppError(
        code: ErrorCode.runtimeNotStarted,
        category: ErrorCategory.runtime,
        message: 'rpc binding is not started',
      );
    }
    return config;
  }

  static bool _configEquals(Config a, Config b) {
    return a.profile == b.profile &&
        a.eventBatchSize == b.eventBatchSize &&
        _listEquals(a.supportedContractVersions, b.supportedContractVersions) &&
        _listEquals(a.requestedCapabilities, b.requestedCapabilities) &&
        _deepMapEquals(a.sdkConfig, b.sdkConfig);
  }

  static bool _listEquals<T>(List<T> a, List<T> b) {
    if (a.length != b.length) {
      return false;
    }
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) {
        return false;
      }
    }
    return true;
  }

  static bool _deepMapEquals(Map<String, Object?> a, Map<String, Object?> b) {
    if (a.length != b.length) {
      return false;
    }
    for (final entry in a.entries) {
      if (!_deepEquals(entry.value, b[entry.key])) {
        return false;
      }
    }
    return true;
  }

  static bool _deepEquals(Object? a, Object? b) {
    if (a is Map<String, Object?> && b is Map<String, Object?>) {
      return _deepMapEquals(a, b);
    }
    if (a is List && b is List) {
      if (a.length != b.length) {
        return false;
      }
      for (var i = 0; i < a.length; i++) {
        if (!_deepEquals(a[i], b[i])) {
          return false;
        }
      }
      return true;
    }
    return a == b;
  }

  static Map<String, Object?> _runtimeConfigFor(Config config) {
    final sdkConfig = config.sdkConfig;
    return <String, Object?>{
      'profile': _rpcProfileFor(config.profile, sdkConfig),
      'bind_mode': sdkConfig['bind_mode'] ?? 'local_only',
      'auth_mode': sdkConfig['auth_mode'] ?? 'local_trusted',
      if (sdkConfig['overflow_policy'] != null)
        'overflow_policy': sdkConfig['overflow_policy'],
      if (sdkConfig['block_timeout_ms'] != null)
        'block_timeout_ms': sdkConfig['block_timeout_ms'],
      if (sdkConfig['store_forward'] case final storeForward?)
        'store_forward': storeForward,
      if (sdkConfig['event_sink'] case final eventSink?)
        'event_sink': eventSink,
      if (sdkConfig['rpc_backend'] case final rpcBackend?)
        'rpc_backend': rpcBackend,
    };
  }

  static Map<String, Object?> _runtimePatchFor(Config config) {
    final patch = <String, Object?>{};
    if (config.eventBatchSize case final batchSize?) {
      patch['event_stream'] = <String, Object?>{'max_poll_events': batchSize};
    }
    if (config.sdkConfig['rpc_patch'] case final rpcPatch?) {
      if (rpcPatch is! Map<String, Object?>) {
        throw const AppError(
          code: ErrorCode.configInvalid,
          category: ErrorCategory.config,
          message: 'sdkConfig.rpc_patch must be an object',
          userActionRequired: true,
        );
      }
      patch.addAll(rpcPatch);
    }
    return patch;
  }

  static String _rpcProfileFor(
    Profile profile,
    Map<String, Object?> sdkConfig,
  ) {
    if (sdkConfig['rpc_profile'] case final override?) {
      return override.toString();
    }
    return switch (profile) {
      Profile.mobileDefault => 'desktop-local-runtime',
      Profile.desktopDefault => 'desktop-full',
      Profile.testingDefault => 'desktop-local-runtime',
      Profile.embeddedDefault => 'embedded-alloc',
    };
  }

  static String _messageIdFor(SendRequest request) {
    if (request.idempotencyKey case final key? when key.isNotEmpty) {
      return key;
    }
    if (request.correlationId case final id? when id.isNotEmpty) {
      return id;
    }
    return 'dart-${DateTime.now().microsecondsSinceEpoch}';
  }

  static String _payloadAsContent(Object? payload) {
    if (payload is String) {
      return payload;
    }
    return jsonEncode(payload);
  }

  static RunState _mapRunState(String? value) {
    return switch (value) {
      'running' => RunState.running,
      'starting' => RunState.starting,
      'degraded' => RunState.degraded,
      'stopping' => RunState.stopping,
      'stopped' => RunState.stopped,
      'failed' => RunState.failed,
      _ => RunState.failed,
    };
  }

  static Severity _mapSeverity(String value) {
    return switch (value) {
      'debug' => Severity.debug,
      'info' => Severity.info,
      'warn' => Severity.warn,
      'error' => Severity.error,
      'critical' => Severity.critical,
      _ => Severity.unknown,
    };
  }

  static AppError _mapRpcError(Map<String, Object?> error) {
    final code =
        (error['machine_code'] ?? error['code'] ?? 'unknown').toString();
    final message = (error['message'] ?? 'rpc call failed').toString();
    final details = _mapAt(error, 'details');
    final mapped = switch (code) {
      'SDK_VALIDATION_INVALID_ARGUMENT' => (
          ErrorCode.validationInvalidArgument,
          ErrorCategory.validation,
          false,
          true,
          true,
        ),
      'SDK_VALIDATION_UNKNOWN_FIELD' => (
          ErrorCode.validationUnknownField,
          ErrorCategory.validation,
          false,
          true,
          true,
        ),
      'SDK_CAPABILITY_CONTRACT_INCOMPATIBLE' => (
          ErrorCode.capabilityUnsupportedProfile,
          ErrorCategory.capability,
          false,
          true,
          true,
        ),
      'SDK_CAPABILITY_DISABLED' => (
          ErrorCode.capabilityRequiredFeatureMissing,
          ErrorCategory.capability,
          false,
          true,
          true,
        ),
      'SDK_SECURITY_AUTH_REQUIRED' || 'SDK_SECURITY_REMOTE_BIND_DISALLOWED' => (
          ErrorCode.securityAuthRequired,
          ErrorCategory.security,
          false,
          true,
          true,
        ),
      'SDK_CONFIG_CONFLICT' || 'SDK_CONFIG_UNKNOWN_KEY' => (
          ErrorCode.configInvalid,
          ErrorCategory.config,
          false,
          true,
          true,
        ),
      'SDK_RUNTIME_STREAM_DEGRADED' ||
      'SDK_RUNTIME_CURSOR_EXPIRED' ||
      'SDK_RUNTIME_INVALID_CURSOR' =>
        (
          ErrorCode.runtimeStreamDegraded,
          ErrorCategory.runtime,
          true,
          false,
          false,
        ),
      'SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED' => (
          ErrorCode.deliveryQueuePressure,
          ErrorCategory.delivery,
          true,
          false,
          false,
        ),
      'DELIVERY_FAILED' => (
          ErrorCode.connectivityDisconnected,
          ErrorCategory.connectivity,
          true,
          false,
          false,
        ),
      _ => (
          ErrorCode.internalUnexpectedFailure,
          _mapCategory((error['category'] ?? '').toString()),
          (error['retryable'] as bool?) ?? false,
          true,
          (error['is_user_actionable'] as bool?) ?? false,
        ),
    };

    return AppError(
      code: mapped.$1,
      category: mapped.$2,
      message: message,
      retryable: mapped.$3,
      terminal: mapped.$4,
      userActionRequired: mapped.$5,
      causeCode: code,
      details: details,
    );
  }

  static IdentityBundle _identityBundleFromMap(Map<String, Object?> map) {
    return IdentityBundle(
      identity: map['identity']?.toString() ?? '',
      publicKey: map['public_key']?.toString() ?? '',
      displayName: map['display_name']?.toString(),
      capabilities: _stringListAt(map, 'capabilities'),
      extensions: _mapAt(map, 'extensions'),
    );
  }

  static ContactRecord _contactRecordFromMap(Map<String, Object?> map) {
    return ContactRecord(
      identity: map['identity']?.toString() ?? '',
      displayName: map['display_name']?.toString(),
      trustLevel: _mapTrustLevel(map['trust_level']?.toString()),
      bootstrap: map['bootstrap'] == true,
      updatedTsMs: (map['updated_ts_ms'] as num?)?.toInt() ?? 0,
      metadata: _mapAt(map, 'metadata'),
      extensions: _mapAt(map, 'extensions'),
    );
  }

  static DeliveryStatus _deliveryStatusFromMap(Map<String, Object?> map) {
    return DeliveryStatus(
      messageId: map['id']?.toString() ?? '',
      receiptStatus: map['receipt_status']?.toString(),
      source: map['source']?.toString(),
      destination: map['destination']?.toString(),
      content: map['content']?.toString(),
      timestampMs: _timestampMs(map['timestamp']),
      direction: map['direction']?.toString(),
      fields: _mapAt(map, 'fields'),
    );
  }

  static MessageRecord messageRecordFromMap(Map<String, Object?> map) {
    return MessageRecord(
      id: map['id']?.toString() ?? '',
      source: map['source']?.toString(),
      destination: map['destination']?.toString(),
      title: map['title']?.toString(),
      content: map['content']?.toString(),
      timestampMs: _timestampMs(map['timestamp']),
      direction: map['direction']?.toString(),
      fields: _mapAt(map, 'fields'),
      receiptStatus: map['receipt_status']?.toString(),
      raw: map,
    );
  }

  static DeliveryStatus? _deliveryStatusFromEvent(AppEvent event) {
    if (event.rawEventType == 'delivery_cancelled') {
      final payload = event.details is Map<String, Object?>
          ? event.details! as Map<String, Object?>
          : const <String, Object?>{};
      final messageId = payload['message_id']?.toString();
      if (messageId == null || messageId.isEmpty) {
        return null;
      }
      return DeliveryStatus(
        messageId: messageId,
        receiptStatus: 'cancelled',
      );
    }
    if (event.rawEventType != 'outbound' ||
        event.details is! Map<String, Object?>) {
      return null;
    }
    final payload = event.details! as Map<String, Object?>;
    final message = payload['message'];
    if (message is! Map) {
      return null;
    }
    return _deliveryStatusFromMap(
      message.map((key, value) => MapEntry(key.toString(), value)),
    );
  }

  static TrustLevel _mapTrustLevel(String? value) {
    return switch (value) {
      'trusted' => TrustLevel.trusted,
      'untrusted' => TrustLevel.untrusted,
      'blocked' => TrustLevel.blocked,
      _ => TrustLevel.unknown,
    };
  }

  static int? _timestampMs(Object? raw) {
    if (raw is int) {
      return raw < 1000000000000 ? raw * 1000 : raw;
    }
    if (raw is num) {
      final value = raw.toInt();
      return value < 1000000000000 ? value * 1000 : value;
    }
    return null;
  }

  static ErrorCategory _mapCategory(String category) {
    return switch (category) {
      'Validation' => ErrorCategory.validation,
      'Capability' => ErrorCategory.capability,
      'Config' => ErrorCategory.config,
      'Policy' => ErrorCategory.policy,
      'Transport' => ErrorCategory.connectivity,
      'Storage' => ErrorCategory.persistence,
      'Timeout' => ErrorCategory.timeout,
      'Runtime' => ErrorCategory.runtime,
      'Security' => ErrorCategory.security,
      _ => ErrorCategory.internal,
    };
  }

  static Map<String, Object?> _mapAt(Map<String, Object?> value, String key) {
    final nested = value[key];
    if (nested is Map<String, Object?>) {
      return nested;
    }
    if (nested is Map) {
      return nested.map((nestedKey, nestedValue) {
        return MapEntry(nestedKey.toString(), nestedValue);
      });
    }
    return const <String, Object?>{};
  }

  static List<String> _stringListAt(Map<String, Object?> value, String key) {
    final nested = value[key];
    if (nested is! List) {
      return const <String>[];
    }
    return nested.map((entry) => entry.toString()).toList(growable: false);
  }

  static String? _stringAt(Map<String, Object?> value, String key) {
    final nested = value[key];
    return nested?.toString();
  }

  static Map<String, Object?> _envelopeParams(Envelope envelope) {
    return <String, Object?>{
      'operation_id': envelope.operationId,
      'kind': _envelopeKindToWire(envelope.kind),
      if (envelope.target case final target?) 'target': target,
      if (envelope.correlationId case final correlationId?)
        'correlation_id': correlationId,
      if (envelope.timeoutMs case final timeoutMs?) 'timeout_ms': timeoutMs,
      'payload': envelope.payload,
      if (envelope.extensions.isNotEmpty) 'extensions': envelope.extensions,
    };
  }

  static OperationEntry _operationEntryFromMap(Map<String, Object?> map) {
    return OperationEntry(
      id: map['id']?.toString() ?? '',
      group: map['group']?.toString() ?? '',
      kind: _operationKind(map['kind']?.toString()),
      transportVariant: _transportVariant(map['transport_variant']?.toString()),
      description: map['description']?.toString() ?? '',
      aliases: _stringListAt(map, 'aliases'),
      requiredCapabilities: _stringListAt(map, 'required_capabilities'),
    );
  }

  static EnvelopeResponse _envelopeResponseFromMap(Map<String, Object?> map) {
    return EnvelopeResponse(
      operationId: map['operation_id']?.toString() ?? '',
      kind: _envelopeKind(map['kind']?.toString()),
      accepted: map['accepted'] == true,
      correlationId: map['correlation_id']?.toString(),
      payload: map['payload'],
      extensions: _mapAt(map, 'extensions'),
    );
  }

  static EnvelopeKind _envelopeKind(String? value) {
    return switch (value) {
      'query' => EnvelopeKind.query,
      'command' => EnvelopeKind.command,
      'result' => EnvelopeKind.result,
      'error' => EnvelopeKind.error,
      _ => EnvelopeKind.error,
    };
  }

  static String _envelopeKindToWire(EnvelopeKind kind) {
    return switch (kind) {
      EnvelopeKind.query => 'query',
      EnvelopeKind.command => 'command',
      EnvelopeKind.result => 'result',
      EnvelopeKind.error => 'error',
    };
  }

  static OperationKind _operationKind(String? value) {
    return switch (value) {
      'query' => OperationKind.query,
      'command' => OperationKind.command,
      _ => OperationKind.command,
    };
  }

  static TransportVariant _transportVariant(String? value) {
    return switch (value) {
      'app' => TransportVariant.app,
      'rpc' => TransportVariant.rpc,
      'legacy_rpc' => TransportVariant.legacyRpc,
      'extension' => TransportVariant.extension,
      _ => TransportVariant.extension,
    };
  }

  static num? _numberAt(Map<String, Object?> value, String key) {
    final nested = value[key];
    return nested is num ? nested : null;
  }
}

int _delayForAttempt(BackoffSchedule schedule, int attempt) {
  if (schedule.initialDelayMs <= 0) {
    return 0;
  }
  var delay = schedule.initialDelayMs;
  for (var idx = 1; idx < attempt; idx++) {
    delay *= schedule.multiplier;
    if (delay >= schedule.maxDelayMs) {
      return schedule.maxDelayMs;
    }
  }
  return delay > schedule.maxDelayMs ? schedule.maxDelayMs : delay;
}
