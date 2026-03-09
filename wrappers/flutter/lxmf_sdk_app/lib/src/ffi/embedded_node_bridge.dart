import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

import '../client.dart';
import '../models.dart';
import 'bindings.dart';

class EmbeddedNodeTranslation {
  const EmbeddedNodeTranslation._();

  static RunState mapRunState(int runState) {
    return switch (runState) {
      rnsEmbeddedV1RunStateRunning => RunState.running,
      rnsEmbeddedV1RunStateStopped => RunState.stopped,
      _ => RunState.failed,
    };
  }

  static Severity severityFromLogLevel(int logLevel) {
    return switch (logLevel) {
      0 => Severity.error,
      1 => Severity.warn,
      2 => Severity.info,
      3 => Severity.debug,
      4 => Severity.debug,
      _ => Severity.unknown,
    };
  }

  static AppEvent mapEvent(
    RnsEmbeddedV1NodeEvent event, {
    required Profile profile,
  }) {
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
        severity: severityFromLogLevel(event.logLevel),
        profileId: profile.id,
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

  static AppEvent syntheticEvent({
    required EventKind kind,
    required String rawEventType,
    required int epoch,
    required int eventId,
    required Profile profile,
    StreamGapDetails? streamGap,
  }) {
    return AppEvent(
      metadata: EventMetadata(
        eventId: '$eventId',
        runtimeId: 'embedded-node-$epoch',
        seqNo: eventId,
        occurredAtMs: DateTime.now().millisecondsSinceEpoch,
        severity: Severity.info,
        profileId: profile.id,
      ),
      kind: kind,
      rawEventType: rawEventType,
      streamGap: streamGap,
    );
  }

  static AppError statusError(
    int status, {
    int? nodeErrorCode,
  }) {
    final mapped = mapNodeError(nodeErrorCode, status);
    return AppError(
      code: mapped.$1,
      category: mapped.$2,
      message: mapped.$3,
      retryable: mapped.$4,
      terminal: mapped.$5,
      causeCode: nodeErrorCode == null
          ? null
          : 'RNS_EMBEDDED_V1_NODE_ERROR_$nodeErrorCode',
      details: <String, Object?>{
        'ffiStatus': status,
        'nodeErrorCode': nodeErrorCode
      },
    );
  }

  static (ErrorCode, ErrorCategory, String, bool, bool) mapNodeError(
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
}

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
        runtimeId: runtimeStatus.runtimeId ??
            'embedded-node-${runtimeStatus.state.name}',
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
        state: EmbeddedNodeTranslation.mapRunState(native.runState),
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
      destinationPtr
          .asTypedList(destinationBytes.length)
          .setAll(0, destinationBytes);
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
    throw const AppError(
      code: ErrorCode.capabilityRequiredFeatureMissing,
      category: ErrorCategory.capability,
      message:
          'operationRegistry is not supported by the experimental embedded binding',
      userActionRequired: true,
    );
  }

  @override
  Future<EnvelopeResponse> executeEnvelope(Envelope envelope) async {
    throw AppError(
      code: ErrorCode.capabilityRequiredFeatureMissing,
      category: ErrorCategory.capability,
      message:
          'executeEnvelope is not supported by the experimental embedded binding',
      userActionRequired: true,
      details: <String, Object?>{'operationId': envelope.operationId},
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
      final status =
          _api.subscriptionNext(subscription, 100, pollPtr, eventPtr, errorPtr);
      _throwIfNeeded(status, errorPtr);
      final poll = pollPtr.ref;
      switch (poll.kind) {
        case rnsEmbeddedV1PollTimeout:
          return null;
        case rnsEmbeddedV1PollClosed:
          throw const _BridgeClosed();
        case rnsEmbeddedV1PollGap:
          return EmbeddedNodeTranslation.syntheticEvent(
            kind: EventKind.streamGapDetected,
            rawEventType: 'poll_gap',
            epoch: poll.epoch,
            eventId: poll.nextEventId,
            profile: _activeConfig?.profile ?? Profile.testingDefault,
            streamGap: const StreamGapDetails(droppedCount: 1),
          );
        case rnsEmbeddedV1PollNodeStopped:
          return EmbeddedNodeTranslation.syntheticEvent(
            kind: EventKind.runtimeStopped,
            rawEventType: 'poll_node_stopped',
            epoch: poll.epoch,
            eventId: poll.nextEventId,
            profile: _activeConfig?.profile ?? Profile.testingDefault,
          );
        case rnsEmbeddedV1PollNodeRestarted:
          return EmbeddedNodeTranslation.syntheticEvent(
            kind: EventKind.runtimeRecovered,
            rawEventType: 'poll_node_restarted',
            epoch: poll.epoch,
            eventId: poll.nextEventId,
            profile: _activeConfig?.profile ?? Profile.testingDefault,
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
    return EmbeddedNodeTranslation.mapEvent(
      event,
      profile: _activeConfig?.profile ?? Profile.testingDefault,
    );
  }

  void _writeConfig(RnsEmbeddedV1NodeConfig out, Config config) {
    final native = _api.nodeConfigDefault();
    out.structSize = sizeOf<RnsEmbeddedV1NodeConfig>();
    out.structVersion = native.structVersion == 0 ? 1 : native.structVersion;
    _copyArray(native.storeIdentity, out.storeIdentity, 32);
    _copyArray(native.lxmfAddress, out.lxmfAddress, 16);
    out.nodeMode = switch (config.transportMode) {
      TransportMode.bleOnly => rnsEmbeddedNodeModeBleOnly,
      TransportMode.tcpClient => rnsEmbeddedNodeModeTcpClient,
      TransportMode.tcpServer => rnsEmbeddedNodeModeTcpServer,
    };
    out.announceIntervalMs = native.announceIntervalMs;
    out.maxOutboundQueue = native.maxOutboundQueue;
    out.maxEvents = config.eventBatchSize ?? native.maxEvents;
    out.captureDefaultMaxBytes = native.captureDefaultMaxBytes;
    out.bleMtuHint = native.bleMtuHint;
    out.bleMaxInboundFrames = native.bleMaxInboundFrames;
    out.bleMaxOutboundFrames = native.bleMaxOutboundFrames;
    out.bleOrderedDelivery = native.bleOrderedDelivery;
    _copyArray(native.tcpHost, out.tcpHost, 256);
    out.tcpPort = native.tcpPort;
    out.tcpListenPort = native.tcpListenPort;
    _writeTcpConfig(out, config);
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
    return EmbeddedNodeTranslation.statusError(
      status,
      nodeErrorCode: errorPtr?.ref.code,
    );
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

  static String _defaultLibraryPath() {
    final override = Platform.environment['RNS_EMBEDDED_FFI_LIB'];
    if (override != null && override.isNotEmpty) {
      return override;
    }
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
        (index) =>
            int.parse(compact.substring(index * 2, index * 2 + 2), radix: 16),
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

  static void _copyArray(
      Array<Uint8> source, Array<Uint8> destination, int length) {
    for (var index = 0; index < length; index++) {
      destination[index] = source[index];
    }
  }

  static void _writeTcpConfig(RnsEmbeddedV1NodeConfig out, Config config) {
    switch (config.transportMode) {
      case TransportMode.bleOnly:
        break;
      case TransportMode.tcpClient:
        final host = config.tcpHost;
        final port = config.tcpPort;
        if (host == null ||
            host.isEmpty ||
            port == null ||
            port <= 0 ||
            port > 65535) {
          throw const AppError(
            code: ErrorCode.configInvalid,
            category: ErrorCategory.config,
            message: 'tcp client mode requires a host and valid port',
            userActionRequired: true,
          );
        }
        _writeCString(host, out.tcpHost, 256);
        out.tcpPort = port;
      case TransportMode.tcpServer:
        final listenPort = config.tcpListenPort;
        if (listenPort == null || listenPort <= 0 || listenPort > 65535) {
          throw const AppError(
            code: ErrorCode.configInvalid,
            category: ErrorCategory.config,
            message: 'tcp server mode requires a valid listen port',
            userActionRequired: true,
          );
        }
        out.tcpListenPort = listenPort;
    }
  }

  static void _writeCString(
      String value, Array<Uint8> destination, int maxBytes) {
    final bytes = utf8.encode(value);
    if (bytes.length >= maxBytes) {
      throw const AppError(
        code: ErrorCode.validationInvalidArgument,
        category: ErrorCategory.validation,
        message: 'string exceeds native buffer limit',
        userActionRequired: true,
      );
    }
    for (var index = 0; index < maxBytes; index++) {
      destination[index] = 0;
    }
    for (var index = 0; index < bytes.length; index++) {
      destination[index] = bytes[index];
    }
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
}

class _BridgeClosed implements Exception {
  const _BridgeClosed();
}
