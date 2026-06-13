# Unsafe Inventory

This table is the authoritative inventory for Rust `unsafe` usage in workspace code.

## Active Unsafe Entries

| Id | File | Line | Safety Invariant | Owner | Last Reviewed |
| --- | --- | --- | --- | --- | --- |
| EMBEDDED-FFI-001 | crates/libs/rns-embedded-ffi/src/lib_parts/module_prelude.rs | 53 | Install the no-op critical-section backend only for the single-threaded embedded shim where no interrupt masking state needs to be preserved. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-002 | crates/libs/rns-embedded-ffi/src/lib_parts/module_prelude.rs | 56 | `acquire` must remain a no-op that returns the unit restore token expected by the single-threaded critical-section shim. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-003 | crates/libs/rns-embedded-ffi/src/lib_parts/module_prelude.rs | 60 | `release` must stay paired with the no-op acquire path and never attempt to restore nonexistent machine state. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-004 | crates/libs/rns-embedded-ffi/src/lib_parts/rnsembeddedv1nodeerror.rs | 288 | Initialize the global allocator exactly once before allocation-heavy entrypoints and mutate the bootstrap globals only inside this guarded path. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-005 | crates/libs/rns-embedded-ffi/src/lib_parts/rnsembeddedv1nodeerror.rs | 324 | Write one `RnsEmbeddedV1Capabilities` value only to a caller pointer that was validated non-null immediately before the store. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-006 | crates/libs/rns-embedded-ffi/src/lib_parts/rnsembeddedv1nodeerror.rs | 346 | Reclaim a v1 node pointer only when it originated from `Box::into_raw` in `rns_embedded_v1_node_new` and is freed exactly once. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-007 | crates/libs/rns-embedded-ffi/src/lib_parts/rnsembeddedv1nodeerror.rs | 361 | Dereference caller `RnsEmbeddedNodeConfig*` only after null-check and treat it as immutable for the duration of node construction. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-008 | crates/libs/rns-embedded-ffi/src/lib_parts/rnsembeddedv1nodeerror.rs | 399 | Reclaim a node pointer only when it originated from `Box::into_raw` in `rns_embedded_node_new` and is freed exactly once. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-009 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 49 | Write `out_len = 0` only after validating the output pointer and only for the no-frame path of the same call. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-010 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 56 | Write the required frame length back to `out_len` only after validating the pointer and only for the capacity-miss path of the same call. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-011 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 64 | Copy outbound frame bytes and update `out_len` only when both output pointers are valid and `frame.len() <= out_capacity`. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-012 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 87 | Read the destination buffer only after null-check and only for the fixed 16-byte ABI width. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-013 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 96 | Write `out_sequence` only after non-null validation and only with the queue result produced by the same call. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-014 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 123 | Dereference caller `RnsEmbeddedV1NodeConfig*` only after null-check and treat it as immutable while building the start configuration. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-015 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 169 | Dereference caller `RnsEmbeddedV1NodeConfig*` only after null-check and treat it as immutable while building the restart configuration. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-016 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 196 | Write one mapped node status only to a caller pointer that was validated non-null immediately before the store. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-017 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 230 | Read the send destination buffer only after null-check and only for the fixed 16-byte ABI width. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-018 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 237 | Write one mapped send receipt only to a caller pointer that was validated non-null immediately before the store. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-019 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 286 | Write one mapped broadcast receipt only to a caller pointer that was validated non-null immediately before the store. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-020 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 333 | Publish the subscription handle only to validated caller storage and only for a handle allocated by `Box::into_raw` in this call. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-021 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 366 | Write poll-result sideband outputs only when both output pointers were validated non-null for the current subscription poll. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-022 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 391 | Reclaim a subscription pointer only when it originated from `Box::into_raw` in `rns_embedded_v1_node_subscribe_events` and is freed exactly once. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-023 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 405 | Convert an opaque node handle into a mutable reference only for the current FFI call and only for handles allocated by this crate. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-024 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 414 | Convert an opaque v1 node handle into a mutable reference only for the current FFI call and only for handles allocated by this crate. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-025 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 425 | Convert an opaque subscription handle into a mutable reference only for the current FFI call and only for handles allocated by this crate. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-026 | crates/libs/rns-embedded-ffi/src/lib_parts/rns_embedded_node_get_lifecycle_stat.rs | 437 | Convert caller byte pointers into slices only after null/length checks and never let the slice outlive the call. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-027 | crates/libs/rns-embedded-ffi/src/lib_parts/destination_list.rs | 10 | Convert the packed destination list pointer into a slice only after null-check and only for `count * 16` readable bytes. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-028 | crates/libs/rns-embedded-ffi/src/lib_parts/destination_list.rs | 273 | Clear the sideband node-error struct only when the output pointer is non-null and points to writable caller storage. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-029 | crates/libs/rns-embedded-ffi/src/lib_parts/destination_list.rs | 295 | Write the poll sideband node-error struct only when the output pointer is non-null and points to writable caller storage. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-030 | crates/libs/rns-embedded-ffi/src/lib_parts/destination_list.rs | 309 | Write the pointer-validation node-error struct only when the output pointer is non-null and points to writable caller storage. | @FreeTAKTeam | 2026-03-31 |
| EMBEDDED-FFI-031 | crates/libs/rns-embedded-ffi/src/lib_parts/destination_list.rs | 323 | Write the mapped node-error struct only when the output pointer is non-null and points to writable caller storage. | @FreeTAKTeam | 2026-03-31 |

## Update Rules
1. Replace the `NONE` row with concrete entries before introducing any unsafe site.
2. Keep `File` and `Line` exact and current.
3. Keep a local `SAFETY:` comment adjacent to each unsafe site.
4. Remove rows immediately after deleting unsafe code.
