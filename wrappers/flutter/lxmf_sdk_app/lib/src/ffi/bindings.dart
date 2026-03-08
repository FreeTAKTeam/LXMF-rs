import 'dart:ffi';

final class RnsEmbeddedV1Node extends Opaque {}

final class RnsEmbeddedEventSubscription extends Opaque {}

const int rnsEmbeddedStatusOk = 0;
const int rnsEmbeddedStatusInvalidInput = 1;
const int rnsEmbeddedStatusInvalidArgument = 2;
const int rnsEmbeddedStatusInvalidState = 3;
const int rnsEmbeddedStatusNotFound = 4;
const int rnsEmbeddedStatusSeqGap = 5;
const int rnsEmbeddedStatusIntegrityFailure = 6;
const int rnsEmbeddedStatusChecksumMismatch = 7;
const int rnsEmbeddedStatusIdempotencyConflict = 8;
const int rnsEmbeddedStatusReplayRejected = 9;
const int rnsEmbeddedStatusTimeout = 10;
const int rnsEmbeddedStatusBackpressure = 11;
const int rnsEmbeddedStatusDisconnected = 12;
const int rnsEmbeddedStatusStorageCorruption = 13;
const int rnsEmbeddedStatusUnsupported = 14;

const int rnsEmbeddedV1RunStateStopped = 0;
const int rnsEmbeddedV1RunStateRunning = 1;

const int rnsEmbeddedV1EventStatusChanged = 0;
const int rnsEmbeddedV1EventLog = 1;
const int rnsEmbeddedV1EventError = 2;
const int rnsEmbeddedV1EventPacketReceived = 3;
const int rnsEmbeddedV1EventPacketSent = 4;
const int rnsEmbeddedV1EventExtension = 5;

const int rnsEmbeddedV1PollEvent = 0;
const int rnsEmbeddedV1PollTimeout = 1;
const int rnsEmbeddedV1PollClosed = 2;
const int rnsEmbeddedV1PollGap = 3;
const int rnsEmbeddedV1PollNodeStopped = 4;
const int rnsEmbeddedV1PollNodeRestarted = 5;

const int rnsEmbeddedNodeModeBleOnly = 0;
const int rnsEmbeddedNodeModeTcpClient = 1;
const int rnsEmbeddedNodeModeTcpServer = 2;

base class RnsEmbeddedV1NodeError extends Struct {
  @IntPtr()
  external int structSize;

  @Uint32()
  external int structVersion;

  @Uint32()
  external int code;

  @Array(16)
  external Array<Uint8> reserved;
}

base class RnsEmbeddedV1NodeConfig extends Struct {
  @IntPtr()
  external int structSize;

  @Uint32()
  external int structVersion;

  @Array(32)
  external Array<Uint8> storeIdentity;

  @Array(16)
  external Array<Uint8> lxmfAddress;

  @Uint32()
  external int nodeMode;

  @Uint64()
  external int announceIntervalMs;

  @IntPtr()
  external int maxOutboundQueue;

  @IntPtr()
  external int maxEvents;

  @Uint32()
  external int captureDefaultMaxBytes;

  @Uint16()
  external int bleMtuHint;

  @IntPtr()
  external int bleMaxInboundFrames;

  @IntPtr()
  external int bleMaxOutboundFrames;

  @Bool()
  external bool bleOrderedDelivery;

  @Array(256)
  external Array<Uint8> tcpHost;

  @Uint16()
  external int tcpPort;

  @Uint16()
  external int tcpListenPort;

  @Array(28)
  external Array<Uint8> reserved;
}

base class RnsEmbeddedV1NodeStatus extends Struct {
  @IntPtr()
  external int structSize;

  @Uint32()
  external int structVersion;

  @Uint32()
  external int runState;

  @Uint64()
  external int epoch;

  @Uint32()
  external int lifecycleState;

  @IntPtr()
  external int pendingOutbound;

  @Uint32()
  external int announcesQueued;

  @Uint32()
  external int outboundSent;

  @Uint32()
  external int outboundDeferred;

  @Uint32()
  external int inboundAccepted;

  @Uint32()
  external int inboundRejected;

  @Uint32()
  external int announcesReceived;

  @Uint32()
  external int lxmfMessagesReceived;

  @Uint32()
  external int logLevel;

  @Array(24)
  external Array<Uint8> reserved;
}

base class RnsEmbeddedV1SendReceipt extends Struct {
  @IntPtr()
  external int structSize;

  @Uint32()
  external int structVersion;

  @Uint64()
  external int operationId;

  @Uint64()
  external int epoch;

  @IntPtr()
  external int acceptedBytes;

  @Bool()
  external bool queued;

  @Uint32()
  external int targetCount;

  @Array(24)
  external Array<Uint8> reserved;
}

base class RnsEmbeddedV1Capabilities extends Struct {
  @IntPtr()
  external int structSize;

  @Uint32()
  external int structVersion;

  @Uint32()
  external int abiVersion;

  @Uint32()
  external int capabilitySchemaVersion;

  @Uint64()
  external int knownCapabilityBits;

  @Uint64()
  external int compileTimeCapabilityBits;

  @Uint64()
  external int capabilityBits;

  @Uint32()
  external int maxEventPayloadBytes;

  @Uint32()
  external int maxSubscriptions;

  @Uint64()
  external int maxBlockingTimeoutMs;

  @Uint32()
  external int driverTickTargetMs;

  @Uint32()
  external int driverTickMaxMs;

  @Array(24)
  external Array<Uint8> reserved;
}

base class RnsEmbeddedV1NodeEvent extends Struct {
  @IntPtr()
  external int structSize;

  @Uint32()
  external int structVersion;

  @Uint32()
  external int kind;

  @Uint64()
  external int eventId;

  @Uint64()
  external int epoch;

  @Uint64()
  external int occurredAtMs;

  @Uint64()
  external int operationId;

  @Bool()
  external bool hasOperationId;

  @Uint32()
  external int runState;

  @Uint32()
  external int lifecycleState;

  @Uint32()
  external int logLevel;

  @Uint32()
  external int errorCode;

  @Uint8()
  external int frameKind;

  @Uint32()
  external int sequence;

  @IntPtr()
  external int bytes;

  @Uint32()
  external int extensionId;

  @Uint64()
  external int value0;

  @Uint64()
  external int value1;

  @Array(24)
  external Array<Uint8> reserved;
}

base class RnsEmbeddedV1PollResult extends Struct {
  @IntPtr()
  external int structSize;

  @Uint32()
  external int structVersion;

  @Uint32()
  external int kind;

  @Uint64()
  external int nextEventId;

  @Uint64()
  external int epoch;

  @Array(24)
  external Array<Uint8> reserved;
}

typedef _NodeConfigDefaultNative = RnsEmbeddedV1NodeConfig Function();
typedef _NodeConfigDefaultDart = RnsEmbeddedV1NodeConfig Function();
typedef _GetCapabilitiesNative = Int32 Function(
    Pointer<RnsEmbeddedV1Capabilities>);
typedef _GetCapabilitiesDart = int Function(Pointer<RnsEmbeddedV1Capabilities>);
typedef _NodeNewNative = Pointer<RnsEmbeddedV1Node> Function();
typedef _NodeNewDart = Pointer<RnsEmbeddedV1Node> Function();
typedef _NodeFreeNative = Void Function(Pointer<RnsEmbeddedV1Node>);
typedef _NodeFreeDart = void Function(Pointer<RnsEmbeddedV1Node>);
typedef _NodeStartNative = Int32 Function(
  Pointer<RnsEmbeddedV1Node>,
  Pointer<RnsEmbeddedV1NodeConfig>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _NodeStartDart = int Function(
  Pointer<RnsEmbeddedV1Node>,
  Pointer<RnsEmbeddedV1NodeConfig>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _NodeStopNative = Int32 Function(
    Pointer<RnsEmbeddedV1Node>, Pointer<RnsEmbeddedV1NodeError>);
typedef _NodeStopDart = int Function(
    Pointer<RnsEmbeddedV1Node>, Pointer<RnsEmbeddedV1NodeError>);
typedef _NodeGetStatusNative = Int32 Function(
    Pointer<RnsEmbeddedV1Node>, Pointer<RnsEmbeddedV1NodeStatus>);
typedef _NodeGetStatusDart = int Function(
    Pointer<RnsEmbeddedV1Node>, Pointer<RnsEmbeddedV1NodeStatus>);
typedef _NodeSendNative = Int32 Function(
  Pointer<RnsEmbeddedV1Node>,
  Pointer<Uint8>,
  Pointer<Uint8>,
  IntPtr,
  Pointer<RnsEmbeddedV1SendReceipt>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _NodeSendDart = int Function(
  Pointer<RnsEmbeddedV1Node>,
  Pointer<Uint8>,
  Pointer<Uint8>,
  int,
  Pointer<RnsEmbeddedV1SendReceipt>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _NodeSubscribeNative = Int32 Function(
  Pointer<RnsEmbeddedV1Node>,
  Pointer<Pointer<RnsEmbeddedEventSubscription>>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _NodeSubscribeDart = int Function(
  Pointer<RnsEmbeddedV1Node>,
  Pointer<Pointer<RnsEmbeddedEventSubscription>>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _SubscriptionNextNative = Int32 Function(
  Pointer<RnsEmbeddedEventSubscription>,
  Uint64,
  Pointer<RnsEmbeddedV1PollResult>,
  Pointer<RnsEmbeddedV1NodeEvent>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _SubscriptionNextDart = int Function(
  Pointer<RnsEmbeddedEventSubscription>,
  int,
  Pointer<RnsEmbeddedV1PollResult>,
  Pointer<RnsEmbeddedV1NodeEvent>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _SubscriptionCloseNative = Int32 Function(
  Pointer<RnsEmbeddedEventSubscription>,
  Pointer<RnsEmbeddedV1NodeError>,
);
typedef _SubscriptionCloseDart = int Function(
  Pointer<RnsEmbeddedEventSubscription>,
  Pointer<RnsEmbeddedV1NodeError>,
);

class EmbeddedFfiApi {
  EmbeddedFfiApi(this.library)
      : _nodeConfigDefault = library.lookupFunction<_NodeConfigDefaultNative,
            _NodeConfigDefaultDart>('rns_embedded_v1_node_config_default'),
        _getCapabilities = library.lookupFunction<_GetCapabilitiesNative,
            _GetCapabilitiesDart>('rns_embedded_v1_get_capabilities'),
        _nodeNew = library.lookupFunction<_NodeNewNative, _NodeNewDart>(
          'rns_embedded_v1_node_new',
        ),
        _nodeFree = library.lookupFunction<_NodeFreeNative, _NodeFreeDart>(
          'rns_embedded_v1_node_free',
        ),
        _nodeStart = library.lookupFunction<_NodeStartNative, _NodeStartDart>(
          'rns_embedded_v1_node_start',
        ),
        _nodeStop = library.lookupFunction<_NodeStopNative, _NodeStopDart>(
          'rns_embedded_v1_node_stop',
        ),
        _nodeGetStatus =
            library.lookupFunction<_NodeGetStatusNative, _NodeGetStatusDart>(
                'rns_embedded_v1_node_get_status'),
        _nodeSend = library.lookupFunction<_NodeSendNative, _NodeSendDart>(
          'rns_embedded_v1_node_send',
        ),
        _nodeSubscribe =
            library.lookupFunction<_NodeSubscribeNative, _NodeSubscribeDart>(
                'rns_embedded_v1_node_subscribe_events'),
        _subscriptionNext = library.lookupFunction<_SubscriptionNextNative,
            _SubscriptionNextDart>('rns_embedded_v1_subscription_next'),
        _subscriptionClose = library.lookupFunction<_SubscriptionCloseNative,
            _SubscriptionCloseDart>('rns_embedded_v1_subscription_close');

  final DynamicLibrary library;
  final _NodeConfigDefaultDart _nodeConfigDefault;
  final _GetCapabilitiesDart _getCapabilities;
  final _NodeNewDart _nodeNew;
  final _NodeFreeDart _nodeFree;
  final _NodeStartDart _nodeStart;
  final _NodeStopDart _nodeStop;
  final _NodeGetStatusDart _nodeGetStatus;
  final _NodeSendDart _nodeSend;
  final _NodeSubscribeDart _nodeSubscribe;
  final _SubscriptionNextDart _subscriptionNext;
  final _SubscriptionCloseDart _subscriptionClose;

  RnsEmbeddedV1NodeConfig nodeConfigDefault() => _nodeConfigDefault();

  int getCapabilities(Pointer<RnsEmbeddedV1Capabilities> outCapabilities) =>
      _getCapabilities(outCapabilities);

  Pointer<RnsEmbeddedV1Node> nodeNew() => _nodeNew();

  void nodeFree(Pointer<RnsEmbeddedV1Node> node) => _nodeFree(node);

  int nodeStart(
    Pointer<RnsEmbeddedV1Node> node,
    Pointer<RnsEmbeddedV1NodeConfig> config,
    Pointer<RnsEmbeddedV1NodeError> outNodeError,
  ) =>
      _nodeStart(node, config, outNodeError);

  int nodeStop(
    Pointer<RnsEmbeddedV1Node> node,
    Pointer<RnsEmbeddedV1NodeError> outNodeError,
  ) =>
      _nodeStop(node, outNodeError);

  int nodeGetStatus(
    Pointer<RnsEmbeddedV1Node> node,
    Pointer<RnsEmbeddedV1NodeStatus> outStatus,
  ) =>
      _nodeGetStatus(node, outStatus);

  int nodeSend(
    Pointer<RnsEmbeddedV1Node> node,
    Pointer<Uint8> destination,
    Pointer<Uint8> body,
    int bodyLength,
    Pointer<RnsEmbeddedV1SendReceipt> outReceipt,
    Pointer<RnsEmbeddedV1NodeError> outNodeError,
  ) =>
      _nodeSend(node, destination, body, bodyLength, outReceipt, outNodeError);

  int nodeSubscribeEvents(
    Pointer<RnsEmbeddedV1Node> node,
    Pointer<Pointer<RnsEmbeddedEventSubscription>> outSubscription,
    Pointer<RnsEmbeddedV1NodeError> outNodeError,
  ) =>
      _nodeSubscribe(node, outSubscription, outNodeError);

  int subscriptionNext(
    Pointer<RnsEmbeddedEventSubscription> subscription,
    int timeoutMs,
    Pointer<RnsEmbeddedV1PollResult> outPollResult,
    Pointer<RnsEmbeddedV1NodeEvent> outEvent,
    Pointer<RnsEmbeddedV1NodeError> outNodeError,
  ) =>
      _subscriptionNext(
        subscription,
        timeoutMs,
        outPollResult,
        outEvent,
        outNodeError,
      );

  int subscriptionClose(
    Pointer<RnsEmbeddedEventSubscription> subscription,
    Pointer<RnsEmbeddedV1NodeError> outNodeError,
  ) =>
      _subscriptionClose(subscription, outNodeError);
}
