# #611 utility/network evidence

Status: **partial / unverified**. This record covers the bounded native `rncp`
slice on the forward parity branch, including authenticated pinned-Python and
Rust sender/listener roles. The mixed-runtime compression increment is
implemented by `3c6757ba`, and process-level missing-file/denied-identity
failure assertions are implemented by `2b281b87`; these increments do not
close #611 or #605. Malformed identity and unusable-save-path failures are
covered by `053ef246`, and path-discovery timeout status is covered by
`9dc9bd62`. Listener identity persistence and a post-restart transfer are
covered by `d66b19d1`; local destination disk-error status is covered by
`9b8e4ed6`.

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
| Listener | TCP listener, deterministic/persisted identity, `rncp.receive` announce, periodic re-announce, save directory, collision-safe filename selection | `rncp_process` listener process; pinned Python client/service trace | implemented; no-auth Python interoperability evidenced |
| Send | TCP announce discovery, Link establishment, local identity identification, Resource send with `{"name": <binary filename>}` metadata, adaptive timeout and terminal failure status | Rust↔Rust process test; pinned Python client/service trace | verified for two independent processes and the bounded Python roles |
| Fetch | `fetch_file` Link request/response, `True`/`False`/`0xF0`/`nil` status mapping, the pinned Python rncp listener's ordinary metadata-bearing file Resource contract, correlated Rust response Resources for other callers, and metadata-driven save | `rncp_process` Rust client to Rust listener; pinned Python listener/client trace; pinned Python client fetching from Rust | verified for two independent Rust processes and the bounded Python↔Rust fetch paths |
| Authentication | `--no-auth`, explicit `--allowed-identity`, rejected identified peers, nonzero sender failure, and reciprocal Python/Rust identity allow-lists for send and fetch roles | manual denied-transfer run; pinned Python interop | verified for the bounded send/fetch roles; broader option and callback parity remains open |
| Jail and save safety | Canonical jail containment, traversal rejection, basename-only metadata, overwrite/suffix behavior | protocol unit tests and process test | verified locally |
| Timeout/output | `--timeout`, silent mode, nonzero status for denied senders, preserved not-found failure output for missing fetches, malformed identity rejection, unusable save-path rejection, and path-discovery timeout status | unit/manual process runs, `rncp_process` | bounded Rust behavior and these five process-level failure categories verified; interruption/cancellation coverage remains open |
| Restart | Persisted listener identity, same TCP endpoint, stable destination hash, and a second binary transfer after listener restart | `rncp_process` | verified for the bounded Rust listener/client path |
| Disk failure | Fetch save failure when the overwrite target is a directory | `rncp_process` | verified with nonzero status and preserved OS error output; remote receive-side save faults remain open |
| Compression option | `--no-compress` disables opportunistic Resource compression for outbound sends and fetch responses while preserving the default auto-compression path | Resource compression regression, `rncp_process`, mixed Python/Rust compression matrix | verified for the focused Python↔Rust send and listener-side fetch-response roles; direct callback telemetry and the complete utility matrix remain open |
| Other shipped utilities | `rnpath`, `rnprobe`, `rnsd`, `rnid`, `rnir`, `rnodeconf`, `rnpkg`, `rnsh`, `rnx`, and `rngit` | existing tests and callable inventory | not promoted by this slice; network/reference gaps remain |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
cargo fmt --all -- --check                         PASS
cargo test -p rns-tools --all-features             PASS
cargo clippy -p rns-tools --bin rncp --test rncp_process \
  --all-features --no-deps -- -D warnings           PASS
cargo clippy -p rns-tools --test rncp_python_interop \
  --all-features --no-deps -- -D warnings           PASS
cargo test -p rns-tools --test rncp_process \
  --all-features -- --nocapture                     6 passed (1.04s)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  rncp_mixed_runtime_compression_matrix_roundtrips_binary_files \
  -- --ignored --nocapture                         1 passed (4.78s)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  -- --ignored --nocapture                         2 passed (32.30s)
```

The process tests start independent listener and client processes with isolated
temporary roots. The success path sends an 8,192-byte binary payload, checks
the listener's saved bytes, fetches the same file back into a third root, and
checks the bytes again. It then fetches a missing file and asserts nonzero
status plus the `remote file was not found` category. A separate secure
listener rejects an unidentified sender and the client asserts nonzero status
plus `Resource transfer failed`; the listener does not save the payload. The
same test binary also rejects a malformed allowed identity and a file supplied
as the save path before network work begins. Its timeout case targets an unused
TCP endpoint, asserts nonzero status, and preserves `path discovery timed out`.
The same process binary persists a listener identity, restarts the listener on
the same TCP endpoint, verifies the destination hash is stable, and completes a
second binary transfer. The observed run completed in approximately 1.04
seconds. The same run attempts a fetch with an overwrite target that is a
directory and asserts nonzero status plus the `Is a directory` OS error.

An additional manual run transferred a 4,369-byte binary payload in both
directions. Both copies had SHA-256
`0ff31ee2a634edbb0f413f7c773eb2683775c006eb8d80da174acd4080ff94be`. A secure
listener without the sender in its allow-list returned exit status 1 and
`rncp: Resource transfer failed`; it did not report apparent success.

The pinned-Python interop run starts separate Rust and Python listener/client
processes with isolated TCP configs and identity files. It sends binary
`12,345`- and `16,384`-byte payloads in both directions and verifies the saved
bytes exactly. It then fetches a `9,876`-byte binary payload from the Python
listener using an independently identified Rust fetch identity, verifies the
metadata-driven save and exact bytes, and confirms a different identity is
rejected. The same trace pre-populates both receive destinations, enables
overwrite, and proves exact replacement without a `.1` collision suffix: a
Python sender replaces an existing Rust file, and a Python fetch client
replaces an existing destination with a file served by Rust. The latter also
exercises the Python fetch resource-conclusion/save callback through the
production path. The Python listener uses its production allow-list and fetch
jail; the Rust client accepts the Python reference's ordinary metadata-bearing
file Resource after its `True` request response, while the Rust listener now
serves that same ordinary Resource shape to Python. The trace authorizes the
send and fetch roles by their identified identity hashes. This covers bounded
authenticated send/fetch and overwrite/save-callback behavior through the
production Link/Resource path.

The `3c6757ba` compression-matrix run adds a highly compressible payload and a
payload pre-compressed with bzip2. It exercises Python sender default and
`-C` modes into Rust, Rust sender default and `--no-compress` modes into
Python, and Python listener default and `-C` fetch-response modes into a Rust
fetch client. Each process uses an isolated TCP interface, identity, and file
root; every received file matches the original bytes exactly. The Resource
unit tests remain the direct wire-flag proof; this process trace proves the
utility flags and mixed-runtime decompression/save behavior.

The `2b281b87` process increment also proves two negative categories through
the production CLI: a missing fetch exits nonzero with `remote file was not
found`, and a sender rejected by the listener's identity policy exits nonzero
with `Resource transfer failed` without creating a destination file. These
checks, together with `053ef246`, `9dc9bd62`, and `9b8e4ed6`, also cover
malformed identity, unusable save-path, path-discovery timeout, and local disk
failures. Interruption, cancellation, remote receive-side disk faults, and
multi-client behavior remain open; `d66b19d1` covers the bounded listener
restart path.

## Unresolved requirements

The following #611 acceptance items remain open and are deliberately not
classified as complete:

- Add direct progress/status callback telemetry and failure-side callback
  assertions around the completed Python fetch; the current trace proves the
  successful resource-conclusion/save side effect and exact overwrite result.
- Build the complete utility option/behavior matrix from every frozen
  `RNS/Utilities` entry point. The current slice does not add network workflows
  to `rnpath`, `rnprobe`, `rnsd`, or the radio/interactive utilities.
- Prove real `rngit` fetch/push/bundle workflows and configured initial-branch
  behavior under #601; bounded pinned-Python `/git/list`, `/git/fetch`,
  `/git/push`, `/git/delete`, `/git/create`, `/git/sync`, `/git/fork`, and
  `/git/mirror` requests now prove the `git.repositories`
  listing/bundle/mutation/clone seam, while the remaining Git/work network
  implementation belongs to #612/#613.
- Add interrupted-link, cancellation, slow-interface, remote receive-side
  disk-error, and multi-client transcripts with exact failure/status assertions.
- Add process-level advertisement/transfer-size assertions for each remaining
  compression role if the utility evidence must independently expose the wire
  compression flag; the Resource unit tests already cover that direct flag.

These are evidence or implementation gaps, not claims that the local Rust
process test represents Python interoperability or complete utility parity.
