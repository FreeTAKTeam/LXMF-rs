# #611 utility/network evidence

Status: **partial / unverified**. This record covers the bounded native `rncp`
slice implemented at candidate commit `10bcd7a0` on top of the forward parity
branch. It does not close #611 or #605.

## Reference and ownership

- Forward reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`
  (`1.5.4-dev`), including `RNS/Utilities/rncp.py`.
- Rust owner: `crates/apps/rns-tools` using the existing
  `reticulum-rs-transport` Link/Resource and TCP interface APIs.
- No second daemon or utility protocol was introduced. The existing local copy
  mode remains available when no network flags are supplied.

## Implemented behavior matrix

| Workflow | Implementation-backed behavior | Evidence | Status |
| --- | --- | --- | --- |
| Local copy | Root-scoped binary copy, overwrite guard, parent traversal rejection | `rncp.rs` unit test | verified for the existing local convenience mode |
| Listener | TCP listener, deterministic/persisted identity, `rncp.receive` announce, periodic re-announce, save directory, collision-safe filename selection | `rncp_process` listener process | implemented; Python interoperability unverified |
| Send | TCP announce discovery, Link establishment, local identity identification, Resource send with `{"name": <binary filename>}` metadata, adaptive timeout and terminal failure status | `rncp_process` Rust client to Rust listener | verified for two independent processes |
| Fetch | `fetch_file` Link request/response, `True`/`False`/`0xF0`/`nil` status mapping, response Resource, metadata-driven save | `rncp_process` Rust client to Rust listener | verified for two independent processes |
| Authentication | `--no-auth`, explicit `--allowed-identity`, rejected identified peers, nonzero sender failure | manual denied-transfer run | verified locally for the negative path; Python allow-list parity unverified |
| Jail and save safety | Canonical jail containment, traversal rejection, basename-only metadata, overwrite/suffix behavior | protocol unit tests and process test | verified locally |
| Timeout/output | `--timeout`, silent mode, accurate failure output for missing/denied/failed transfers | unit/manual process runs | bounded Rust behavior verified |
| Compression option | `--no-compress` is parsed and reports that the shared Resource API still chooses compression automatically | CLI/process test | partial; transport option plumbing remains open |
| Other shipped utilities | `rnpath`, `rnprobe`, `rnsd`, `rnid`, `rnir`, `rnodeconf`, `rnpkg`, `rnsh`, `rnx`, and `rngit` | existing tests and callable inventory | not promoted by this slice; network/reference gaps remain |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
cargo fmt --all -- --check                         PASS
cargo test -p rns-tools --all-features             PASS
cargo clippy -p rns-tools --bin rncp --test rncp_process \
  --all-features --no-deps -- -D warnings           PASS
cargo test -p rns-tools --test rncp_process \
  --all-features -- --nocapture                     1 passed
```

The process test starts one listener and separate client processes with
isolated temporary roots. It sends an 8,192-byte binary payload, checks the
listener's saved bytes, fetches the same file back into a third root, and checks
the bytes again. The observed run completed in approximately 0.25 seconds.

An additional manual run transferred a 4,369-byte binary payload in both
directions. Both copies had SHA-256
`0ff31ee2a634edbb0f413f7c773eb2683775c006eb8d80da174acd4080ff94be`. A secure
listener without the sender in its allow-list returned exit status 1 and
`rncp: Resource transfer failed`; it did not report apparent success.

## Unresolved requirements

The following #611 acceptance items remain open and are deliberately not
classified as complete:

- Run the frozen Python client against the Rust listener and the Rust client
  against the frozen Python `rncp` listener, including Python's identity-file,
  allow-list, jail, overwrite, and callback behavior.
- Build the complete utility option/behavior matrix from every frozen
  `RNS/Utilities` entry point. The current slice does not add network workflows
  to `rnpath`, `rnprobe`, `rnsd`, or the radio/interactive utilities.
- Prove real `rngit` fetch/push/bundle workflows and configured initial-branch
  behavior under #601; the network/service implementation belongs to #612/#613.
- Add restart, interrupted-link, cancellation, slow-interface, disk-error, and
  multi-client transcripts with exact failure/status assertions.
- Plumb an explicit no-compression Resource option instead of accepting the
  flag while documenting the shared API limitation.

These are evidence or implementation gaps, not claims that the local Rust
process test represents Python interoperability or complete utility parity.
