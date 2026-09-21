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
| Listener | TCP listener, deterministic/persisted identity, `rncp.receive` announce, periodic re-announce, save directory, collision-safe filename selection | `rncp_process` listener process; pinned Python client/service trace | implemented; no-auth Python interoperability evidenced |
| Send | TCP announce discovery, Link establishment, local identity identification, Resource send with `{"name": <binary filename>}` metadata, adaptive timeout and terminal failure status | Rust↔Rust process test; pinned Python client/service trace | verified for two independent processes and the bounded Python roles |
| Fetch | `fetch_file` Link request/response, `True`/`False`/`0xF0`/`nil` status mapping, response Resource, metadata-driven save | `rncp_process` Rust client to Rust listener | verified for two independent processes |
| Authentication | `--no-auth`, explicit `--allowed-identity`, rejected identified peers, nonzero sender failure | manual denied-transfer run | verified locally for the negative path; Python allow-list parity unverified |
| Jail and save safety | Canonical jail containment, traversal rejection, basename-only metadata, overwrite/suffix behavior | protocol unit tests and process test | verified locally |
| Timeout/output | `--timeout`, silent mode, accurate failure output for missing/denied/failed transfers | unit/manual process runs | bounded Rust behavior verified |
| Compression option | `--no-compress` disables opportunistic Resource compression for outbound sends and fetch responses while preserving the default auto-compression path | Resource compression regression, `rncp_process` | locally verified; mixed Python compression matrix remains open |
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
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  -- --ignored --nocapture                         1 passed
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

The pinned-Python interop run starts separate Rust and Python listener/client
processes with isolated TCP configs and identity files. It sends binary
`12,345`- and `16,384`-byte payloads in both directions and verifies the saved
bytes exactly. This covers the no-auth Python client and service roles through
the production Link/Resource path; it does not promote authenticated allow-list
parity.

## Unresolved requirements

The following #611 acceptance items remain open and are deliberately not
classified as complete:

- Exercise Python's identity-file, allow-list, jail, overwrite, and callback
  behavior through the cross-implementation path; the current trace covers
  isolated identity files and no-auth operation only.
- Build the complete utility option/behavior matrix from every frozen
  `RNS/Utilities` entry point. The current slice does not add network workflows
  to `rnpath`, `rnprobe`, `rnsd`, or the radio/interactive utilities.
- Prove real `rngit` fetch/push/bundle workflows and configured initial-branch
  behavior under #601; bounded pinned-Python `/git/list`, `/git/fetch`,
  `/git/push`, `/git/delete`, `/git/create`, `/git/sync`, `/git/fork`, and
  `/git/mirror` requests now prove the `git.repositories`
  listing/bundle/mutation/clone seam, while the remaining Git/work network
  implementation belongs to #612/#613.
- Add restart, interrupted-link, cancellation, slow-interface, disk-error, and
  multi-client transcripts with exact failure/status assertions.
- Exercise the explicit no-compression path in the full pinned-Python
  transfer matrix, including compressed and already-compressed payloads and
  response-side assertions; the production option is now wired and the
  cross-process payload regression remains green.

These are evidence or implementation gaps, not claims that the local Rust
process test represents Python interoperability or complete utility parity.
