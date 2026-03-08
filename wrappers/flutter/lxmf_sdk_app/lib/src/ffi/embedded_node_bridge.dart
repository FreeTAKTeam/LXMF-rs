import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

import '../client.dart';
import '../models.dart';
import 'bindings.dart';

class EmbeddedNodeBridge implements AppBinding {
  EmbeddedNodeBridge(this._api) : _node = _api.nodeNew() {
    if (_node == nullptr) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'failed to allocate embedded node handle',
      );
    }
  }

  factory EmbeddedNodeBridge.open({String? libraryPath}) {
    final library = libraryPath == null
        ? DynamicLibrary.open(_defaultLibraryPath())
        : DynamicLibrary.open(libraryPath);
    return EmbeddedNodeBridge(EmbeddedFfiApi(library));
  }

  final EmbeddedFfiApi _api;
  final Pointer<RnsEmbeddedV1Node> _node;

  Config? _activeConfig;
  Pointer<RnsEmbeddedEventSubscription>? _subscription;
  bool _disposed = false;

  @override
  Future<Handle> start(Config config) async {
    _ensureNotDisposed();

    final configPtr = calloc<RnsEmbeddedV1NodeConfig>();
    final errorPtr = _newNodeError();
    try {
      _writeConfig(configPtr.ref, config);
      final status = _api.nodeStart(_node, configPtr, errorPtr);
      _throwIfNeeded(status, errorPtr);

      _activeConfig = config;
      final runtimeStatus = await this.status();
      final capabilities = _readCapabilities();
      return Handle(
        runtimeId: runtimeStatus.runtimeId ?? 'embedded-node-${runtimeStatus.state.name}',
        profile: config.profile,
        capabilities: capabilities,
      );
    } finally {
      calloc.free(configPtr);
      calloc.free(errorPtr);
    }
  }

  @override
  Future<void> stop() async {
    _ensureNotDisposed();

    final errorPtr = _newNodeError();
    try {
      final status = _api.nodeStop(_node, errorPtr);
      _throwIfNeeded(status, errorPtr);
      if (_subscription != null && _subscription != nullptr) {
        await _closeSubscription();
      }
      _activeConfig = null;
    } finally {
      calloc.free(errorPtr);
    }
  }

  @override
  Future<RuntimeStatus> status() async {
    _ensureNotDisposed();

    final statusPtr = calloc<RnsEmbeddedV1NodeStatus>();
    statusPtr.ref.structSize = sizeOf<RnsEmbeddedV1NodeStatus>();
    statusPtr.ref.structVersion = 1;
    try {
      final status = _api.nodeGetStatus(_node, statusPtr);
      if (status != rnsEmbeddedStatusOk) {
        throw _statusError(status, null);
      }
      final native = statusPtr.ref;
      return RuntimeStatus(
        runtimeId: 'embedded-node-${native.epoch}',
        state: _mapRunState(native.runState),
        profile: _activeConfig?.profile,
        capabilities: _activeConfig == null ? null : _readCapabilities(),
        queuedMessages: native.pendingOutbound,
        inFlightMessages: native.outboundDeferred,
        eventStreamPosition: native.outboundSent,
        configRevision: native.epoch,
      );
    } finally {
      calloc.free(statusPtr);
    }
  }

  @override
  Future<SendReceipt> send(SendRequest request) async {
    _ensureStarted();

    final receiptPtr = calloc<RnsEmbeddedV1SendReceipt>();
    receiptPtr.ref.structSize = sizeOf<RnsEmbeddedV1SendReceipt>();
    receiptPtr.ref.structVersion = 1;
    final errorPtr = _newNodeError();
    final destinationBytes = _parseDestination(request.destination);
    final bodyBytes = _encodePayload(request.payload);
    final destinationPtr = calloc<Uint8>(destinationBytes.length);
    final bodyPtr = calloc<Uint8>(bodyBytes.length);
    try {
      destinationPtr.asTypedList(destinationBytes.length).setAll(0, destinationBytes);
      bodyPtr.asTypedList(bodyBytes.length).setAll(0, bodyBytes);
      final status = _api.nodeSend(
        _node,
        destinationPtr,
        bodyPtr,
        bodyBytes.length,
        receiptPtr,
        errorPtr,
      );
      _throwIfNeeded(status, errorPtr);

      final receipt = receiptPtr.ref;
      return SendReceipt(
        runtimeId: 'embedded-node-${receipt.epoch}',
        messageId: '${receipt.operationId}',
        profile: _activeConfig!.profile,
        correlationId: request.correlationId,
      );
    } finally {
      calloc.free(destinationPtr);
      calloc.free(bodyPtr);
      calloc.free(receiptPtr);
      calloc.free(errorPtr);
    }
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
    final config = _activeConfig;
    if (config == null) {
      throw const AppError(
        code: ErrorCode.runtimeNotStarted,
        category: ErrorCategory.runtime,
        message: 'embedded node is not started',
      );
    }

    final plan = _deliveryPlanForConfig(config);
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
        final canRetry =
            (isQueuePressure && queuePressureStrategy == QueuePressureStrategy.retry) ||
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
  Stream<AppEvent> subscribeEvents() {
    _ensureStarted();

    return Stream<AppEvent>.multi((controller) async {
      final subscription = await _openSubscription();
      var active = true;

      controller.onCancel = () async {
        active = false;
        await _closeSubscription();
      };

      while (active) {
        try {
          final event = _pollNextEvent(subscription);
          if (event != null) {
            controller.add(event);
          }
        } on _BridgeClosed {
          active = false;
          await controller.close();
        } on AppError catch (error) {
          controller.addError(error);
          active = false;
          await _closeSubscription();
          rethrow;
        }
      }
    });
  }

  Future<void> dispose() async {
    if (_disposed) {
      return;
    }
    if (_subscription != null && _subscription != nullptr) {
      await _closeSubscription();
    }
    _activeConfig = null;
    _api.nodeFree(_node);
    _disposed = true;
  }

  CapabilitySummary _readCapabilities() {
    final capabilitiesPtr = calloc<RnsEmbeddedV1Capabilities>();
    capabilitiesPtr.ref.structSize = sizeOf<RnsEmbeddedV1Capabilities>();
    capabilitiesPtr.ref.structVersion = 1;
    try {
      final status = _api.getCapabilities(capabilitiesPtr);
      if (status != rnsEmbeddedStatusOk) {
        throw _statusError(status, null);
      }
      final native = capabilitiesPtr.ref;
      return CapabilitySummary(
        activeContractVersion: native.abiVersion,
        effectiveCapabilities: <String>[
          'schema:${native.capabilitySchemaVersion}',
          'bits:${native.capabilityBits}',
        ],
        effectiveLimits: <String, Object?>{
          'maxEventPayloadBytes': native.maxEventPayloadBytes,
          'maxSubscriptions': native.maxSubscriptions,
          'maxBlockingTimeoutMs': native.maxBlockingTimeoutMs,
          'driverTickTargetMs': native.driverTickTargetMs,
          'driverTickMaxMs': native.driverTickMaxMs,
        },
      );
    } finally {
      calloc.free(capabilitiesPtr);
    }
  }

  Future<Pointer<RnsEmbeddedEventSubscription>> _openSubscription() async {
    if (_subscription != null && _subscription != nullptr) {
      return _subscription!;
    }

    final subscriptionPtr = calloc<Pointer<RnsEmbeddedEventSubscription>>();
    final errorPtr = _newNodeError();
    try {
      final status = _api.nodeSubscribeEvents(_node, subscriptionPtr, errorPtr);
      _throwIfNeeded(status, errorPtr);
      _subscription = subscriptionPtr.value;
      return _subscription!;
    } finally {
      calloc.free(subscriptionPtr);
      calloc.free(errorPtr);
    }
  }

  Future<void> _closeSubscription() async {
    final subscription = _subscription;
    if (subscription == null || subscription == nullptr) {
      _subscription = null;
      return;
    }

    final errorPtr = _newNodeError();
    try {
      final status = _api.subscriptionClose(subscription, errorPtr);
      if (status != rnsEmbeddedStatusOk && errorPtr.ref.code != 12) {
        _throwIfNeeded(status, errorPtr);
      }
    } finally {
      _subscription = null;
      calloc.free(errorPtr);
    }
  }

  AppEvent? _pollNextEvent(Pointer<RnsEmbeddedEventSubscription> subscription) {
    final pollPtr = calloc<RnsEmbeddedV1PollResult>();
    final eventPtr = calloc<RnsEmbeddedV1NodeEvent>();
    final errorPtr = _newNodeError();
    pollPtr.ref.structSize = sizeOf<RnsEmbeddedV1PollResult>();
    pollPtr.ref.structVersion = 1;
    eventPtr.ref.structSize = sizeOf<RnsEmbeddedV1NodeEvent>();
    eventPtr.ref.structVersion = 1;
    try {
      final status = _api.subscriptionNext(subscription, 100, pollPtr, eventPtr, errorPtr);
      _throwIfNeeded(status, errorPtr);
      final poll = pollPtr.ref;
      switch (poll.kind) {
        case rnsEmbeddedV1PollTimeout:
          return null;
        case rnsEmbeddedV1PollClosed:
          throw const _BridgeClosed();
        case rnsEmbeddedV1PollGap:
          return _syntheticEvent(
            kind: EventKind.streamGapDetected,
            rawEventType: 'poll_gap',
            epoch: poll.epoch,
            eventId: poll.nextEventId,
            streamGap: const StreamGapDetails(droppedCount: 1),
          );
        case rnsEmbeddedV1PollNodeStopped:
          return _syntheticEvent(
            kind: EventKind.runtimeStopped,
            rawEventType: 'poll_node_stopped',
            epoch: poll.epoch,
            eventId: poll.nextEventId,
          );
        case rnsEmbeddedV1PollNodeRestarted:
          return _syntheticEvent(
            kind: EventKind.runtimeRecovered,
            rawEventType: 'poll_node_restarted',
            epoch: poll.epoch,
            eventId: poll.nextEventId,
          );
        case rnsEmbeddedV1PollEvent:
          return _mapEvent(eventPtr.ref);
      }
      return null;
    } finally {
      calloc.free(pollPtr);
      calloc.free(eventPtr);
      calloc.free(errorPtr);
    }
  }

  AppEvent _mapEvent(RnsEmbeddedV1NodeEvent event) {
    final kind = switch (event.kind) {
      rnsEmbeddedV1EventStatusChanged => EventKind.runtimeStarted,
      rnsEmbeddedV1EventLog => EventKind.unknown,
      rnsEmbeddedV1EventError => EventKind.fatalErrorRaised,
      rnsEmbeddedV1EventPacketReceived => EventKind.inboundMessageReceived,
      rnsEmbeddedV1EventPacketSent => EventKind.messageSent,
      rnsEmbeddedV1EventExtension => EventKind.unknown,
      _ => EventKind.unknown,
    };

    return AppEvent(
      metadata: EventMetadata(
        eventId: '${event.eventId}',
        runtimeId: 'embedded-node-${event.epoch}',
        seqNo: event.eventId,
        occurredAtMs: event.occurredAtMs,
        severity: _severityFromLogLevel(event.logLevel),
        profileId: _activeConfig?.profile.id ?? Profile.testingDefault.id,
        operationId: event.hasOperationId ? '${event.operationId}' : null,
      ),
      kind: kind,
      rawEventType: 'native_v1_${event.kind}',
      details: <String, Object?>{
        'epoch': event.epoch,
        'lifecycleState': event.lifecycleState,
        'runState': event.runState,
        'errorCode': event.errorCode,
        'frameKind': event.frameKind,
        'sequence': event.sequence,
        'bytes': event.bytes,
        'extensionId': event.extensionId,
        'value0': event.value0,
        'value1': event.value1,
      },
    );
  }

  AppEvent _syntheticEvent({
    required EventKind kind,
    required String rawEventType,
    required int epoch,
    required int eventId,
    StreamGapDetails? streamGap,
  }) {
    return AppEvent(
      metadata: EventMetadata(
        eventId: '$eventId',
        runtimeId: 'embedded-node-$epoch',
        seqNo: eventId,
        occurredAtMs: DateTime.now().millisecondsSinceEpoch,
        severity: Severity.info,
        profileId: _activeConfig?.profile.id ?? Profile.testingDefault.id,
      ),
      kind: kind,
      rawEventType: rawEventType,
      streamGap: streamGap,
    );
  }

  void _writeConfig(RnsEmbeddedV1NodeConfig out, Config config) {
    final native = _api.nodeConfigDefault();
    out.structSize = sizeOf<RnsEmbeddedV1NodeConfig>();
    out.structVersion = native.structVersion == 0 ? 1 : native.structVersion;
    _copyArray(native.storeIdentity, out.storeIdentity, 32);
    _copyArray(native.lxmfAddress, out.lxmfAddress, 16);
    out.nodeMode = native.nodeMode;
    out.announceIntervalMs = native.announceIntervalMs;
    out.maxOutboundQueue = native.maxOutboundQueue;
    out.maxEvents = config.eventBatchSize ?? native.maxEvents;
    out.captureDefaultMaxBytes = native.captureDefaultMaxBytes;
    out.bleMtuHint = native.bleMtuHint;
    out.bleMaxInboundFrames = native.bleMaxInboundFrames;
    out.bleMaxOutboundFrames = native.bleMaxOutboundFrames;
    out.bleOrderedDelivery = native.bleOrderedDelivery;
  }

  Pointer<RnsEmbeddedV1NodeError> _newNodeError() {
    final errorPtr = calloc<RnsEmbeddedV1NodeError>();
    errorPtr.ref.structSize = sizeOf<RnsEmbeddedV1NodeError>();
    errorPtr.ref.structVersion = 1;
    return errorPtr;
  }

  void _throwIfNeeded(
    int status,
    Pointer<RnsEmbeddedV1NodeError>? errorPtr,
  ) {
    if (status == rnsEmbeddedStatusOk) {
      return;
    }
    throw _statusError(status, errorPtr);
  }

  AppError _statusError(int status, Pointer<RnsEmbeddedV1NodeError>? errorPtr) {
    final nodeErrorCode = errorPtr?.ref.code;
    final mapped = _mapNodeError(nodeErrorCode, status);
    return AppError(
      code: mapped.$1,
      category: mapped.$2,
      message: mapped.$3,
      retryable: mapped.$4,
      terminal: mapped.$5,
      causeCode: nodeErrorCode == null ? null : 'RNS_EMBEDDED_V1_NODE_ERROR_$nodeErrorCode',
      details: <String, Object?>{'ffiStatus': status, 'nodeErrorCode': nodeErrorCode},
    );
  }

  (ErrorCode, ErrorCategory, String, bool, bool) _mapNodeError(
    int? nodeErrorCode,
    int ffiStatus,
  ) {
    switch (nodeErrorCode) {
      case 1:
        return (
          ErrorCode.configInvalid,
          ErrorCategory.config,
          'invalid embedded node configuration',
          false,
          true,
        );
      case 3:
        return (
          ErrorCode.connectivityDisconnected,
          ErrorCategory.connectivity,
          'embedded node backend is disconnected',
          true,
          false,
        );
      case 6:
        return (
          ErrorCode.runtimeNotStarted,
          ErrorCategory.runtime,
          'embedded node is not running',
          false,
          true,
        );
      case 7:
        return (
          ErrorCode.timeoutOperationExpired,
          ErrorCategory.timeout,
          'embedded node wait timed out',
          false,
          true,
        );
      case 11:
        return (
          ErrorCode.runtimeInvalidState,
          ErrorCategory.runtime,
          'embedded node mode conflict',
          false,
          true,
        );
      case 15:
        return (
          ErrorCode.deliveryQueuePressure,
          ErrorCategory.delivery,
          'embedded node queue pressure',
          true,
          false,
        );
    }

    switch (ffiStatus) {
      case rnsEmbeddedStatusTimeout:
        return (
          ErrorCode.timeoutOperationExpired,
          ErrorCategory.timeout,
          'embedded node operation timed out',
          false,
          true,
        );
      case rnsEmbeddedStatusBackpressure:
        return (
          ErrorCode.deliveryQueuePressure,
          ErrorCategory.delivery,
          'embedded node reported backpressure',
          true,
          false,
        );
      case rnsEmbeddedStatusDisconnected:
        return (
          ErrorCode.connectivityDisconnected,
          ErrorCategory.connectivity,
          'embedded node reported disconnection',
          true,
          false,
        );
      default:
        return (
          ErrorCode.internalUnexpectedFailure,
          ErrorCategory.internal,
          'embedded node ffi operation failed',
          false,
          true,
        );
    }
  }

  void _ensureNotDisposed() {
    if (_disposed) {
      throw const AppError(
        code: ErrorCode.internalUnexpectedFailure,
        category: ErrorCategory.internal,
        message: 'embedded node bridge has been disposed',
      );
    }
  }

  void _ensureStarted() {
    _ensureNotDisposed();
    if (_activeConfig == null) {
      throw const AppError(
        code: ErrorCode.runtimeNotStarted,
        category: ErrorCategory.runtime,
        message: 'embedded node is not started',
      );
    }
  }

  static RunState _mapRunState(int runState) {
    return switch (runState) {
      rnsEmbeddedV1RunStateRunning => RunState.running,
      rnsEmbeddedV1RunStateStopped => RunState.stopped,
      _ => RunState.failed,
    };
  }

  static String _defaultLibraryPath() {
    if (Platform.isMacOS || Platform.isIOS) {
      return 'librns_embedded_ffi.dylib';
    }
    if (Platform.isAndroid || Platform.isLinux) {
      return 'librns_embedded_ffi.so';
    }
    if (Platform.isWindows) {
      return 'rns_embedded_ffi.dll';
    }
    throw UnsupportedError('unsupported platform for embedded ffi loading');
  }

  static List<int> _parseDestination(String destination) {
    final compact = destination.replaceAll(':', '').replaceAll('-', '');
    if (_isHex32(compact)) {
      return List<int>.generate(
        16,
        (index) => int.parse(compact.substring(index * 2, index * 2 + 2), radix: 16),
      );
    }

    final raw = utf8.encode(destination);
    if (raw.length == 16) {
      return raw;
    }

    throw const AppError(
      code: ErrorCode.validationInvalidArgument,
      category: ErrorCategory.validation,
      message: 'destination must be 16 raw bytes or 32 hex characters',
      userActionRequired: true,
    );
  }

  static bool _isHex32(String value) =>
      value.length == 32 && RegExp(r'^[0-9a-fA-F]{32}$').hasMatch(value);

  static List<int> _encodePayload(Object? payload) {
    if (payload is String) {
      return utf8.encode(payload);
    }
    return utf8.encode(jsonEncode(payload));
  }

  static void _copyArray(Array<Uint8> source, Array<Uint8> destination, int length) {
    for (var index = 0; index < length; index++) {
      destination[index] = source[index];
    }
  }

  static Severity _severityFromLogLevel(int logLevel) {
    return switch (logLevel) {
      0 => Severity.error,
      1 => Severity.warn,
      2 => Severity.info,
      3 => Severity.debug,
      4 => Severity.debug,
      _ => Severity.unknown,
    };
  }

  static int _delayForAttempt(BackoffSchedule backoff, int attempt) {
    var delay = backoff.initialDelayMs;
    for (var index = 1; index < attempt; index++) {
      delay *= backoff.multiplier < 1 ? 1 : backoff.multiplier;
      if (delay >= backoff.maxDelayMs) {
        return backoff.maxDelayMs;
      }
    }
    return delay > backoff.maxDelayMs ? backoff.maxDelayMs : delay;
  }

  static DeliveryPlan _deliveryPlanForConfig(Config config) {
    return switch (config.profile) {
      Profile.mobileDefault => DeliveryPlan(
        profile: Profile.mobileDefault,
        retry: const RetryPolicy(
          maxAttempts: 3,
          backoff: BackoffSchedule(
            initialDelayMs: 250,
            multiplier: 2,
            maxDelayMs: 2000,
          ),
        ),
        reconnect: const ReconnectPolicy(
          enabled: true,
          maxAttempts: 5,
          backoff: BackoffSchedule(
            initialDelayMs: 500,
            multiplier: 2,
            maxDelayMs: 10000,
          ),
        ),
        queuePressure: const QueuePressurePolicy(
          strategy: QueuePressureStrategy.retry,
          maxAttempts: 3,
          backoff: BackoffSchedule(
            initialDelayMs: 100,
            multiplier: 2,
            maxDelayMs: 750,
          ),
        ),
        timeout: const TimeoutPolicy(
          sendTimeoutMs: 5000,
          eventNextTimeoutMs: 1000,
          reconnectGraceMs: 15000,
        ),
        durableQueueing: false,
        restartRecovery: false,
        defaultEventBatchSize: config.eventBatchSize ?? 32,
        redactionEnabled: true,
      ),
      Profile.desktopDefault => DeliveryPlan(
        profile: Profile.desktopDefault,
        retry: const RetryPolicy(
          maxAttempts: 5,
          backoff: BackoffSchedule(
            initialDelayMs: 200,
            multiplier: 2,
            maxDelayMs: 5000,
          ),
        ),
        reconnect: const ReconnectPolicy(
          enabled: true,
          maxAttempts: 10,
          backoff: BackoffSchedule(
            initialDelayMs: 500,
            multiplier: 2,
            maxDelayMs: 15000,
          ),
        ),
        queuePressure: const QueuePressurePolicy(
          strategy: QueuePressureStrategy.retry,
          maxAttempts: 4,
          backoff: BackoffSchedule(
            initialDelayMs: 100,
            multiplier: 2,
            maxDelayMs: 1000,
          ),
        ),
        timeout: const TimeoutPolicy(
          sendTimeoutMs: 10000,
          eventNextTimeoutMs: 2000,
          reconnectGraceMs: 30000,
        ),
        durableQueueing: false,
        restartRecovery: false,
        defaultEventBatchSize: config.eventBatchSize ?? 64,
        redactionEnabled: true,
      ),
      Profile.embeddedDefault => DeliveryPlan(
        profile: Profile.embeddedDefault,
        retry: const RetryPolicy(
          maxAttempts: 2,
          backoff: BackoffSchedule(
            initialDelayMs: 500,
            multiplier: 2,
            maxDelayMs: 2000,
          ),
        ),
        reconnect: const ReconnectPolicy(
          enabled: false,
          maxAttempts: 1,
          backoff: BackoffSchedule(
            initialDelayMs: 1000,
            multiplier: 1,
            maxDelayMs: 1000,
          ),
        ),
        queuePressure: const QueuePressurePolicy(
          strategy: QueuePressureStrategy.failFast,
          maxAttempts: 1,
          backoff: BackoffSchedule(
            initialDelayMs: 0,
            multiplier: 1,
            maxDelayMs: 0,
          ),
        ),
        timeout: const TimeoutPolicy(
          sendTimeoutMs: 3000,
          eventNextTimeoutMs: 500,
        ),
        durableQueueing: false,
        restartRecovery: false,
        defaultEventBatchSize: config.eventBatchSize ?? 16,
        redactionEnabled: true,
      ),
      Profile.testingDefault => DeliveryPlan(
        profile: Profile.testingDefault,
        retry: const RetryPolicy(
          maxAttempts: 2,
          backoff: BackoffSchedule(
            initialDelayMs: 10,
            multiplier: 1,
            maxDelayMs: 10,
          ),
        ),
        reconnect: const ReconnectPolicy(
          enabled: true,
          maxAttempts: 2,
          backoff: BackoffSchedule(
            initialDelayMs: 25,
            multiplier: 1,
            maxDelayMs: 25,
          ),
        ),
        queuePressure: const QueuePressurePolicy(
          strategy: QueuePressureStrategy.failFast,
          maxAttempts: 1,
          backoff: BackoffSchedule(
            initialDelayMs: 0,
            multiplier: 1,
            maxDelayMs: 0,
          ),
        ),
        timeout: const TimeoutPolicy(
          sendTimeoutMs: 500,
          eventNextTimeoutMs: 100,
          reconnectGraceMs: 250,
        ),
        durableQueueing: false,
        restartRecovery: false,
        defaultEventBatchSize: config.eventBatchSize ?? 16,
        redactionEnabled: true,
      ),
    };
  }
}

class _BridgeClosed implements Exception {
  const _BridgeClosed();
}
