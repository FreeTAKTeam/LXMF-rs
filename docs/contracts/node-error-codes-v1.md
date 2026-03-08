# Embedded Node Error Codes v1

Generated from [node-error-codes-v1.json](./node-error-codes-v1.json).

Schema version: `1`

| Value | Rust Variant | C Constant | Default FFI Status | Description |
| --- | --- | --- | --- | --- |
| `0` | `Unknown` | `RNS_EMBEDDED_V1_NODE_ERROR_UNKNOWN` | `RNS_EMBEDDED_STATUS_INVALID_ARGUMENT` | Unspecified node-centric failure. |
| `1` | `InvalidConfig` | `RNS_EMBEDDED_V1_NODE_ERROR_INVALID_CONFIG` | `RNS_EMBEDDED_STATUS_INVALID_INPUT` | Caller supplied an invalid or unsupported node configuration. |
| `2` | `IoError` | `RNS_EMBEDDED_V1_NODE_ERROR_IO_ERROR` | `RNS_EMBEDDED_STATUS_INVALID_STATE` | The node hit an IO or backend state failure. |
| `3` | `NetworkError` | `RNS_EMBEDDED_V1_NODE_ERROR_NETWORK_ERROR` | `RNS_EMBEDDED_STATUS_DISCONNECTED` | The active backend or link is disconnected or unavailable. |
| `4` | `ReticulumError` | `RNS_EMBEDDED_V1_NODE_ERROR_RETICULUM_ERROR` | `RNS_EMBEDDED_STATUS_INVALID_STATE` | The underlying embedded protocol/runtime rejected the operation. |
| `5` | `AlreadyRunning` | `RNS_EMBEDDED_V1_NODE_ERROR_ALREADY_RUNNING` | `RNS_EMBEDDED_STATUS_INVALID_STATE` | The node is already running for the requested lifecycle transition. |
| `6` | `NotRunning` | `RNS_EMBEDDED_V1_NODE_ERROR_NOT_RUNNING` | `RNS_EMBEDDED_STATUS_INVALID_STATE` | The operation requires a running node. |
| `7` | `Timeout` | `RNS_EMBEDDED_V1_NODE_ERROR_TIMEOUT` | `RNS_EMBEDDED_STATUS_TIMEOUT` | The requested wait budget elapsed. |
| `8` | `InternalError` | `RNS_EMBEDDED_V1_NODE_ERROR_INTERNAL_ERROR` | `RNS_EMBEDDED_STATUS_INVALID_STATE` | The runtime or FFI bridge reached an internal invariant failure. |
| `9` | `InvalidHandle` | `RNS_EMBEDDED_V1_NODE_ERROR_INVALID_HANDLE` | `RNS_EMBEDDED_STATUS_INVALID_ARGUMENT` | The caller passed a null, stale, or mismatched opaque handle. |
| `10` | `InvalidPointer` | `RNS_EMBEDDED_V1_NODE_ERROR_INVALID_POINTER` | `RNS_EMBEDDED_STATUS_INVALID_ARGUMENT` | The caller passed an invalid pointer for an input or output buffer. |
| `11` | `ModeConflict` | `RNS_EMBEDDED_V1_NODE_ERROR_MODE_CONFLICT` | `RNS_EMBEDDED_STATUS_INVALID_STATE` | The caller mixed manual and managed node progression modes. |
| `12` | `SubscriptionClosed` | `RNS_EMBEDDED_V1_NODE_ERROR_SUBSCRIPTION_CLOSED` | `RNS_EMBEDDED_STATUS_OK` | A poll observed that the subscription handle was closed. |
| `13` | `NodeRestarted` | `RNS_EMBEDDED_V1_NODE_ERROR_NODE_RESTARTED` | `RNS_EMBEDDED_STATUS_OK` | A poll observed a generation change and the node restarted. |
| `14` | `EventGap` | `RNS_EMBEDDED_V1_NODE_ERROR_EVENT_GAP` | `RNS_EMBEDDED_STATUS_OK` | A poll observed an event-log gap and advanced to the next retained event. |
| `15` | `QueuePressure` | `RNS_EMBEDDED_V1_NODE_ERROR_QUEUE_PRESSURE` | `RNS_EMBEDDED_STATUS_BACKPRESSURE` | The node rejected the operation because queue capacity would be exceeded. |

Notes:

- `SUBSCRIPTION_CLOSED`, `NODE_RESTARTED`, and `EVENT_GAP` are sideband semantic codes surfaced alongside successful poll results.
- Wrappers must tolerate new codes added in future schema versions and treat unknown numeric values as opaque but non-fatal contract growth.
