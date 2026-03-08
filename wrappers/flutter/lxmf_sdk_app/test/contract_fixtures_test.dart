import 'dart:convert';
import 'dart:io';

import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';
import 'package:test/test.dart';

void main() {
  final fixtureDir = Directory('../../../docs/fixtures/sdk-app-v1');

  Map<String, dynamic> readFixture(String name) {
    final file = File('${fixtureDir.path}/$name');
    return jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
  }

  const eventKinds = <String, EventKind>{
    'RuntimeStarted': EventKind.runtimeStarted,
    'RuntimeStopped': EventKind.runtimeStopped,
    'RuntimeDegraded': EventKind.runtimeDegraded,
    'RuntimeRecovered': EventKind.runtimeRecovered,
    'MessageQueued': EventKind.messageQueued,
    'MessageDispatching': EventKind.messageDispatching,
    'MessageSent': EventKind.messageSent,
    'MessageDelivered': EventKind.messageDelivered,
    'MessageFailed': EventKind.messageFailed,
    'MessageCancelled': EventKind.messageCancelled,
    'InboundMessageReceived': EventKind.inboundMessageReceived,
    'QueuePressureRaised': EventKind.queuePressureRaised,
    'RetryScheduled': EventKind.retryScheduled,
    'ReconnectScheduled': EventKind.reconnectScheduled,
    'StreamGapDetected': EventKind.streamGapDetected,
    'SecurityActionRequired': EventKind.securityActionRequired,
    'FatalErrorRaised': EventKind.fatalErrorRaised,
  };

  const errorCodes = <String, ErrorCode>{
    'SDK_APP_VALIDATION_INVALID_ARGUMENT': ErrorCode.validationInvalidArgument,
    'SDK_APP_VALIDATION_UNKNOWN_FIELD': ErrorCode.validationUnknownField,
    'SDK_APP_CAPABILITY_UNSUPPORTED_PROFILE': ErrorCode.capabilityUnsupportedProfile,
    'SDK_APP_CAPABILITY_REQUIRED_FEATURE_MISSING':
        ErrorCode.capabilityRequiredFeatureMissing,
    'SDK_APP_CONFIG_INVALID': ErrorCode.configInvalid,
    'SDK_APP_RUNTIME_INVALID_STATE': ErrorCode.runtimeInvalidState,
    'SDK_APP_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG':
        ErrorCode.runtimeAlreadyRunningDifferentConfig,
    'SDK_APP_RUNTIME_STREAM_DEGRADED': ErrorCode.runtimeStreamDegraded,
    'SDK_APP_RUNTIME_NOT_STARTED': ErrorCode.runtimeNotStarted,
    'SDK_APP_DELIVERY_QUEUE_PRESSURE': ErrorCode.deliveryQueuePressure,
    'SDK_APP_DELIVERY_PARTIAL_ACCEPTANCE': ErrorCode.deliveryPartialAcceptance,
    'SDK_APP_DELIVERY_RETRY_EXHAUSTED': ErrorCode.deliveryRetryExhausted,
    'SDK_APP_DELIVERY_CANCELLED': ErrorCode.deliveryCancelled,
    'SDK_APP_CONNECTIVITY_DISCONNECTED': ErrorCode.connectivityDisconnected,
    'SDK_APP_CONNECTIVITY_RECONNECT_FAILED': ErrorCode.connectivityReconnectFailed,
    'SDK_APP_PERSISTENCE_UNAVAILABLE': ErrorCode.persistenceUnavailable,
    'SDK_APP_PERSISTENCE_RECOVERY_REQUIRED': ErrorCode.persistenceRecoveryRequired,
    'SDK_APP_TIMEOUT_OPERATION_EXPIRED': ErrorCode.timeoutOperationExpired,
    'SDK_APP_SECURITY_AUTH_REQUIRED': ErrorCode.securityAuthRequired,
    'SDK_APP_SECURITY_AUTHZ_DENIED': ErrorCode.securityAuthzDenied,
    'SDK_APP_SECURITY_REDACTION_REQUIRED': ErrorCode.securityRedactionRequired,
    'SDK_APP_INTERNAL_UNEXPECTED_FAILURE': ErrorCode.internalUnexpectedFailure,
  };

  const errorCategories = <String, ErrorCategory>{
    'Validation': ErrorCategory.validation,
    'Capability': ErrorCategory.capability,
    'Config': ErrorCategory.config,
    'Policy': ErrorCategory.policy,
    'Delivery': ErrorCategory.delivery,
    'Connectivity': ErrorCategory.connectivity,
    'Persistence': ErrorCategory.persistence,
    'Security': ErrorCategory.security,
    'Timeout': ErrorCategory.timeout,
    'Runtime': ErrorCategory.runtime,
    'Internal': ErrorCategory.internal,
  };

  const profiles = <String, Profile>{
    'mobile_default': Profile.mobileDefault,
    'desktop_default': Profile.desktopDefault,
    'embedded_default': Profile.embeddedDefault,
    'testing_default': Profile.testingDefault,
  };

  test('sdk-app manifest covers required scenarios', () {
    final manifest = readFixture('manifest.json');
    expect(manifest['fixture_schema_version'], 1);
    expect(manifest['contract_family'], 'sdk-app');
    expect(manifest['contract_release'], 'v1');

    final scenarios = (manifest['scenarios'] as List).cast<Map<String, dynamic>>();
    final ids = scenarios.map((scenario) => scenario['id']).toSet();
    expect(
      ids,
      containsAll(<String>{
        'lifecycle.start_stop_restart',
        'events.delivery_ordering',
        'timeout.poll_timeout',
        'delivery.queue_pressure',
        'connectivity.reconnect_recovery',
        'errors.typed_mapping',
        'compatibility.unknown_additive',
      }),
    );

    for (final scenario in scenarios) {
      final path = scenario['path'] as String;
      final body = readFixture(path);
      expect(body['scenario_id'], scenario['id']);
      expect(body['kind'], scenario['kind']);
    }
  });

  test('fixture vocabularies map to exported Flutter contract types', () {
    final manifest = readFixture('manifest.json');
    final scenarios = (manifest['scenarios'] as List).cast<Map<String, dynamic>>();

    for (final scenario in scenarios) {
      final body = readFixture(scenario['path'] as String);
      final profile = body['profile'];
      if (profile != null) {
        expect(profiles[profile], isNotNull, reason: 'unknown profile $profile');
      }

      final expectedEvents = (body['expected_events'] as List?)?.cast<String>() ?? const [];
      for (final event in expectedEvents) {
        expect(eventKinds[event], isNotNull, reason: 'unknown event $event');
      }

      final expectedError = body['expected_error'];
      if (expectedError != null) {
        expect(errorCodes[expectedError], isNotNull, reason: 'unknown error $expectedError');
      }

      final mappings = (body['mappings'] as List?)?.cast<Map<String, dynamic>>() ?? const [];
      for (final mapping in mappings) {
        expect(errorCodes[mapping['code']], isNotNull, reason: 'unknown code ${mapping['code']}');
        expect(
          errorCategories[mapping['category']],
          isNotNull,
          reason: 'unknown category ${mapping['category']}',
        );
      }
    }
  });

  test('profile defaults mirror the frozen sdk-app reference defaults', () {
    final mobile = Profile.mobileDefault.defaults();
    expect(mobile.retry.maxAttempts, 3);
    expect(mobile.retry.backoff.initialDelayMs, 250);
    expect(mobile.queuePressure.strategy, QueuePressureStrategy.retry);
    expect(mobile.queuePressure.maxAttempts, 3);
    expect(mobile.reconnect.enabled, isTrue);

    final desktop = Profile.desktopDefault.defaults();
    expect(desktop.retry.maxAttempts, 5);
    expect(desktop.retry.backoff.initialDelayMs, 200);
    expect(desktop.queuePressure.maxAttempts, 4);
    expect(desktop.reconnect.enabled, isTrue);

    final embedded = Profile.embeddedDefault.defaults();
    expect(embedded.retry.maxAttempts, 2);
    expect(embedded.queuePressure.strategy, QueuePressureStrategy.failFast);
    expect(embedded.reconnect.enabled, isFalse);

    final testing = Profile.testingDefault.defaults();
    expect(testing.retry.maxAttempts, 2);
    expect(testing.retry.backoff.initialDelayMs, 10);
    expect(testing.retry.backoff.multiplier, 1);
    expect(testing.queuePressure.strategy, QueuePressureStrategy.failFast);
    expect(testing.reconnect.enabled, isTrue);
    expect(testing.reconnect.maxAttempts, 2);
    expect(testing.reconnect.backoff.initialDelayMs, 25);
  });

  test('typed error mapping fixture stays aligned with app error envelope', () {
    final fixture = readFixture('errors.typed_mapping.json');
    final mappings = (fixture['mappings'] as List).cast<Map<String, dynamic>>();

    for (final mapping in mappings) {
      final code = errorCodes[mapping['code']]!;
      final category = errorCategories[mapping['category']]!;
      final error = AppError(
        code: code,
        category: category,
        message: 'fixture assertion',
        retryable: mapping['retryable'] as bool,
        terminal: mapping['terminal'] as bool,
        userActionRequired: mapping['user_action_required'] as bool,
      );

      expect(error.code.wireName, mapping['code']);
      expect(error.category, category);
      expect(error.retryable, mapping['retryable']);
      expect(error.terminal, mapping['terminal']);
      expect(error.userActionRequired, mapping['user_action_required']);
    }
  });

  test('queue pressure and compatibility fixtures remain visible at wrapper layer', () {
    final queuePressure = readFixture('delivery.queue_pressure.json');
    final queueEvents = (queuePressure['expected_events'] as List).cast<String>();
    expect(queueEvents, containsAll(<String>['QueuePressureRaised', 'RetryScheduled']));
    expect(queuePressure['expected_error'], ErrorCode.deliveryQueuePressure.wireName);
    expect(queuePressure['assertions']['partial_acceptance_visible'], isTrue);
    expect(queuePressure['assertions']['queue_pressure_hidden_in_ok'], isFalse);

    final compatibility = readFixture('compatibility.unknown_additive.json');
    final policy = compatibility['expected_policy'] as Map<String, dynamic>;
    expect(policy['ignore_unknown_capabilities'], isTrue);
    expect(policy['ignore_unknown_fields'], isTrue);
    expect(policy['preserve_known_fields'], isTrue);
    expect(policy['fail_only_on_required_by_profile'], isTrue);
  });
}
