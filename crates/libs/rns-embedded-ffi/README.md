# rns-embedded-ffi

`rns-embedded-ffi` exposes two C-facing surfaces:

- legacy compatibility entrypoints for manual tick plus raw wire ingress/egress
- the `v1` node-centric API for lifecycle, status, send/broadcast, and event subscriptions

The public header is [include/rns_embedded_ffi.h](/Users/tommy/Documents/TAK/LXMF-rs/crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h).

## v1 API Summary

Use the `v1` API when you want a stable node handle instead of wiring transport primitives manually:

- `rns_embedded_v1_node_new`
- `rns_embedded_v1_get_capabilities`
- `rns_embedded_v1_node_start`
- `rns_embedded_v1_node_stop`
- `rns_embedded_v1_node_restart`
- `rns_embedded_v1_node_get_status`
- `rns_embedded_v1_node_send`
- `rns_embedded_v1_node_broadcast`
- `rns_embedded_v1_node_set_log_level`
- `rns_embedded_v1_node_subscribe_events`
- `rns_embedded_v1_subscription_next`
- `rns_embedded_v1_subscription_close`

The `v1` structs are self-describing:

- every public struct starts with `struct_size`
- every public struct carries `struct_version`
- callers should zero-init or use the provided default-producing functions before passing structs in

## Minimal Flow

1. Create a node with `rns_embedded_v1_node_new()`.
2. Probe ABI/capabilities with `rns_embedded_v1_abi_version()` and `rns_embedded_v1_get_capabilities(...)`.
3. Fill `RnsEmbeddedV1NodeConfig` from `rns_embedded_v1_node_config_default()`.
4. Call `rns_embedded_v1_node_start(...)`.
5. Optionally create a subscription with `rns_embedded_v1_node_subscribe_events(...)`.
6. Use `rns_embedded_v1_node_send(...)` or `rns_embedded_v1_node_broadcast(...)`.
7. Read `RnsEmbeddedV1PollResult` + `RnsEmbeddedV1NodeEvent` with `rns_embedded_v1_subscription_next(...)`.
8. Close subscriptions, stop the node, then free the node handle.

## Example

```c
#include "rns_embedded_ffi.h"
#include <stdio.h>
#include <string.h>

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
        rns_embedded_v1_node_stop(node, &node_error);
        rns_embedded_v1_node_free(node);
        return 1;
    }

    uint8_t destination[16];
    memset(destination, 0x42, sizeof(destination));
    RnsEmbeddedV1SendReceipt receipt = {0};
    receipt.struct_size = sizeof(receipt);
    if (rns_embedded_v1_node_send(
            node,
            destination,
            (const uint8_t *)"hello",
            5,
            &receipt,
            &node_error) != RNS_EMBEDDED_STATUS_OK) {
        fprintf(stderr, "send failed: %u\n", (unsigned)node_error.code);
    }

    RnsEmbeddedV1PollResult poll = {0};
    poll.struct_size = sizeof(poll);
    RnsEmbeddedV1NodeEvent event = {0};
    event.struct_size = sizeof(event);
    if (rns_embedded_v1_subscription_next(subscription, 100, &poll, &event, &node_error) == RNS_EMBEDDED_STATUS_OK) {
        if (poll.kind == RNS_EMBEDDED_V1_POLL_EVENT) {
            printf("event kind=%u epoch=%llu\n",
                   (unsigned)event.kind,
                   (unsigned long long)event.epoch);
        }
    }

    rns_embedded_v1_subscription_close(subscription, &node_error);
    rns_embedded_v1_node_stop(node, &node_error);
    rns_embedded_v1_node_free(node);
    return 0;
}
```

## Legacy Compatibility Surface

The legacy API remains available for existing firmware and bridge code:

- `rns_embedded_node_new`
- `rns_embedded_node_tick`
- `rns_embedded_node_push_inbound_wire`
- `rns_embedded_node_take_outbound_wire`
- `rns_embedded_node_queue_message`

Use that surface when the integration owns transport progression explicitly and wants raw wire access.
