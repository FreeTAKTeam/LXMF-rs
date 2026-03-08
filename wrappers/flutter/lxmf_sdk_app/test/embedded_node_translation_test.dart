import 'dart:ffi';

import 'package:ffi/ffi.dart';
import 'package:lxmf_sdk_app/lxmf_sdk_app.dart';
import 'package:lxmf_sdk_app/src/ffi/bindings.dart';
import 'package:test/test.dart';

void main() {
  test('run state mapping matches sdk-app contract states', () {
    expect(
      EmbeddedNodeTranslation.mapRunState(rnsEmbeddedV1RunStateRunning),
      RunState.running,
    );
    expect(
      EmbeddedNodeTranslation.mapRunState(rnsEmbeddedV1RunStateStopped),
      RunState.stopped,
    );
    expect(EmbeddedNodeTranslation.mapRunState(99), RunState.failed);
  });

  test('ffi status and node error codes map to typed app errors', () {
    final queuePressure = EmbeddedNodeTranslation.statusError(
      rnsEmbeddedStatusBackpressure,
      nodeErrorCode: 15,
    );
    expect(queuePressure.code, ErrorCode.deliveryQueuePressure);
    expect(queuePressure.category, ErrorCategory.delivery);
    expect(queuePressure.retryable, isTrue);
    expect(queuePressure.terminal, isFalse);
    expect(queuePressure.causeCode, 'RNS_EMBEDDED_V1_NODE_ERROR_15');

    final timeout = EmbeddedNodeTranslation.statusError(
      rnsEmbeddedStatusTimeout,
      nodeErrorCode: 7,
    );
    expect(timeout.code, ErrorCode.timeoutOperationExpired);
    expect(timeout.category, ErrorCategory.timeout);
    expect(timeout.retryable, isFalse);
    expect(timeout.terminal, isTrue);
  });

  test('native packet sent event maps into typed app event', () {
    final eventPtr = calloc<RnsEmbeddedV1NodeEvent>();
    try {
      eventPtr.ref
        ..structSize = sizeOf<RnsEmbeddedV1NodeEvent>()
        ..structVersion = 1
        ..kind = rnsEmbeddedV1EventPacketSent
        ..eventId = 41
        ..epoch = 7
        ..occurredAtMs = 123456
        ..operationId = 88
        ..hasOperationId = true
        ..logLevel = 2
        ..sequence = 3
        ..bytes = 144;

      final mapped = EmbeddedNodeTranslation.mapEvent(
        eventPtr.ref,
        profile: Profile.testingDefault,
      );

      expect(mapped.kind, EventKind.messageSent);
      expect(mapped.metadata.eventId, '41');
      expect(mapped.metadata.runtimeId, 'embedded-node-7');
      expect(mapped.metadata.seqNo, 41);
      expect(mapped.metadata.occurredAtMs, 123456);
      expect(mapped.metadata.operationId, '88');
      expect(mapped.metadata.profileId, Profile.testingDefault.id);
      expect(mapped.metadata.severity, Severity.info);
      expect(mapped.rawEventType, 'native_v1_4');

      final details = mapped.details! as Map<String, Object?>;
      expect(details['sequence'], 3);
      expect(details['bytes'], 144);
    } finally {
      calloc.free(eventPtr);
    }
  });

  test('synthetic gap event is visible and recovery-oriented', () {
    final event = EmbeddedNodeTranslation.syntheticEvent(
      kind: EventKind.streamGapDetected,
      rawEventType: 'poll_gap',
      epoch: 9,
      eventId: 101,
      profile: Profile.mobileDefault,
      streamGap: const StreamGapDetails(
        expectedSeqNo: 100,
        observedSeqNo: 101,
        droppedCount: 1,
        recoveryRequired: true,
      ),
    );

    expect(event.kind, EventKind.streamGapDetected);
    expect(event.metadata.runtimeId, 'embedded-node-9');
    expect(event.metadata.seqNo, 101);
    expect(event.metadata.profileId, Profile.mobileDefault.id);
    expect(event.streamGap, isNotNull);
    expect(event.streamGap!.droppedCount, 1);
    expect(event.streamGap!.recoveryRequired, isTrue);
  });
}
