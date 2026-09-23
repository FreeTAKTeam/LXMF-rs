# #611 utility/network evidence

Status: **partial / unverified**. This record covers the bounded native `rncp`,
`rnprobe`, and `rnsh` slices on the forward parity branch, including
authenticated pinned-Python and Rust sender/listener roles. The mixed-runtime compression increment is
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
`708dc980`; the interop fixture lock is covered by `a81f0cf6`; the successful
Python fetch-client completion callback is asserted by `e6f71d21`. The native
authenticated `rnsh` channel workflow is implemented by `f24e0038`, with
channel-window retry, bounded inbound queue overflow handling, and a large-
output regression added by `a32b6d71`. Its Rust process tests cover command
output, mirrored exit status, allow-list rejection, and flow-control output,
while both pinned-Python initiator/listener roles are covered by the ignored
interop fixture at `e57afb99`, with the command-before-stdin and bounded EOF
grace fix at `662dcdbe`.
The two-direction pinned-Python/native `rnprobe` process exchange is covered
by `f86ecc1c`.

## Reference and ownership

- Forward reference: Reticulum `99de23c040d507e3fefca19e87b182302902725d`
  (`1.5.4-dev`), including `RNS/Utilities/rncp.py` and
  `RNS/Utilities/rnsh/`.
- Rust owner: `crates/apps/rns-tools` using the existing
  `reticulum-rs-transport` Link/Resource, packet/receipt, daemon RPC, and TCP
  interface APIs.
- No second daemon or competing utility protocol was introduced: the native
  `rnsh` path uses the frozen Python channel envelope family. The existing
  local copy mode remains available when no network flags are supplied.

## Frozen utility option/behavior matrix

The matrix below is derived from the frozen Python entry-point help and source
at `99de23c040d507e3fefca19e87b182302902725d`, then compared with the Rust
entry points in the parity branch. “Partial” means that the Rust binary has a
useful product path but does not yet implement the reference workflow or
option family; it is not a callable-surface completion claim.

| Frozen entry point | Reference behavior families | Rust implementation and evidence | Current classification |
| --- | --- | --- | --- |
| `rncp` | Local copy; authenticated listener/send/fetch; jail/save/overwrite; compression; identity allow-list; progress, timeout, cancellation, and file failure status | Native TCP/Link/Resource send/fetch plus isolated Rust processes and pinned-Python send/fetch roles in this record | partial / bounded network slice evidenced |
| `rnpath` | Path table/rates; discovery; path and announce eviction; transport-via eviction; blackhole list/add/remove; remote management identity and timeout; JSON/human output | `rnpath-rs`/`rnpath` discovery and daemon-backed rate/eviction/blackhole subset; `rnpath_cli` and daemon RPC tests | partial / local daemon management subset evidenced |
| `rnprobe` | Resolve full destination name and hash, send probe payloads of configurable size/count, wait between probes, report RTT/hops/loss and status | Native `rnprobe` sends a `probe` RPC with Python-compatible defaults and options; the daemon resolves the destination identity/path, validates the name/hash and packet limits, sends random payload packets, correlates delivery proofs through the existing receipt bridge, and returns per-probe RTT/hops/loss. `respond_to_probes = true` registers the `rnstransport.probe` destination with `ProofStrategy::All` and announces it through the existing transport schedule. `f86ecc1c` exercises pinned Python→Rust and native Rust→pinned Python probe roles over isolated TCP interfaces. | partial / bounded software workflow evidenced; physical/public-network and fault/restart evidence remain open |
| `rnsd` | Configured daemon launch, service/interactive modes, verbosity, example configuration | Rust compatibility shim resolves and delegates to `reticulumd`; delegation/help/status tests exist | partial / daemon delegation evidenced |
| `rnid` | Generate/import/export identities; announce/hash; sign/validate; encrypt/decrypt; metadata; optional network identity request and encoding modes | Rust `rnid` generates and displays persisted private identities with overwrite protection | partial / local identity subset evidenced |
| `rnir` | Resolver configuration, verbosity, example configuration, and resolver runtime integration | Rust accepts global/config/example options but does not expose a resolver network workflow | partial / configuration-only |
| `rnodeconf` | Serial RNode information, firmware/bootstrap/update, EEPROM, Wi-Fi/Bluetooth/display/radio management, signing/trust operations | Rust `rnodeconf-rs` exposes daemon-backed management commands and mock-RPC coverage; physical serial/firmware rows are separate | partial / software management evidenced; hardware-unverified |
| `rnpkg` | Package-manager configuration and package workflow entry point | Rust exposes global/example-config options only, matching the currently shipped no-subcommand surface | partial / configuration-only |
| `rnsh` | Authenticated remote shell listener/initiator; identity/allow-list/no-auth; command policy; stdin/stdout/stderr streams; timeout and mirrored exit status | `f24e0038` adds a native TCP/Link/Channel listener and initiator using the frozen `0xAC00`–`0xAC07` envelope family, persisted identities, allow-list/no-auth modes, root-scoped command execution, remote-command policy, stream forwarding, timeout, and mirrored exit status. `a32b6d71` retries channel-window backpressure, fails closed on bounded inbound queue overflow, and exercises a 128 KiB output stream. `rnsh_process` covers Rust↔Rust command output and authenticated allow-list rejection; `rnsh_python_interop` covers both pinned-Python initiator→Rust listener and Rust initiator→pinned-Python listener output and exit status (`e57afb99`). `662dcdbe` orders the execute envelope before stdin EOF and adds bounded EOF grace for the pinned listener's short-command cleanup behavior. The existing local root-scoped executor remains the no-network mode. | partial / bounded native and both pinned-Python roles evidenced |
| `rnx` | Authenticated Reticulum remote execution, listener/initiator, interactive and stream options, identity and timeout controls | Rust `rnx` is a production interop/diagnostic harness with mesh, resource, BLE, TCP, and path scenarios; its scenarios are not a drop-in `rnsh` endpoint | partial / harness workflows evidenced, reference remote shell remains open |
| `rngit` | Reticulum Git client/server, repository and work operations, bundles, pages/media, permissions, signatures, and network failure/restart behavior | Rust local CLI plus daemon-side service handlers; #612/#613 records pinned-Python request/bundle/page/media seams and the reciprocal native Rust-client `/git/list`/`/git/fetch`/`/git/push`/signed `/mgmt/work` plus bounded release request trace, with exact fetched-bundle and pushed-ref verification | partial / split across #611–#613 |

The matrix prevents parser-only or local-only commands from being promoted as
reference-equivalent network utilities. Hardware-facing `rnodeconf` rows and
physical/public-network evidence remain outside the software-only pass.

## Implemented behavior matrix

| Workflow | Implementation-backed behavior | Evidence | Status |
| --- | --- | --- | --- |
| Local copy | Root-scoped binary copy, overwrite guard, parent traversal rejection | `rncp.rs` unit test | verified for the existing local convenience mode |
| Listener | TCP listener, deterministic/persisted identity, `rncp.receive` announce, periodic re-announce, save directory, collision-safe filename selection | `rncp_process` listener process; pinned Python client/service trace | implemented; no-auth Python interoperability evidenced |
| Send | TCP announce discovery, Link establishment, local identity identification, Resource send with `{"name": <binary filename>}` metadata, adaptive timeout and terminal failure status | Rust↔Rust process test; pinned Python client/service trace | verified for two independent processes and the bounded Python roles |
| Fetch | `fetch_file` Link request/response, `True`/`False`/`0xF0`/`nil` status mapping, the pinned Python rncp listener's ordinary metadata-bearing file Resource contract, correlated Rust response Resources for other callers, and metadata-driven save | `rncp_process` Rust client to Rust listener; pinned Python listener/client trace; pinned Python client fetching from Rust | verified for two independent Rust processes and the bounded Python↔Rust fetch paths |
| Authentication | `--no-auth`, explicit `--allowed-identity`, rejected identified peers, nonzero sender failure, and reciprocal Python/Rust identity allow-lists for send and fetch roles | manual denied-transfer run; pinned Python interop | verified for the bounded send/fetch roles; broader option and callback parity remains open |
| Jail and save safety | Canonical jail containment, traversal rejection, basename-only metadata, overwrite/suffix behavior | protocol unit tests and process test | verified locally |
| Timeout/output | `--timeout`, silent mode, nonzero status for denied senders, preserved not-found failure output for missing fetches, malformed identity rejection, unusable save-path rejection, path-discovery timeout status, client Ctrl-C cancellation, interrupted Resource-link failure, medium-path timeout after an active TCP interface connects, delayed/rate-limited TCP-path transfer, Python fetch completion and save-failure callbacks, and listener receive-save failure diagnostics | unit/process tests and pinned Python interop | bounded process-level outcomes are verified; the pinned Python fetch callback prints its save error but leaves the operation unresolved, so accurate terminal failure status remains a gap; Python-peer receive-side cancellation remains open |
| Status output | Non-silent path request, link-establishment, transfer, and fetch-request phase lines; silent mode suppresses them; the Python fetch client emits `Transfer complete` on successful save | `rncp_process`, CLI phase transcript, pinned Python interop | verified for the native Rust client path and the bounded Python fetch-client path |
| Restart | Persisted listener identity, same TCP endpoint, stable destination hash, and a second binary transfer after listener restart | `rncp_process`; ignored `rncp_python_interop` restart process | verified for the bounded Rust listener/client path and the Python-listener/Rust-client role |
| Disk failure | Rust client/listener save failures; pinned Python listener receive-save failure; pinned Python fetch-client save callback failure after the save directory becomes unusable mid-transfer | `rncp_process::rncp_listener_reports_received_file_disk_error`; ignored `rncp_python_interop::rncp_python_listener_reports_received_file_disk_error`; ignored `rncp_python_fetch_failure::rncp_python_fetch_client_save_error_is_reported_but_never_resolved` | The Python fetch callback emits its save error but never resolves the completed transfer and the CLI remains running; the regression records this pinned-reference defect, not successful terminal failure handling. No case claims an application-level negative acknowledgment to the sender |
| Multi-client | Three independent clients send distinct binary files concurrently to one listener | `rncp_process` | verified for the bounded Rust listener/client path |
| Adaptive timeout | Initial TCP clients reach `connected` before network work begins, allowing `operation_timeout` to observe the active interface bitrate and apply the RNS medium-path lower bound; a slow proxy delays the first server response and rate-limits both directions during a real send | `rncp_process::rncp_uses_medium_timeout_after_interface_activation`; `rncp_process::rncp_completes_after_delayed_first_hop_on_a_rate_limited_tcp_path` | verified for active local TCP and a delayed/rate-limited software TCP path; carrier-specific and physical timing are not claimed |
| Packet probe exchange | Probe packet delivery and proof correlation between the native daemon path and the pinned Python utility, in both initiator/responder directions | ignored `rnprobe_python_interop` (2 tests, commit `f86ecc1c`) | verified for isolated Rust daemon/Python TCP roles; public/multi-hop, carrier-fault, and physical timing remain open |
| Compression option | `--no-compress` disables opportunistic Resource compression for outbound sends and fetch responses while preserving the default auto-compression path | Resource compression regression, `rncp_process`, mixed Python/Rust compression matrix | verified for the focused Python↔Rust send and listener-side fetch-response roles; the successful Python fetch-client completion callback and native listener save-failure diagnostic are asserted, while Python-peer failure callbacks and the complete utility matrix remain open |
| Path management | `rnpath-rs`/`rnpath` discovers paths through daemon RPC and now exposes daemon-backed rate inspection, path and announce-queue eviction, path-via eviction, blackhole listing, and timed/reasoned blackhole add/remove operations with human and JSON output | `rnpath_cli` mock-RPC and parser/process regressions | verified for the Rust client/daemon RPC boundary; pinned-Python utility roles and the remaining reference path-table/remote-management options remain open |
| Remote shell | Native authenticated listener/initiator, frozen channel message numbers, root-scoped process launch, stdin/stdout/stderr stream framing, command policy, timeout, mirrored exit status, and allow-list rejection | `rnsh` unit tests; `rnsh_process`; ignored `rnsh_python_interop` (`e57afb99`, `662dcdbe`) | verified for the bounded software/TCP slice in both pinned-Python roles, including the immediate EOF case; PTY/resize and the full option/fault/restart matrix remain open |
| Other shipped utilities | `rnsd`, `rnid`, `rnir`, `rnodeconf`, `rnpkg`, `rnsh`, `rnx`, and `rngit` | existing tests and callable inventory; `rnprobe` is recorded in the row above | not promoted by this slice; network/reference gaps remain |

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
  rncp_exchanges_binary_files_with_pinned_python_in_both_directions \
  -- --ignored --nocapture                         1 passed (32.52s)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  -- --ignored --nocapture                         3 passed (37.26s)
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  rncp_python_listener_restart_preserves_identity_and_transfer \
  -- --ignored --nocapture                         1 passed (1.62s)
RETICULUM_PY_REPO=Reticulum-parity LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  rncp_python_listener_reports_received_file_disk_error \
  -- --ignored --exact --nocapture                  1 passed (0.82s)
```

The issue-specific receiver disk-failure increment was verified separately in
its isolated branch:

```text
cargo fmt --all -- --check                         PASS
cargo test -p rns-tools --test rncp_process \
  rncp_listener_reports_received_file_disk_error \
  -- --nocapture                                    1 passed
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
error. The Rust receiver disk-failure regression sends a binary Resource whose
basename collides with a directory while listener overwrite is enabled, then
waits for and asserts the listener's save-failure diagnostic. The ignored
pinned-Python counterpart sends a binary Resource to a Python listener whose
configured save root is a regular file; it asserts the sender's Resource
delivery succeeds while the Python callback logs the save error. Both
distinguish transport delivery from application save, and neither claims an
application-level negative acknowledgment to the sender. PR Verify runs the
pinned-Python regression against the frozen 1.5.4 development checkout.
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

The delayed-path regression transfers an 8 KiB binary file through a TCP proxy
that delays the first server response by two seconds and waits 25 ms per
forwarded 256-byte chunk in both directions. The Rust client uses
`--timeout 1`, yet completes with exact saved bytes and nonzero traffic in both
directions; this exercises the adaptive timeout on a slow software path rather
than only checking its lower bound during a path-discovery timeout. It does not
claim carrier-specific or physical-link timing.

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

Commit `e6f71d21` removes the Python fetch client's quiet-mode flag for this
fixture and asserts its successful `Transfer complete` callback output. The
listener-side save callback remains represented by the exact saved-byte
assertion; its console logging is not treated as a stable contract because the
pinned listener is run with quiet logging and terminated after the transfer.

The ignored exact-target `rncp_python_fetch_failure` process regression makes
the validated Python fetch save directory unusable after the CLI accepts it,
then observes `An error occurred while saving received resource:` from the
fetch-client callback. The pinned callback returns before setting
`resource_resolved`, so the command remains alive; the test bounds and stops
that process. This demonstrates the reference failure path but does not satisfy
the accurate-terminal-status acceptance requirement or imply a sender-visible
negative acknowledgment.

The `3c6757ba` compression-matrix run adds a highly compressible payload and a
payload pre-compressed with bzip2. It exercises Python sender default and
`-C` modes into Rust, Rust sender default and `--no-compress` modes into
Python, and Python listener default and `-C` fetch-response modes into a Rust
fetch client. Each process uses an isolated TCP interface, identity, and file
root; every received file matches the original bytes exactly. The Resource
unit tests remain the direct wire-flag proof; this process trace proves the
utility flags and mixed-runtime decompression/save behavior.

PR #631 extends this process trace with test-only instrumentation of the pinned
Python `ResourceAdvertisement.pack()` path and the Link's resource-advertised
callback. It observes the actual packed or received advertisement fields and
deduplicates outgoing retries by Resource hash and segment. The six roles now
assert `compressed`, `transfer_size`, and `data_size`: Python→Rust sends in
default/`-C` modes, Rust→Python sends in default/`--no-compress` modes, and
Python fetch responses in default/`-C` modes. Compressed payloads advertise a
smaller transfer than data size; uncompressed payloads preserve at least the
data size (the Python reference's transfer size also includes metadata and
random-hash overhead). The recorder is confined to the test's Python path and
does not change production behavior.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
  LXMF_PYTHON_BIN=python3 cargo test -p rns-tools --test rncp_python_interop \
  rncp_mixed_runtime_compression_matrix_roundtrips_binary_files \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed; repeated 3 consecutive times

RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
  LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rncp_python_interop \
  -- --ignored --nocapture --test-threads=1
# 4 passed (38.16s)

cargo test -p rns-tools --test rncp_process -- --nocapture
# 11 passed (6.19s)
```

Verify now runs the focused compression command as an explicit exact-target
gate. Its hosted result is required before treating this CI increment as
complete.

The ignored Python interop fixtures share a process-level lock
(`a81f0cf6`) because an earlier parallel run allowed listener/announce
contention to produce one compression-matrix path-discovery timeout. With the
lock in place, the four-test command completed all tests in 38.16
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

## Current `rnpath` management increment

Commit `aff10e67` extends the existing daemon-RPC utility path without adding
another protocol. The positional destination is now optional for management
operations, while ordinary discovery still requires a validated 16-byte hash.
The CLI dispatches `--rates` to `get_rate_table`, `--drop` to `drop_path`,
`--drop-announces` to `drop_announce_queues`, `--drop-via` to `drop_all_via`,
`--blackholed` to `get_blackholed_identities`, and `--blackhole`/
`--unblackhole` to the corresponding persisted identity policy operations.
Timed blackholes are converted from hours to a Unix expiry and the optional
reason is preserved in the RPC request. All management responses support the
existing human and JSON output modes; invalid combinations fail before any
backend connection.

```text
cargo test -p rns-tools --test rnpath_cli rnpath_ -- --nocapture
# 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; 32.03s

cargo clippy -p rns-tools --bin rnpath-rs --test rnpath_cli \
  --all-features --no-deps -- -D warnings
# passed

cargo test -p rns-tools --bin rnpath-rs --all-features
# 4 passed; 0 failed

tools/scripts/check-module-size.sh
# module-size checks: ok
```

These are implementation-backed daemon-RPC tests with a mock server; they do
not claim a physical carrier or a Python utility process. `rnsh`, `rnir`, `rnpkg`,
and hardware-facing `rnodeconf` remain separate parity rows.

## Current `rnprobe` packet increment

Commit `98e4eb63` replaces the old `rnpath` delegation wrapper with the first
native packet-probe workflow. `rnprobe FULL_NAME DESTINATION_HASH` now sends
the legacy daemon `probe` RPC with the frozen Python defaults (`size=16`,
`probes=1`, `timeout=12`, and `wait=0`) and supports configurable payload
size/count, per-probe timeout, inter-probe wait, human/JSON output, TCP or Unix
RPC, and packet-loss exit status `2`. The daemon-side bridge waits for a
destination identity/path, constructs the named `SingleOutputDestination`,
sends random payload packets through the existing transport, registers the
final encrypted packet hash before dispatch, and waits for the normal delivery
receipt. The result retains one row per probe with delivered/timeout status,
RTT, packet hash, hops, reply count, and loss percentage.

The responder is opt-in through the existing `reticulum.respond_to_probes`
runtime policy. When enabled, daemon startup registers
`rnstransport.probe` with `ProofStrategy::All`, prints its destination hash,
and includes it in existing startup and scheduled announce paths. The
delivery receipt bridge notifies only the shared probe registry before
continuing its normal message-receipt mapping, so diagnostic probes do not
consume or alter ordinary LXMF receipt state.

~~~text
cargo test -p rns-tools --test rnprobe_cli
# 4 passed; 0 failed

cargo test -p rns-tools --bin rnprobe
# 4 passed; 0 failed

cargo test -p reticulum-rs-rpc --lib probe_rpc
# 3 passed; 0 failed

cargo test -p reticulumd --test receipt_bridge
# 3 passed; 0 failed

cargo test -p reticulumd --bin reticulumd probe_destination_is_opt_in_and_uses_rnstransport_probe_name
# 1 passed; 0 failed

cargo test -p reticulumd --bin reticulumd path_lookup_bridge_
# 7 passed; 0 failed

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rnprobe_python_interop \
  -- --ignored --nocapture
# 2 passed; 0 failed (Python→Rust and native Rust→Python)

cargo clippy -p reticulumd -p reticulum-rs-rpc -p rns-tools --all-targets --all-features --no-deps -- -D warnings
# passed

tools/scripts/check-boundaries.sh
# boundary checks: ok

tools/scripts/check-module-size.sh
# module-size checks: ok
~~~

The CLI integration tests use a local mock RPC server and verify the exact
probe option envelope, human RTT formatting, JSON preservation, malformed
destination rejection, and exit status `2` for partial loss. The focused
daemon tests verify RPC defaults/aliases, the unavailable-bridge error, the
receipt-registry handoff, and opt-in responder naming. The ignored process
test starts a Rust daemon with an announced probe responder for pinned Python,
then starts a pinned-Python `PROVE_ALL` responder for native `rnprobe`; both
directions deliver two probes with zero loss over isolated TCP interfaces.
This is still software-only evidence: no carrier fault matrix, multi-hop/
public-network run, hardware run, or performance claim is included.

## Current `rnsh` channel increment

Commits `f24e0038` and `a32b6d71` replace the former local-only `rnsh` implementation with a
bounded native network workflow while preserving local mode when no network
flags are supplied. The listener and initiator use the existing Reticulum
Link/Channel transport and the frozen Python `rnsh` message family: version,
execute, stream (`0xAC04`), error, window, no-op, and command-exit envelopes.
The stream codec matches the Python two-byte EOF/compression header and accepts
Python bzip2-compressed input; native output is chunked with the negotiated
Channel MDU and retries flow-control backpressure before sending stream data,
EOF, or the terminal exit message. The bounded inbound session queue reports
overflow and closes the session instead of silently dropping channel messages.

The network CLI supports persisted or deterministic identities, exact
no-aspect `rnsh` destination hashing, TCP listener/client interfaces,
allow-list or explicit no-auth operation, root-scoped command execution,
remote-command policy flags, timeout, and mirrored exit status. The local mode
continues to require a root and an explicit allow-list entry for the executable.

```text
cargo test -p rns-tools --bin rnsh -- --nocapture
# 8 passed; 0 failed

cargo test -p rns-tools --test rnsh_process -- --nocapture
# 2 passed; 0 failed
# includes a 128 KiB output stream through the negotiated Channel window

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p rns-tools --test rnsh_python_interop -- \
  --ignored --nocapture --test-threads=1
# 2 passed; 0 failed (1.81s)

cargo test -p rns-tools --tests
# passed; ignored Python fixtures remain ignored by default
```

The Rust process fixture starts an independent listener and initiator over
loopback TCP, resolves the listener destination from its persisted identity,
executes `/bin/echo`, and verifies the forwarded output plus mirrored status.
Its authenticated case allows one client identity, verifies a successful
command, then verifies a different identity receives a nonzero failure. The
pinned-Python fixture starts the frozen Python initiator with an isolated TCP
configuration and identity, and verifies its command output and exit status on
the Rust listener. The reciprocal fixture starts the pinned Python listener
with a server-side TCP interface and an isolated identity, sends the execute
envelope before stdin, and verifies Rust command output plus the Python
`CommandExited(0)` response with an immediately closed non-TTY stdin. The Rust
initiator's bounded EOF grace accommodates the pinned listener's short-command
stdin-close cleanup path. This is a bounded software/TCP slice: PTY allocation
and resize, native outbound compression, full restart/fault/cancellation
coverage, and public or multi-hop transport remain unverified.

## Unresolved requirements

The following #611 acceptance items remain open and are deliberately not
classified as complete:

- Add failure-side callback assertions for the Python fetch client receiving a
  Resource, and correlate the completed Python fetch with stable listener-side
  save-status evidence. `e6f71d21` asserts the successful Python fetch client's
  `Transfer complete` callback; `rncp_python_listener_reports_received_file_disk_error`
  now proves the distinct Python listener receive callback, not fetch-client
  save failure or a sender-visible negative acknowledgment.
- Build the complete utility option/behavior matrix from every frozen
  `RNS/Utilities` entry point. The current slice now covers the daemon-backed
  `rnpath` management subset, a bounded native `rnprobe` packet workflow with
  both pinned-Python initiator/responder roles, and a bounded native `rnsh`
  channel workflow with both pinned-Python initiator/listener roles, including
  the bounded immediate-EOF trace, but does not prove full `rnsh` PTY/resize/
  fault/restart behavior, public/multi-hop behavior, or network workflows to `rnsd` and the
  radio/interactive utilities.
- Prove real `rngit` fetch/push/bundle workflows and configured initial-branch
  behavior under #601; bounded pinned-Python `/git/list`, `/git/fetch`,
  `/git/push`, `/git/delete`, `/git/create`, `/git/sync`, `/git/fork`, and
  `/git/mirror` requests now prove the `git.repositories`
  listing/bundle/mutation/clone seam, while the remaining Git/work network
  implementation belongs to #612/#613.
- Add remote receive-side cancellation transcripts with exact failure/status
  assertions. The delayed/rate-limited TCP proxy now exercises the adaptive
  timeout on a slow software path; carrier-specific and physical-link timing
  are not claimed. Rust and pinned-Python disk-error callbacks are covered.

These are evidence or implementation gaps, not claims that the local Rust
process test represents Python interoperability or complete utility parity.
