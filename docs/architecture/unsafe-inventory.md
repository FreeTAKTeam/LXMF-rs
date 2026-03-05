# Unsafe Inventory

This table is the authoritative inventory for Rust `unsafe` usage in workspace code.

## Active Unsafe Entries

| Id | File | Line | Safety Invariant | Owner | Last Reviewed |
| --- | --- | --- | --- | --- | --- |
| EMBEDDED-FFI-001 | crates/libs/rns-embedded-ffi/src/lib.rs | 91 | Dereference caller `RnsEmbeddedNodeConfig*` only after null-check; treat it as immutable for the duration of construction. | @FreeTAKTeam | 2026-03-05 |
| EMBEDDED-FFI-002 | crates/libs/rns-embedded-ffi/src/lib.rs | 127 | Reclaim a node pointer only when it originated from `Box::into_raw` and is freed exactly once. | @FreeTAKTeam | 2026-03-05 |
| EMBEDDED-FFI-003 | crates/libs/rns-embedded-ffi/src/lib.rs | 190 | Write `out_len` only after non-null validation and only within caller-provided writable memory. | @FreeTAKTeam | 2026-03-05 |
| EMBEDDED-FFI-004 | crates/libs/rns-embedded-ffi/src/lib.rs | 201 | Copy outbound wire bytes only when `out_ptr` is valid and capacity is checked against frame length. | @FreeTAKTeam | 2026-03-05 |
| EMBEDDED-FFI-005 | crates/libs/rns-embedded-ffi/src/lib.rs | 224 | Read the 16-byte destination buffer only after null-check and only for the fixed ABI width. | @FreeTAKTeam | 2026-03-05 |
| EMBEDDED-FFI-006 | crates/libs/rns-embedded-ffi/src/lib.rs | 233 | Write `out_sequence` only after non-null validation and only with the queue result from the same call. | @FreeTAKTeam | 2026-03-05 |
| EMBEDDED-FFI-007 | crates/libs/rns-embedded-ffi/src/lib.rs | 248 | Convert the opaque node pointer into a mutable reference only for the call scope and only for handles allocated by this crate. | @FreeTAKTeam | 2026-03-05 |
| EMBEDDED-FFI-008 | crates/libs/rns-embedded-ffi/src/lib.rs | 260 | Convert caller byte pointers into slices only after null/length checks and never outlive the call. | @FreeTAKTeam | 2026-03-05 |

## Update Rules
1. Replace the `NONE` row with concrete entries before introducing any unsafe site.
2. Keep `File` and `Line` exact and current.
3. Keep a local `SAFETY:` comment adjacent to each unsafe site.
4. Remove rows immediately after deleting unsafe code.
