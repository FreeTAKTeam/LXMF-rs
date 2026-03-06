#ifndef RNS_EMBEDDED_FFI_H
#define RNS_EMBEDDED_FFI_H

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct RnsEmbeddedNode RnsEmbeddedNode;
typedef struct RnsEmbeddedV1Node RnsEmbeddedV1Node;
typedef struct RnsEmbeddedEventSubscription RnsEmbeddedEventSubscription;

typedef struct {
  uint8_t store_identity[32];
  uint8_t lxmf_address[16];
  uint32_t node_mode;
  uint64_t announce_interval_ms;
  size_t max_outbound_queue;
  size_t max_events;
  uint32_t capture_default_max_bytes;
  uint16_t ble_mtu_hint;
  size_t ble_max_inbound_frames;
  size_t ble_max_outbound_frames;
  bool ble_ordered_delivery;
} RnsEmbeddedNodeConfig;

typedef enum {
  RNS_EMBEDDED_NODE_MODE_BLE_ONLY = 0,
  RNS_EMBEDDED_NODE_MODE_TCP_CLIENT = 1,
  RNS_EMBEDDED_NODE_MODE_TCP_SERVER = 2,
} RnsEmbeddedNodeMode;

typedef enum {
  RNS_EMBEDDED_LINK_DOWN = 0,
  RNS_EMBEDDED_LINK_CONNECTING = 1,
  RNS_EMBEDDED_LINK_UP = 2,
} RnsEmbeddedLinkState;

typedef enum {
  RNS_EMBEDDED_LIFECYCLE_BOOT = 0,
  RNS_EMBEDDED_LIFECYCLE_UNPROVISIONED = 1,
  RNS_EMBEDDED_LIFECYCLE_PROVISIONED_OFFLINE = 2,
  RNS_EMBEDDED_LIFECYCLE_TCP_ONLINE = 3,
  RNS_EMBEDDED_LIFECYCLE_BLE_RECOVERY = 4,
  RNS_EMBEDDED_LIFECYCLE_FAILURE_RECONNECT = 5,
} RnsEmbeddedLifecycleState;

typedef enum {
  RNS_EMBEDDED_STATUS_OK = 0,
  RNS_EMBEDDED_STATUS_INVALID_INPUT = 1,
  RNS_EMBEDDED_STATUS_INVALID_ARGUMENT = 2,
  RNS_EMBEDDED_STATUS_INVALID_STATE = 3,
  RNS_EMBEDDED_STATUS_NOT_FOUND = 4,
  RNS_EMBEDDED_STATUS_SEQ_GAP = 5,
  RNS_EMBEDDED_STATUS_INTEGRITY_FAILURE = 6,
  RNS_EMBEDDED_STATUS_CHECKSUM_MISMATCH = 7,
  RNS_EMBEDDED_STATUS_IDEMPOTENCY_CONFLICT = 8,
  RNS_EMBEDDED_STATUS_REPLAY_REJECTED = 9,
  RNS_EMBEDDED_STATUS_TIMEOUT = 10,
  RNS_EMBEDDED_STATUS_BACKPRESSURE = 11,
  RNS_EMBEDDED_STATUS_DISCONNECTED = 12,
  RNS_EMBEDDED_STATUS_STORAGE_CORRUPTION = 13,
  RNS_EMBEDDED_STATUS_UNSUPPORTED = 14,
} RnsEmbeddedStatus;

typedef enum {
  RNS_EMBEDDED_V1_RUN_STATE_STOPPED = 0,
  RNS_EMBEDDED_V1_RUN_STATE_RUNNING = 1,
} RnsEmbeddedV1RunState;

typedef enum {
  RNS_EMBEDDED_V1_LOG_LEVEL_ERROR = 0,
  RNS_EMBEDDED_V1_LOG_LEVEL_WARN = 1,
  RNS_EMBEDDED_V1_LOG_LEVEL_INFO = 2,
  RNS_EMBEDDED_V1_LOG_LEVEL_DEBUG = 3,
  RNS_EMBEDDED_V1_LOG_LEVEL_TRACE = 4,
} RnsEmbeddedV1LogLevel;

typedef enum {
  RNS_EMBEDDED_V1_EVENT_STATUS_CHANGED = 0,
  RNS_EMBEDDED_V1_EVENT_LOG = 1,
  RNS_EMBEDDED_V1_EVENT_ERROR = 2,
  RNS_EMBEDDED_V1_EVENT_PACKET_RECEIVED = 3,
  RNS_EMBEDDED_V1_EVENT_PACKET_SENT = 4,
  RNS_EMBEDDED_V1_EVENT_EXTENSION = 5,
} RnsEmbeddedV1EventKind;

typedef enum {
  RNS_EMBEDDED_V1_POLL_EVENT = 0,
  RNS_EMBEDDED_V1_POLL_TIMEOUT = 1,
  RNS_EMBEDDED_V1_POLL_CLOSED = 2,
  RNS_EMBEDDED_V1_POLL_GAP = 3,
  RNS_EMBEDDED_V1_POLL_NODE_STOPPED = 4,
  RNS_EMBEDDED_V1_POLL_NODE_RESTARTED = 5,
} RnsEmbeddedV1PollResultKind;

typedef enum {
  RNS_EMBEDDED_V1_NODE_ERROR_UNKNOWN = 0,
  RNS_EMBEDDED_V1_NODE_ERROR_INVALID_CONFIG = 1,
  RNS_EMBEDDED_V1_NODE_ERROR_IO_ERROR = 2,
  RNS_EMBEDDED_V1_NODE_ERROR_NETWORK_ERROR = 3,
  RNS_EMBEDDED_V1_NODE_ERROR_RETICULUM_ERROR = 4,
  RNS_EMBEDDED_V1_NODE_ERROR_ALREADY_RUNNING = 5,
  RNS_EMBEDDED_V1_NODE_ERROR_NOT_RUNNING = 6,
  RNS_EMBEDDED_V1_NODE_ERROR_TIMEOUT = 7,
  RNS_EMBEDDED_V1_NODE_ERROR_INTERNAL_ERROR = 8,
  RNS_EMBEDDED_V1_NODE_ERROR_INVALID_HANDLE = 9,
  RNS_EMBEDDED_V1_NODE_ERROR_INVALID_POINTER = 10,
} RnsEmbeddedV1NodeErrorCode;

typedef struct {
  size_t struct_size;
  uint32_t struct_version;
  RnsEmbeddedV1NodeErrorCode code;
  uint8_t reserved[16];
} RnsEmbeddedV1NodeError;

typedef struct {
  size_t struct_size;
  uint32_t struct_version;
  uint8_t store_identity[32];
  uint8_t lxmf_address[16];
  uint32_t node_mode;
  uint64_t announce_interval_ms;
  size_t max_outbound_queue;
  size_t max_events;
  uint32_t capture_default_max_bytes;
  uint16_t ble_mtu_hint;
  size_t ble_max_inbound_frames;
  size_t ble_max_outbound_frames;
  bool ble_ordered_delivery;
  uint8_t reserved[32];
} RnsEmbeddedV1NodeConfig;

typedef struct {
  size_t struct_size;
  uint32_t struct_version;
  RnsEmbeddedV1RunState run_state;
  uint64_t epoch;
  RnsEmbeddedLifecycleState lifecycle_state;
  size_t pending_outbound;
  uint32_t announces_queued;
  uint32_t outbound_sent;
  uint32_t outbound_deferred;
  uint32_t inbound_accepted;
  uint32_t inbound_rejected;
  uint32_t announces_received;
  uint32_t lxmf_messages_received;
  RnsEmbeddedV1LogLevel log_level;
  uint8_t reserved[24];
} RnsEmbeddedV1NodeStatus;

typedef struct {
  size_t struct_size;
  uint32_t struct_version;
  uint64_t operation_id;
  uint64_t epoch;
  size_t accepted_bytes;
  bool queued;
  uint32_t target_count;
  uint8_t reserved[24];
} RnsEmbeddedV1SendReceipt;

typedef struct {
  size_t struct_size;
  uint32_t struct_version;
  uint32_t abi_version;
  uint64_t capability_bits;
  uint32_t max_event_payload_bytes;
  uint32_t max_subscriptions;
  uint8_t reserved[32];
} RnsEmbeddedV1Capabilities;

typedef struct {
  size_t struct_size;
  uint32_t struct_version;
  RnsEmbeddedV1EventKind kind;
  uint64_t event_id;
  uint64_t epoch;
  uint64_t occurred_at_ms;
  uint64_t operation_id;
  bool has_operation_id;
  RnsEmbeddedV1RunState run_state;
  RnsEmbeddedLifecycleState lifecycle_state;
  RnsEmbeddedV1LogLevel log_level;
  RnsEmbeddedV1NodeErrorCode error_code;
  uint8_t frame_kind;
  uint32_t sequence;
  size_t bytes;
  uint32_t extension_id;
  uint64_t value0;
  uint64_t value1;
  uint8_t reserved[24];
} RnsEmbeddedV1NodeEvent;

typedef struct {
  size_t struct_size;
  uint32_t struct_version;
  RnsEmbeddedV1PollResultKind kind;
  uint64_t next_event_id;
  uint64_t epoch;
  uint8_t reserved[24];
} RnsEmbeddedV1PollResult;

RnsEmbeddedNodeConfig rns_embedded_node_config_default(void);
RnsEmbeddedV1NodeConfig rns_embedded_v1_node_config_default(void);
uint32_t rns_embedded_v1_abi_version(void);
RnsEmbeddedStatus rns_embedded_v1_get_capabilities(
    RnsEmbeddedV1Capabilities *out_capabilities);
RnsEmbeddedV1Node *rns_embedded_v1_node_new(void);
void rns_embedded_v1_node_free(RnsEmbeddedV1Node *node);
RnsEmbeddedStatus rns_embedded_v1_node_start(
    RnsEmbeddedV1Node *node,
    const RnsEmbeddedV1NodeConfig *config,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_node_stop(
    RnsEmbeddedV1Node *node,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_node_restart(
    RnsEmbeddedV1Node *node,
    const RnsEmbeddedV1NodeConfig *config,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_node_get_status(
    RnsEmbeddedV1Node *node,
    RnsEmbeddedV1NodeStatus *out_status);
RnsEmbeddedStatus rns_embedded_v1_node_send(
    RnsEmbeddedV1Node *node,
    const uint8_t *destination_ptr,
    const uint8_t *body_ptr,
    size_t body_len,
    RnsEmbeddedV1SendReceipt *out_receipt,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_node_broadcast(
    RnsEmbeddedV1Node *node,
    const uint8_t *destinations_ptr,
    size_t destination_count,
    const uint8_t *body_ptr,
    size_t body_len,
    RnsEmbeddedV1SendReceipt *out_receipt,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_node_set_log_level(
    RnsEmbeddedV1Node *node,
    RnsEmbeddedV1LogLevel level,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_node_subscribe_events(
    RnsEmbeddedV1Node *node,
    RnsEmbeddedEventSubscription **out_subscription,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_subscription_next(
    RnsEmbeddedEventSubscription *subscription,
    uint64_t timeout_ms,
    RnsEmbeddedV1PollResult *out_poll_result,
    RnsEmbeddedV1NodeEvent *out_event,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedStatus rns_embedded_v1_subscription_close(
    RnsEmbeddedEventSubscription *subscription,
    RnsEmbeddedV1NodeError *out_node_error);
RnsEmbeddedNode *rns_embedded_node_new(const RnsEmbeddedNodeConfig *config);
void rns_embedded_node_free(RnsEmbeddedNode *node);
RnsEmbeddedStatus rns_embedded_node_set_link_state(
    RnsEmbeddedNode *node,
    RnsEmbeddedLinkState state);
RnsEmbeddedStatus rns_embedded_node_set_network_provisioned(
    RnsEmbeddedNode *node,
    bool provisioned);
RnsEmbeddedStatus rns_embedded_node_set_ble_recovery_active(
    RnsEmbeddedNode *node,
    bool active);
RnsEmbeddedLifecycleState rns_embedded_node_get_lifecycle_state(
    RnsEmbeddedNode *node);
RnsEmbeddedStatus rns_embedded_node_tick(RnsEmbeddedNode *node, uint64_t now_ms);
RnsEmbeddedStatus rns_embedded_node_push_inbound_wire(
    RnsEmbeddedNode *node,
    const uint8_t *bytes_ptr,
    size_t bytes_len);
RnsEmbeddedStatus rns_embedded_node_take_outbound_wire(
    RnsEmbeddedNode *node,
    uint8_t *out_ptr,
    size_t out_capacity,
    size_t *out_len);
RnsEmbeddedStatus rns_embedded_node_queue_message(
    RnsEmbeddedNode *node,
    const uint8_t *destination_ptr,
    const uint8_t *body_ptr,
    size_t body_len,
    uint32_t *out_sequence);

#ifdef __cplusplus
}
#endif

#endif
