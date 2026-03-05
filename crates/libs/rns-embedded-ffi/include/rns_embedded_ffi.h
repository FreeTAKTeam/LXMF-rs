#ifndef RNS_EMBEDDED_FFI_H
#define RNS_EMBEDDED_FFI_H

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct RnsEmbeddedNode RnsEmbeddedNode;

typedef struct {
  uint8_t store_identity[32];
  uint8_t lxmf_address[16];
  uint64_t announce_interval_ms;
  size_t max_outbound_queue;
  size_t max_events;
  uint16_t ble_mtu_hint;
  size_t ble_max_inbound_frames;
  size_t ble_max_outbound_frames;
  bool ble_ordered_delivery;
} RnsEmbeddedNodeConfig;

typedef enum {
  RNS_EMBEDDED_LINK_DOWN = 0,
  RNS_EMBEDDED_LINK_CONNECTING = 1,
  RNS_EMBEDDED_LINK_UP = 2,
} RnsEmbeddedLinkState;

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

RnsEmbeddedNodeConfig rns_embedded_node_config_default(void);
RnsEmbeddedNode *rns_embedded_node_new(const RnsEmbeddedNodeConfig *config);
void rns_embedded_node_free(RnsEmbeddedNode *node);
RnsEmbeddedStatus rns_embedded_node_set_link_state(
    RnsEmbeddedNode *node,
    RnsEmbeddedLinkState state);
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
