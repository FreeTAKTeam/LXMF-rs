#include "rns_embedded_ffi.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

static void fill_destination(uint8_t out[16], uint8_t value) {
  memset(out, value, 16);
}

int main(void) {
  RnsEmbeddedV1Node *node = rns_embedded_v1_node_new();
  if (!node) {
    return 1;
  }

  RnsEmbeddedV1Capabilities capabilities = {0};
  capabilities.struct_size = sizeof(capabilities);
  if (rns_embedded_v1_get_capabilities(&capabilities) != RNS_EMBEDDED_STATUS_OK) {
    rns_embedded_v1_node_free(node);
    return 1;
  }

  if ((capabilities.capability_bits & RNS_EMBEDDED_V1_CAP_BLOCKING_NEXT) == 0) {
    fprintf(stderr, "blocking next() unsupported in this build\n");
  }

  RnsEmbeddedV1NodeConfig config = rns_embedded_v1_node_config_default();
  RnsEmbeddedV1NodeError node_error = {0};
  node_error.struct_size = sizeof(node_error);
  if (rns_embedded_v1_node_start(node, &config, &node_error) != RNS_EMBEDDED_STATUS_OK) {
    fprintf(stderr, "start failed: %u\n", (unsigned)node_error.code);
    rns_embedded_v1_node_free(node);
    return 1;
  }

  RnsEmbeddedEventSubscription *subscription = NULL;
  if (rns_embedded_v1_node_subscribe_events(node, &subscription, &node_error) != RNS_EMBEDDED_STATUS_OK) {
    fprintf(stderr, "subscribe failed: %u\n", (unsigned)node_error.code);
    rns_embedded_v1_node_stop(node, &node_error);
    rns_embedded_v1_node_free(node);
    return 1;
  }

  uint8_t destination[16];
  fill_destination(destination, 0x42);
  RnsEmbeddedV1SendReceipt receipt = {0};
  receipt.struct_size = sizeof(receipt);
  if (rns_embedded_v1_node_send(
          node,
          destination,
          (const uint8_t *)"mobile-wrapper-send",
          strlen("mobile-wrapper-send"),
          &receipt,
          &node_error) != RNS_EMBEDDED_STATUS_OK) {
    fprintf(stderr, "send failed: %u\n", (unsigned)node_error.code);
  }

  RnsEmbeddedV1PollResult poll = {0};
  poll.struct_size = sizeof(poll);
  RnsEmbeddedV1NodeEvent event = {0};
  event.struct_size = sizeof(event);
  if (rns_embedded_v1_subscription_next(subscription, 100, &poll, &event, &node_error) == RNS_EMBEDDED_STATUS_OK) {
    printf("poll kind=%u sideband_error=%u epoch=%llu\n",
           (unsigned)poll.kind,
           (unsigned)node_error.code,
           (unsigned long long)poll.epoch);
  }

  rns_embedded_v1_subscription_close(subscription, &node_error);
  rns_embedded_v1_node_stop(node, &node_error);
  rns_embedded_v1_node_free(node);
  return 0;
}
