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
`9b8e4ed6`, and client Ctrl-C cancellation status is covered by
`397a9525`. Concurrent clients are covered by `e668ae60`.
Interrupted-link status and flushed non-silent phase output are covered by
`a5f57dba`. Adaptive medium-path timeout after TCP interface activation is
covered by `27bb3fac`. Python-listener restart with a Rust client is covered by
`708dc980`; the interop fixture lock is covered by `a81f0cf6`.

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
| Timeout/output | `--timeout`, silent mode, nonzero status for denied senders, preserved not-found failure output for missing fetches, malformed identity rejection, unusable save-path rejection, path-discovery timeout status, client Ctrl-C cancellation, interrupted Resource-link failure, and medium-path timeout after an active TCP interface connects | unit/manual process runs, `rncp_process` | nine bounded process-level failure/status and adaptive-timeout outcomes are verified; genuinely slow-interface and remote receive-side cancellation remain open |
| Status output | Non-silent path request, link-establishment, transfer, and fetch-request phase lines; silent mode suppresses them | `rncp_process`, CLI phase transcript | verified for the native Rust client path |
| Restart | Persisted listener identity, same TCP endpoint, stable destination hash, and a second binary transfer after listener restart | `rncp_process`; ignored `rncp_python_interop` restart process | verified for the bounded Rust listener/client path and the Python-listener/Rust-client role |
| Disk failure | Fetch save failure when the overwrite target is a directory | `rncp_process` | verified with nonzero status and preserved OS error output; remote receive-side save faults remain open |
| Multi-client | Three independent clients send distinct binary files concurrently to one listener | `rncp_process` | verified for the bounded Rust listener/client path |
| Adaptive timeout | Initial TCP clients reach `connected` before network work begins, allowing `operation_timeout` to observe the active interface bitrate and apply the RNS medium-path lower bound | `rncp_process::rncp_uses_medium_timeout_after_interface_activation` | verified for an active local TCP interface; genuinely slow-interface timing remains open |
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
  --all-features -- --nocapture                     10 passed (6.18s)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  rncp_mixed_runtime_compression_matrix_roundtrips_binary_files \
  -- --ignored --nocapture                         1 passed (4.85s)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  -- --ignored --nocapture                         3 passed (37.13s)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  rncp_python_listener_restart_preserves_identity_and_transfer \
  -- --ignored --nocapture                         1 passed (1.62s)
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
second binary transfer. The same run attempts a fetch with an overwrite target
that is a directory and asserts nonzero status plus the `Is a directory` OS
error.
The Unix process regression also sends SIGINT during path discovery and asserts
nonzero status plus `operation cancelled by user`. The concurrent-client case
starts three independent senders and verifies every listener-side file byte for
all three completed transfers. The interrupted-link case waits for the flushed
`Transferring file...` phase line, terminates the listener, and verifies a
nonzero Resource failure without a saved partial file. The adaptive-timeout
case starts a real listener, waits for the client-side TCP interface to become
connected, requests an unknown destination with `--timeout 1`, and observes
the medium-path timeout lower bound: the run took 6.18 seconds and returned
`path discovery timed out`.

The mixed-runtime restart regression starts the pinned Python listener with a
persisted identity and an allow-listed Rust sender, sends a binary file, stops
the listener, starts it again on the same TCP endpoint, verifies that its
destination hash is unchanged, and sends a second binary file. Both files were
saved with exact bytes; the ignored test completed in 1.62 seconds.

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

The three ignored Python interop fixtures share a process-level lock
(`a81f0cf6`) because an earlier parallel run allowed listener/announce
contention to produce one compression-matrix path-discovery timeout. With the
lock in place, the default three-test command completed all tests in 37.13
seconds; the process isolation and exact file assertions are unchanged.

The `2b281b87` process increment also proves two negative categories through
the production CLI: a missing fetch exits nonzero with `remote file was not
found`, and a sender rejected by the listener's identity policy exits nonzero
with `Resource transfer failed` without creating a destination file. These
checks, together with `053ef246`, `9dc9bd62`, and `9b8e4ed6`, also cover
malformed identity, unusable save-path, path-discovery timeout, and local disk
failures; `397a9525` covers client Ctrl-C cancellation; and `e668ae60` covers
the bounded multi-client path. Commit `27bb3fac` covers readiness-gated medium
timeout selection after TCP interface activation. Slow-interface, interrupted-
link follow-up behavior beyond the bounded local process, and remote
receive-side cancellation/disk faults remain open; `d66b19d1` covers the
bounded listener restart path. Commit `a5f57dba` covers interrupted Resource
failure and native phase output.

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
- Add genuinely slow-interface and remote receive-side cancellation/disk-error
  transcripts with exact failure/status assertions; the active-TCP
  medium-timeout lower-bound case is covered, but it is not a slow-interface
  or physical-link transcript.
- Add process-level advertisement/transfer-size assertions for each remaining
  compression role if the utility evidence must independently expose the wire
  compression flag; the Resource unit tests already cover that direct flag.

These are evidence or implementation gaps, not claims that the local Rust
process test represents Python interoperability or complete utility parity.
