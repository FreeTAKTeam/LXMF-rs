# Unsafe Inventory

This table is the authoritative inventory for Rust `unsafe` usage in workspace code.

## Active Unsafe Entries

| Id | File | Line | Safety Invariant | Owner | Last Reviewed |
| --- | --- | --- | --- | --- | --- |
| NONE | n/a | n/a | Workspace enforces `unsafe_code = "forbid"` and has no approved unsafe exceptions. | @FreeTAKTeam | 2026-02-21 |

## Update Rules
1. Replace the `NONE` row with concrete entries before introducing any unsafe site.
2. Keep `File` and `Line` exact and current.
3. Keep a local `SAFETY:` comment adjacent to each unsafe site.
4. Remove rows immediately after deleting unsafe code.
