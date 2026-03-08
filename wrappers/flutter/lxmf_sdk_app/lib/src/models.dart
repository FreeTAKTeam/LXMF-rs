import 'package:meta/meta.dart';

enum Profile {
  mobileDefault('mobile_default'),
  desktopDefault('desktop_default'),
  embeddedDefault('embedded_default'),
  testingDefault('testing_default');

  const Profile(this.id);
  final String id;

  DeliveryPlan defaults({int? eventBatchSize}) {
    return switch (this) {
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
          defaultEventBatchSize: eventBatchSize ?? 32,
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
          defaultEventBatchSize: eventBatchSize ?? 64,
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
          defaultEventBatchSize: eventBatchSize ?? 16,
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
          defaultEventBatchSize: eventBatchSize ?? 16,
          redactionEnabled: true,
        ),
    };
  }
}

enum TransportMode { bleOnly, tcpClient, tcpServer }

enum RunState {
  newState,
  starting,
  running,
  degraded,
  stopping,
  stopped,
  failed
}

enum ErrorCategory {
  validation,
  capability,
  config,
  policy,
  delivery,
  connectivity,
  persistence,
  security,
  timeout,
  runtime,
  internal,
}

enum ErrorCode {
  validationInvalidArgument('SDK_APP_VALIDATION_INVALID_ARGUMENT'),
  validationUnknownField('SDK_APP_VALIDATION_UNKNOWN_FIELD'),
  capabilityUnsupportedProfile('SDK_APP_CAPABILITY_UNSUPPORTED_PROFILE'),
  capabilityRequiredFeatureMissing(
      'SDK_APP_CAPABILITY_REQUIRED_FEATURE_MISSING'),
  configInvalid('SDK_APP_CONFIG_INVALID'),
  runtimeInvalidState('SDK_APP_RUNTIME_INVALID_STATE'),
  runtimeAlreadyRunningDifferentConfig(
    'SDK_APP_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG',
  ),
  runtimeStreamDegraded('SDK_APP_RUNTIME_STREAM_DEGRADED'),
  runtimeNotStarted('SDK_APP_RUNTIME_NOT_STARTED'),
  deliveryQueuePressure('SDK_APP_DELIVERY_QUEUE_PRESSURE'),
  deliveryPartialAcceptance('SDK_APP_DELIVERY_PARTIAL_ACCEPTANCE'),
  deliveryRetryExhausted('SDK_APP_DELIVERY_RETRY_EXHAUSTED'),
  deliveryCancelled('SDK_APP_DELIVERY_CANCELLED'),
  connectivityDisconnected('SDK_APP_CONNECTIVITY_DISCONNECTED'),
  connectivityReconnectFailed('SDK_APP_CONNECTIVITY_RECONNECT_FAILED'),
  persistenceUnavailable('SDK_APP_PERSISTENCE_UNAVAILABLE'),
  persistenceRecoveryRequired('SDK_APP_PERSISTENCE_RECOVERY_REQUIRED'),
  timeoutOperationExpired('SDK_APP_TIMEOUT_OPERATION_EXPIRED'),
  securityAuthRequired('SDK_APP_SECURITY_AUTH_REQUIRED'),
  securityAuthzDenied('SDK_APP_SECURITY_AUTHZ_DENIED'),
  securityRedactionRequired('SDK_APP_SECURITY_REDACTION_REQUIRED'),
  internalUnexpectedFailure('SDK_APP_INTERNAL_UNEXPECTED_FAILURE'),
  unknown('unknown');

  const ErrorCode(this.wireName);
  final String wireName;
}

@immutable
class AppError implements Exception {
  const AppError({
    required this.code,
    required this.category,
    required this.message,
    this.retryable = false,
    this.terminal = true,
    this.userActionRequired = false,
    this.causeCode,
    this.details = const {},
  });

  final ErrorCode code;
  final ErrorCategory category;
  final String message;
  final bool retryable;
  final bool terminal;
  final bool userActionRequired;
  final String? causeCode;
  final Map<String, Object?> details;

  @override
  String toString() => 'AppError(${code.wireName}, $message)';
}

@immutable
class CapabilitySummary {
  const CapabilitySummary({
    required this.activeContractVersion,
    required this.effectiveCapabilities,
    required this.effectiveLimits,
  });

  final int activeContractVersion;
  final List<String> effectiveCapabilities;
  final Map<String, Object?> effectiveLimits;
}

@immutable
class Handle {
  const Handle({
    required this.runtimeId,
    required this.profile,
    required this.capabilities,
  });

  final String runtimeId;
  final Profile profile;
  final CapabilitySummary capabilities;
}

@immutable
class RuntimeStatus {
  const RuntimeStatus({
    required this.state,
    this.runtimeId,
    this.profile,
    this.capabilities,
    this.queuedMessages = 0,
    this.inFlightMessages = 0,
    this.eventStreamPosition = 0,
    this.configRevision = 0,
  });

  final String? runtimeId;
  final RunState state;
  final Profile? profile;
  final CapabilitySummary? capabilities;
  final int queuedMessages;
  final int inFlightMessages;
  final int eventStreamPosition;
  final int configRevision;
}

@immutable
class Config {
  const Config({
    required this.profile,
    this.supportedContractVersions = const [2],
    this.requestedCapabilities = const [],
    this.eventBatchSize,
    this.transportMode = TransportMode.bleOnly,
    this.tcpHost,
    this.tcpPort,
    this.tcpListenPort,
    this.sdkConfig = const {},
  });

  final Profile profile;
  final List<int> supportedContractVersions;
  final List<String> requestedCapabilities;
  final int? eventBatchSize;
  final TransportMode transportMode;
  final String? tcpHost;
  final int? tcpPort;
  final int? tcpListenPort;
  final Map<String, Object?> sdkConfig;

  factory Config.fromProfile(Profile profile) {
    return switch (profile) {
      Profile.mobileDefault => const Config(
          profile: Profile.mobileDefault,
          eventBatchSize: 32,
        ),
      Profile.desktopDefault => const Config(
          profile: Profile.desktopDefault,
          eventBatchSize: 64,
        ),
      Profile.embeddedDefault => const Config(
          profile: Profile.embeddedDefault,
          eventBatchSize: 16,
        ),
      Profile.testingDefault => const Config(
          profile: Profile.testingDefault,
          eventBatchSize: 16,
        ),
    };
  }

  DeliveryPlan deliveryPlan() =>
      profile.defaults(eventBatchSize: eventBatchSize);
}

@immutable
class SendRequest {
  const SendRequest({
    required this.source,
    required this.destination,
    required this.payload,
    this.idempotencyKey,
    this.ttlMs,
    this.correlationId,
    this.extensions = const {},
  });

  final String source;
  final String destination;
  final Object? payload;
  final String? idempotencyKey;
  final int? ttlMs;
  final String? correlationId;
  final Map<String, Object?> extensions;
}

@immutable
class SendReceipt {
  const SendReceipt({
    required this.runtimeId,
    required this.messageId,
    required this.profile,
    this.correlationId,
  });

  final String runtimeId;
  final String messageId;
  final Profile profile;
  final String? correlationId;
}

enum QueuePressureStrategy { failFast, retry }

@immutable
class BackoffSchedule {
  const BackoffSchedule({
    required this.initialDelayMs,
    required this.multiplier,
    required this.maxDelayMs,
  });

  final int initialDelayMs;
  final int multiplier;
  final int maxDelayMs;
}

@immutable
class RetryPolicy {
  const RetryPolicy({required this.maxAttempts, required this.backoff});

  final int maxAttempts;
  final BackoffSchedule backoff;
}

@immutable
class ReconnectPolicy {
  const ReconnectPolicy({
    required this.enabled,
    required this.backoff,
    this.maxAttempts,
  });

  final bool enabled;
  final int? maxAttempts;
  final BackoffSchedule backoff;
}

@immutable
class QueuePressurePolicy {
  const QueuePressurePolicy({
    required this.strategy,
    required this.maxAttempts,
    required this.backoff,
  });

  final QueuePressureStrategy strategy;
  final int maxAttempts;
  final BackoffSchedule backoff;
}

@immutable
class TimeoutPolicy {
  const TimeoutPolicy({
    this.sendTimeoutMs,
    this.eventNextTimeoutMs,
    this.reconnectGraceMs,
  });

  final int? sendTimeoutMs;
  final int? eventNextTimeoutMs;
  final int? reconnectGraceMs;
}

@immutable
class DeliveryPlan {
  const DeliveryPlan({
    required this.profile,
    required this.retry,
    required this.reconnect,
    required this.queuePressure,
    required this.timeout,
    required this.durableQueueing,
    required this.restartRecovery,
    required this.defaultEventBatchSize,
    required this.redactionEnabled,
  });

  final Profile profile;
  final RetryPolicy retry;
  final ReconnectPolicy reconnect;
  final QueuePressurePolicy queuePressure;
  final TimeoutPolicy timeout;
  final bool durableQueueing;
  final bool restartRecovery;
  final int defaultEventBatchSize;
  final bool redactionEnabled;
}

@immutable
class DeliveryOptions {
  const DeliveryOptions({
    this.maxAttempts,
    this.timeoutMs,
    this.queuePressureStrategy,
  });

  final int? maxAttempts;
  final int? timeoutMs;
  final QueuePressureStrategy? queuePressureStrategy;
}

enum AttemptDisposition { retried, failed }

@immutable
class DeliveryAttempt {
  const DeliveryAttempt({
    required this.attempt,
    required this.disposition,
    required this.errorCode,
    required this.retryable,
    required this.queuePressure,
    this.scheduledDelayMs,
  });

  final int attempt;
  final AttemptDisposition disposition;
  final String errorCode;
  final bool retryable;
  final bool queuePressure;
  final int? scheduledDelayMs;
}

@immutable
class SendReport {
  const SendReport({
    required this.receipt,
    required this.attempts,
    required this.totalDelayMs,
    required this.plan,
  });

  final SendReceipt receipt;
  final List<DeliveryAttempt> attempts;
  final int totalDelayMs;
  final DeliveryPlan plan;
}

enum Severity { debug, info, warn, error, critical, unknown }

enum EventKind {
  runtimeStarted,
  runtimeStopped,
  runtimeDegraded,
  runtimeRecovered,
  messageQueued,
  messageDispatching,
  messageSent,
  messageDelivered,
  messageFailed,
  messageCancelled,
  inboundMessageReceived,
  queuePressureRaised,
  retryScheduled,
  reconnectScheduled,
  streamGapDetected,
  securityActionRequired,
  fatalErrorRaised,
  unknown,
}

@immutable
class StreamGapDetails {
  const StreamGapDetails({
    this.expectedSeqNo,
    this.observedSeqNo,
    this.droppedCount = 0,
    this.recoveryRequired = true,
  });

  final int? expectedSeqNo;
  final int? observedSeqNo;
  final int droppedCount;
  final bool recoveryRequired;
}

@immutable
class EventMetadata {
  const EventMetadata({
    required this.eventId,
    required this.runtimeId,
    required this.seqNo,
    required this.occurredAtMs,
    required this.severity,
    required this.profileId,
    this.operationId,
    this.messageId,
    this.correlationId,
  });

  final String eventId;
  final String runtimeId;
  final int seqNo;
  final int occurredAtMs;
  final Severity severity;
  final String profileId;
  final String? operationId;
  final String? messageId;
  final String? correlationId;
}

@immutable
class AppEvent {
  const AppEvent({
    required this.metadata,
    required this.kind,
    required this.rawEventType,
    this.details,
    this.extensions = const {},
    this.streamGap,
  });

  final EventMetadata metadata;
  final EventKind kind;
  final String rawEventType;
  final Object? details;
  final Map<String, Object?> extensions;
  final StreamGapDetails? streamGap;
}
