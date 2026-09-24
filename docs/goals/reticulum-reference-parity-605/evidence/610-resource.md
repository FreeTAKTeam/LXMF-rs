# Issue #610: Resource collision, shutdown, and mixed-peer evidence

Status: implemented but unproven. This is a forward-candidate software slice;
it does not promote the full #610 acceptance contract or close parent issue
#605.

## Reference and candidate

- Candidate branch: `codex/issue-605-parity`.
- Historical evidence candidate: `4ebaf762236e03df8ae56fd55696bd51c2e3de46`.
- Current branch carrying this behavior: `8b29132c`; later documentation
  commits preserve this slice. The historical checks below are not being
  relabeled as reruns at the newer commit.
- Candidate base: `a5425366` (the merged PR #603 base used by the #605 plan).
- Pinned Reticulum reference: `99de23c040d507e3fefca19e87b182302902725d`.
- Reference surfaces: `RNS/Resource.py`, `RNS/Link.py`, and
  `RNS/Transport.py`.
- Rust owners: `crates/libs/rns-transport/src/resource`, link cleanup, and
  the pinned Python channel interop harness.

## Implemented in this candidate

- Resource sender hash selection now deterministically supports collision
  regeneration through an injected candidate source. The rolling collision
  guard is bounded by the reference collision-guard size and the resource,
  proof, and map hashes are recomputed for the accepted random hash.
- Unit coverage forces a collision instead of relying on probability and
  verifies that the guard permits a repeated map hash only after the complete
  guarded window has left scope.
- A deterministic sender regression requests a hashmap update at the second
  segment boundary. It asserts the exact moving
  `receiver_min_consecutive_height` formula, decodes the emitted global segment
  index and hash slice, and verifies the first in-range request plus rejection
  just below the lower bound and at the exclusive upper bound.
- Link-state removal now emits terminal inbound and outbound resource failure
  events, deduplicates split-resource state, preserves unrelated links, and
  publishes the events through the production maintenance and reset paths.
- Receiver fragment admission is bounded to the active request window, matching
  `RNS/Resource.py::receive_part`; a delayed or future fragment cannot advance
  the contiguous frontier outside the round that requested it.
- Deterministic manager tests cover a reordered round with one dropped and one
  duplicated fragment, recovery through a retry request, and cancellation at
  every split-segment position without retaining the unbuilt tail.
- The public `ResourceManager` and `Transport` surfaces accept a synchronous
  `Read + Send + Sync` source with an exact byte count. Split sends retain the
  reader rather than the complete payload, read one segment per proof, and
  turn later reader failures into a terminal outbound-failure event.
- The Python interop helper can send a deterministic file-backed resource and
  reports an exact SHA-256 digest. Release-profile mixed-peer tests cover
  empty, one-byte, just-below-efficient, split, and 50 MiB resources in both
  Rust-to-Python and Python-to-Rust directions. A separate pinned-Python trace
  exercises the Rust `send_resource_from_reader` API with a real file handle,
  a split `MAX_EFFICIENT_SIZE + 257` payload, and verifies the exact
  cross-peer digest.
- A two-carrier pinned-Python trace now forwards a split Resource through one
  Rust transport with forwarding enabled. The client waits for the remote
  Python endpoint callback and verifies the exact size and SHA-256 digest.
- The pinned-Python interop suite now drives cancellation in both directions:
  a Python receiver rejects a Rust reader-backed split send and Rust emits one
  `OutboundRejected` terminal event, while a Python sender cancels after
  advertisement and Rust emits `InboundFailed(reason=remote_cancelled)`.
  The Python sender also reports its own `FAILED` callback status. A local Rust
  caller cancellation remains the distinct `OutboundCancelled` event.
- The new pinned-Python sender regression waits until data has been sent in
  split segment 2 before calling `Resource.cancel()`. Through the production
  TCP/Link/Resource receiver, Rust observes `InboundFailed` with the exact
  reference-compatible reason `remote_cancelled`; Python independently reports
  `FAILED` and segment 2. Existing
  `remote_cancel_clears_the_partial_split_assembly_and_reports_failure`
  directly asserts that cancellation on segment 2 removes the active receiver
  and split assembly and reports one `remote_cancelled` failure. The interop
  test adds no production state introspection API.
- The `lxmf-runtime` Resource-event consumer now has explicit terminal-event
  regressions: `OutboundFailed`, `OutboundRejected`, and `OutboundCancelled`
  become SDK transport errors with distinct caller-visible messages, and each
  terminal path attempts cleanup rather than reporting success.
- A pinned-Python shutdown trace now waits for the receiver's
  `resource_started` callback, terminates that exact Python process after the
  Rust advertisement is admitted, and observes one Rust `OutboundFailed`
  event for the original split transfer.
- A pinned-Python file-backed failure trace opens a real Rust `File` reader,
  truncates the backing file after Python admits the Resource, and observes one
  Rust `OutboundFailed` event rather than a false completion.
- A pinned-Python sender-side file-like reader now raises after a partial read
  while preparing a later split segment. The Rust receiver observes a terminal
  inbound failure (`retry_limit_exhausted` or `link_closed` under the bounded
  test deadline), while the Python process exits unsuccessfully and preserves
  both the injected exception and its bounded resource timeout in stderr.
- The same real-carrier matrix now uses a Rust `Read + Send + Sync` source and
  covers dropped, duplicated, reordered, and completely missing Resource
  data. A separate reader-backed trace injects an error while building the
  second segment after the Python receiver has acknowledged the first.
- A pinned-Python keepalive fault trace drops only `PacketContext::KeepAlive`
  frames after link establishment, leaves setup and teardown control intact,
  and observes Rust's watchdog close the link with `LinkEvent::Closed`.

## 64 MiB outbound compression threshold correction

The pinned reference at `99de23c040d507e3fefca19e87b182302902725d` defines
`AUTO_COMPRESS_MAX_SIZE` as the maximum source-data size for attempting bz2
compression. In `Resource.__init__`, `data_size` is the known source length;
`total_size` adds metadata for split selection. File-like resources above
`MAX_EFFICIENT_SIZE` calculate `total_segments`, read only the current
segment, and retain the file for the next segment. There is no 64 MiB
outbound admission check. The live pinned-reference probe already on PR #638
observed a 64 MiB + 1 source with compression disabled, `total_size` equal to
the source length, 65 segments, and only the first segment prepared.

The production reader sender now uses the 64 MiB source-size threshold only to
select compression and continues to retain larger known-size sources as a
reader. The byte-slice sender likewise no longer treats the compression
threshold as an admission ceiling. Both split paths use checked conversion of
segment counts into the Rust sender's `u32` segment-index representation;
this is a representational failure boundary, not a new reference-derived
resource-size policy. Inbound transfer limits are unchanged.

The Rust boundary regression verifies admission of 64 MiB + 1, exact logical
size and 65 segments, an uncompressed advertisement, and exactly one first
segment read. A separate test verifies a typed failure if segment count cannot
fit the sender's representation. These checks do not exercise a complete
64 MiB + 1 transfer with a Python peer and do not complete the broader #610
acceptance matrix.

Fix commit: `4902b304f5d36f9a16a99538faed96a6b5272c6c` on the existing PR #638
branch, based on the live PR head `740ee35e22f7f8ec989fe87ba8e147a7430ad9ae`.
Validation results for that candidate are recorded below.

## SDK consumer terminal-event regressions

Added at Rust commit `4bc7188e515d1dc14a8f1437080134c020ba5877` in
`crates/libs/lxmf-runtime/src/tests.rs`. Real `OutboundFailed` and
`OutboundCancelled` events are passed through the SDK consumer's
`await_resource_completion_with_cancel`; the tests assert transport-category
errors, distinct messages, and cleanup callbacks. This is focused
library-consumer evidence, not completion of the broader consumer matrix.

```text
cargo test -p lxmf-runtime  # 14 passed
cargo clippy -p lxmf-runtime --all-targets --all-features --no-deps -- -D warnings
```

## Local evidence

The following checks passed on the candidate:

```text
cargo fmt --all -- --check
cargo clippy -p reticulum-rs-transport --lib --all-features --no-deps -- -D warnings
cargo test -p reticulum-rs-transport --all-features --lib  # 820 passed
cargo test -p reticulumd --bin reticulumd --all-features  # 466 passed
cargo test -p reticulumd --test code_quality_issue_369  # 1 passed
cargo test -p reticulumd --test python_channel_interop --no-run
```

The existing raw Rust/Python resource round trips passed in the debug
profile. The size matrix passed in the release profile in both directions:

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test --release -p reticulumd --test python_channel_interop \
  rust_to_python_resource_size_matrix_roundtrip -- --ignored --nocapture
  # 1 passed; includes the exact 50 MiB SHA-256 check

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test --release -p reticulumd --test python_channel_interop \
  python_to_rust_resource_size_matrix_roundtrip -- --ignored --nocapture
  # 1 passed; includes the exact 50 MiB SHA-256 check

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  rust_reader_to_python_split_resource_roundtrip -- --ignored --nocapture
  # 1 passed; split reader-backed transfer and exact SHA-256 acknowledgement

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  rust_sender_observes_pinned_python_receiver_cancellation -- --ignored --nocapture
  # 1 passed; Rust observes one outbound cancellation

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  rust_receiver_reports_pinned_python_sender_cancellation -- --ignored --nocapture
  # 1 passed; Rust reports remote_cancelled and Python reports FAILED

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  python_to_python_resource_roundtrip_through_rust_transport -- --ignored --nocapture
  # 1 passed; split Resource crosses two Python endpoints over one forwarding
  # Rust transport; client verifies digest and metadata-bearing size/metadata ack

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_fault_matrix -- --ignored --nocapture
  # 1 passed; 35 filtered out; 69.31s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_reverse_fault_matrix -- --ignored --nocapture
  # 1 passed; 36 filtered out; 21.82s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  rust_sender_reports_pinned_python_receiver_shutdown -- --ignored --nocapture
  # 1 passed; 37 filtered out; 15.52s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  pinned_python_reader_resource_fault_matrix -- --ignored --nocapture
  # 1 passed; 39 filtered out; 21.93s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  rust_reader_reports_pinned_python_reader_failure -- --ignored --nocapture
  # 1 passed; 39 filtered out; 0.91s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test --release -p reticulumd --test python_channel_interop \
  rust_file_reader_reports_pinned_python_file_truncation -- --ignored --nocapture --test-threads=1
  # 1 passed; 46 filtered out; 0.60s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test --release -p reticulumd --test python_channel_interop \
  rust_receiver_reports_pinned_python_file_reader_failure -- --ignored --nocapture --test-threads=1
  # 1 passed; 47 filtered out; 13.02s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  pinned_python_link_timeout_and_reconnect_after_dropped_keepalives -- --ignored --nocapture
  # 1 passed; 45 filtered out; 15.38s
```

The fault matrix runs through a production TCP carrier and inspects only
decoded HDLC frames whose packet context is `Resource`; announces, link setup,
proofs, requests, and other control frames remain unmodified. In the
Python-sender → Rust-receiver direction, one dropped segment, one duplicate,
and a two-segment reorder all recovered to a complete resource. Dropping all
Resource data segments produced a terminal Rust inbound failure instead of a
false completion. This is a pinned-Python fault trace in one direction, not
yet a link-timeout-specific trace.

The reverse Rust-sender → Python-receiver matrix uses the same real carrier
proxy and a 70,000-byte multi-fragment transfer. It passes the same
drop/duplicate/reorder cases and observes Rust `OutboundFailed` when all
Resource data frames are missing. Together, the two matrices prove the
fragment-fault behaviors in both implementation directions; they do not by
themselves prove the separate reader-adapter, memory, or hosted acceptance
contracts. The shutdown trace separately proves that an admitted split Resource
is failed when its pinned Python receiver process exits, and the keepalive trace
now proves terminal link timeout followed by a fresh Link with a different
identifier on the same pinned carrier.
The reader-backed matrix uses the same transfer sizes and fault proxy as the
owned-buffer reverse matrix, while the reader-failure trace proves a later
source error becomes a terminal Rust failure after Python has accepted the
transfer. Commit `e9087c0c54016f5c2c88873a9cac2704e49db921` adds the real-file
truncation trace, which exercises the file-backed adapter itself. Commit
`8b29132c` adds the reciprocal pinned-Python file-like-reader fault: the
reference raises from its background segment preparation, the Rust receiver
reports terminal failure, and the sender exits unsuccessfully after its
bounded timeout.

The release runs completed in approximately 10.00 seconds and 11.53 seconds.
The debug Rust-to-Python 50 MiB preparation run exceeded the 180-second
interactive limit while the smaller cases passed; the same protocol and
checksum completed in the optimized release profile. This is recorded as a
profile/environment limitation, not as a passing debug large-transfer claim.

The bounded pinned independent-peer PR profile also completed against
`rns-rs` `6c6d79b83516feff271d15c97d39dd1de7798afe`:

```text
python3 tools/scripts/independent_interop.py --peer rns-rs --level pr \
  --output target/interop/independent/issue-605-pr --keep
# report: target/interop/independent/issue-605-pr/independent-interop.json
# 79 PASS; 3 peer-owned FAIL; 2 dependent BLOCKED
python3 tools/scripts/independent_interop_gate.py \
  target/interop/independent/issue-605-pr/independent-interop.json
# independent interop gate: PASS
```

That independent report includes exact-checksum 1 MiB Resource transfers in
direct and multi-hop topologies, shared-instance traffic before and after
daemon restart, deterministic 1% frame-loss recovery, complete-loss terminal
timeout, and 50 ms per-frame latency. Its peer-owned failures remain in the
report rather than being treated as Rust passes; this is independent `rns-rs`
evidence, not a substitute for the pinned Python fault matrix.

## Current pinned-Python refresh

The following runs were repeated at exact checkout
`099227cce9c7d6bd55f66acf88516b9293a7e1a1`. The source under test is
unchanged from `6e5b1a4865594432a7fd0405fbaf800e71481cc1`; the intervening
commit only refreshes parity evidence.

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  rust_reader_to_python_split_resource_roundtrip -- --ignored --nocapture \
  --test-threads=1
# 1 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out; 1.32s

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop pinned_python_link_ \
  -- --ignored --nocapture --test-threads=1
# 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; 19.51s
```

The current split reader trace again transfers a Resource over the real Rust
carrier path and verifies the exact remote SHA-256 acknowledgement. The new
file-reader-failure trace uses a pinned Python file-like adapter whose
mid-transfer `read()` raises; it records the reference sender's background
exception and bounded timeout alongside Rust's terminal inbound failure. The
timeout pair supplies current terminal link-establishment and keepalive-watchdog
evidence adjacent to the Resource fault matrix. These runs do not add every
consumer callback/status assertion or hosted/physical/soak evidence.

The current candidate adds a Linux `/proc` high-water RSS probe at
`e0d7249035a51b668ec88b9ce193b3fe0f3fc8e7`. It uses an exact 50 MiB transfer
and a 512 MiB per-process release-profile budget; the Rust sender uses the
reader-backed API and the Python sender uses the real Python Resource client.
Both directions passed against the pinned Python checkout:

```text
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test --release -p reticulumd --test python_channel_interop \
  rust_reader_to_python_50_mib_peak_memory -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; 0 ignored; 0 measured; 45 filtered out; 7.89s
# Rust reader -> Python receiver: 20,744 KiB / 100,292 KiB peak RSS
# SHA-256: eae47d4d847479acfbdf72c2c26ff457e57a377c4c5a7f2ee756510541c8026f

RETICULUM_PY_REPO=.tmp/python-refs/Reticulum LXMF_PYTHON_BIN=python3 \
  cargo test --release -p reticulumd --test python_channel_interop \
  python_to_rust_50_mib_peak_memory -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; 0 ignored; 0 measured; 45 filtered out; 10.24s
# Python sender -> Rust receiver: 242,616 KiB / 110,904 KiB peak RSS
# SHA-256: d1833c62bbdb9e3467d20466e31b616385c3d97484cc1737ac68fbbf44595554
```

These are measured process high-water values for the two exact 50 MiB
directions, not a claim that every fault or hosted workload has the same
profile. The values are now evidence for the mixed-peer bounded-memory row;
the remaining acceptance gaps stay explicit below. Commit
`7e310afb878f2230ef46c40b54c0112755397428` extends the same pinned-Python
keepalive fault trace from terminal watchdog closure to a fresh Link with a
different identifier; the targeted release run passed in 15.38 seconds.

## Exact 50 MiB rerun on PR #630 head

Both release-profile pinned-Python directions were rerun on the exact current
PR #630 head `0d9b5dd6ee87b0529b37e4ec4f40f14d74faffbe`, against Reticulum
`99de23c040d507e3fefca19e87b182302902725d`. Each test transferred exactly
52,428,800 bytes, matched the receiver's SHA-256, and stayed below the
524,288 KiB per-process peak-RSS budget:

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PYTHON_BIN=python3 cargo test --release -p reticulumd \
  --test python_channel_interop rust_reader_to_python_50_mib_peak_memory \
  -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; 7.79s
# Rust sender peak RSS: 20,504 KiB; Python receiver peak RSS: 105,328 KiB
# SHA-256: eae47d4d847479acfbdf72c2c26ff457e57a377c4c5a7f2ee756510541c8026f

RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PYTHON_BIN=python3 cargo test --release -p reticulumd \
  --test python_channel_interop python_to_rust_50_mib_peak_memory \
  -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; 11.76s
# Python sender peak RSS: 249,400 KiB; Rust receiver peak RSS: 110,432 KiB
# SHA-256: d1833c62bbdb9e3467d20466e31b616385c3d97484cc1737ac68fbb44595554
```

This refreshes the exact-checksum and bounded-memory evidence at the current
PR head; it does not close the remaining broader timeout/reconnect or
consumer callback/status requirements.

## Resource recovery over a timed-out Link

On the current #610 PR candidate, the ignored pinned-Python test
`pinned_python_link_timeout_and_reconnect_after_dropped_keepalives` now drops
Resource traffic and keepalives together while a 70,000-byte Rust Resource is
in flight. The Rust Link reaches terminal `Closed`, and the transport emits
the specific `OutboundFailed` terminal event for that Resource. The proxy then
restores forwarding; a newly established Link has a different identifier and
carries a second 70,000-byte Rust Resource to Python. The transport emits
`OutboundComplete`, and the Python endpoint acknowledges the exact byte count
and SHA-256 digest.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd \
  --test python_channel_interop \
  pinned_python_link_timeout_and_reconnect_after_dropped_keepalives \
  -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; in-flight failure and fresh-Link Resource digest verified
```

This adds one concrete Rust-initiated Resource timeout/recovery trace, not a
complete timeout matrix.

The reciprocal recovery path is now exercised by the ignored pinned-Python test
`pinned_python_initiated_link_timeout_fails_then_recovers_inbound_resource`.
Python initiates a Link and sends a 70,000-byte Resource through a fault proxy.
The proxy allows Resource requests but drops Resource parts and keepalives.
Rust observes an inbound Resource failure with a non-empty reason, and the
same inbound Link then emits `Closed`. The test restores forwarding through a
fresh proxy, starts another Python sender, verifies a new Link identifier, and
compares the recovered Rust Resource's exact size and SHA-256 with the Python
sender's completion report.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd \
  --test python_channel_interop \
  pinned_python_initiated_link_timeout_fails_then_recovers_inbound_resource \
  -- --ignored --nocapture --test-threads=1
# 1 passed; 0 failed; reverse-role terminal failure and fresh-Link recovery verified
```

Both Rust-initiated and Python-initiated timeout/recovery traces now run in the
PR `Verify` workflow against the frozen Reticulum target. This makes the two
recovery directions a required hosted software check; it does not replace the
broader timeout matrix or callback/status assertions across every consumer.

## Daemon Resource completion and failure receipts

The `reticulumd` outbound Resource completion consumer now has a focused unit
regression, `outbound_resource_completion_event_records_receipt_and_peer_bytes`.
It verifies one `resource-complete` receipt with the original message ID,
Resource hash, peer, byte count, and non-terminal `sent: link resource` status;
it also verifies that a duplicate completion notification emits no second
receipt or byte-accounting update, and that Resource tracking is removed.
This is a transport-completion receipt, not a remote LXMF delivery
acknowledgement, and does not stand in for the remaining consumer callback
matrix.

The companion `outbound_resource_failure_event_marks_tracking_failed`
regression verifies one `resource-failed` receipt with the original message ID,
Resource hash, peer, byte count, and `failed: resource transfer timed out`
status. A repeated failure notification emits no duplicate receipt; tracking
is removed, transmitted-byte accounting is retained, and the peer is marked
inactive with the expected backoff. Other failure and consumer paths remain
outside this focused regression.

The daemon's inbound Resource event consumer previously discarded every
`Progress` event even though `ResourceManager` emitted received/total byte and
part counts. It now writes those counts at debug level with the Resource hash
and Link ID. `inbound_resource_progress_status_preserves_bytes_and_parts`
checks that the consumer's status representation retains all four counters
and both correlation identifiers. This makes intermediate receive progress
observable to daemon operators when debug logging is enabled; it does not
complete the broader callback/status/cleanup acceptance matrix.

```text
cargo test -p reticulumd --bin reticulumd \
  outbound_resource_completion_event_records_receipt_and_peer_bytes
# 1 passed; 466 filtered out
cargo test -p reticulumd --bin reticulumd \
  outbound_resource_failure_event_marks_tracking_failed
# 1 passed; 466 filtered out
cargo test -p reticulumd --bin reticulumd \
  inbound_resource_progress_status_preserves_bytes_and_parts
# 1 passed; progress counters and event correlation fields preserved
```

## Python RCL/ICL terminal-event distinction

The 2026-09-23 candidate aligns remote cancellation with the pinned Python
`RNS/Link.py` context routing. `RESOURCE_ICL` applies only to an incoming
Resource and reports `InboundFailed(reason=remote_cancelled)`;
`RESOURCE_RCL` applies only to an outgoing Resource and reports
`OutboundRejected`. The Rust-local outgoing cancel API continues to report
`OutboundCancelled`. This prevents a peer rejection from being mislabeled as a
local cancellation or from clearing a Resource in the wrong direction.

The `OutboundRejected` event is propagated as a distinct SDK transport error,
remote-control error, daemon `resource-rejected` terminal receipt with tracking
cleanup, `rncp` client/server failure, and independent-interop event. A pinned
Python receiver exercises the RCL path over the real interop harness; focused
unit tests cover context isolation, split-tail cleanup, and the preserved local
cancel event. The independent `rns-rs` PR probe names this peer outcome
`Resource rejection` and requires `outbound_rejected`; `outbound_cancelled`
remains reserved for local cancellation.

```text
cargo test -p reticulum-rs-transport --all-features --lib  # 828 passed
cargo test -p lxmf-runtime  # 15 passed
cargo test -p reticulumd --bin reticulumd --all-features  # 470 passed
cargo test -p reticulumd --test python_channel_interop --no-run
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd \
  --test python_channel_interop \
  rust_sender_maps_pinned_python_receiver_cancel_to_rejection \
  -- --ignored --nocapture --test-threads=1
# 1 passed; pinned Python receiver RCL maps to Rust OutboundRejected
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum-99de23c LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  rust_receiver_reports_pinned_python_cancel_on_second_resource_segment \
  -- --ignored --nocapture --test-threads=1
# 1 passed; cancellation after data in segment 2, Rust remote_cancelled and
# Python FAILED callback both observed
cargo check -p rns-tools --all-targets --all-features
cargo clippy -p reticulum-rs-transport --all-targets --all-features --no-deps -- -D warnings
cargo clippy -p lxmf-runtime --all-targets --all-features --no-deps -- -D warnings
cargo clippy -p reticulumd --bin reticulumd --all-targets --all-features --no-deps -- -D warnings
cargo clippy -p rns-tools --all-targets --all-features --no-deps -- -D warnings
tools/scripts/check-module-size.sh
tools/scripts/check-boundaries.sh  # passes; two existing legacy-boundary notices
python3 tools/scripts/python_surface_inventory.py --check
cargo fmt --all -- --check
git diff --check
```

This is a focused terminal-event fidelity increment, not completion of the
broader timeout, callback/status, mixed-peer size, or operational acceptance
matrix.

## Split Resource metadata placement and size accounting

The 2026-09-23 focused trace verifies the metadata-bearing split transfer in
both directions over production Link and Resource paths against pinned
Reticulum `99de23c040d507e3fefca19e87b182302902725d`. Rust's reader-backed
sender is received by Python with the exact decoded metadata and payload
digest; the Python receiver also reports `total_size` equal to payload bytes
plus the encoded metadata block and its single 3-byte length prefix. In the
reverse direction, the pinned Python sender's split transfer is assembled by
Rust with the expected data length and decoded metadata, while each accepted
segment reports a `total_data_size` equal to that same accounting formula.
The reverse trace repeats this assertion under first-part loss, duplication,
and reordering. No behavioral mismatch was found, so this increment changes
interop assertions and evidence only; it does not alter Resource production
code or claim general large-resource parity.

```text
cargo test -p reticulum-rs-transport resource_receiver_strips_split_metadata_from_the_first_segment_only
# 1 passed
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  rust_reader_to_python_split_resource_roundtrip -- --ignored --nocapture
# 1 passed; exact Python metadata, data digest, and total_size
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_fault_matrix -- --ignored --nocapture
# 1 passed; reverse split metadata and total_data_size under loss, duplication,
# reordering, plus the existing all-parts-missing terminal-failure case
```

## Default Resource compression against pinned Python

At the current PR #630 candidate, `rust_resource_compression_defaults_match_pinned_python`
uses the production TCP/Link/Resource path in both directions. Rust-to-Python
checks default compression of compressible bytes, default handling of
deterministic incompressible bytes, and explicit `auto_compress=false`; the
Python receiver confirms the advertised `compressed` flag and exact received
length/SHA-256. Python-to-Rust repeats those three cases with the Python
`Resource(auto_compress=True)` default and explicit disabled mode; Rust verifies
the exact received length and its digest matches the Python sender's digest.
Both sides report `true, false, false` for the cases. No production mismatch
was demonstrated, so this increment is regression/evidence only.

The Rust-to-Python defaults case additionally sends a compressible Resource
with MessagePack metadata. The pinned Python receiver recovered the exact
content and metadata, report `compressed=true`, and report `total_size` as the
uncompressed content length plus the three-byte metadata-length prefix and
encoded metadata. This covers the composition of compression and metadata
accounting; it does not close the broader #610 acceptance matrix.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  rust_resource_compression_defaults_match_pinned_python \
  -- --ignored --nocapture --test-threads=1
# 1 passed; Python observed compressed=true, false, false for the three cases,
# including compressed metadata and exact total_size accounting
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_compression_defaults_match_rust \
  -- --ignored --nocapture --test-threads=1
# 1 passed; Rust received and verified Python's exact payloads/digests and flags
```

The distinct 64 MiB-plus-one edge was subsequently probed and corrected in
the PR #638 follow-up recorded below. The remaining #610 transfer-selection
and failure matrix remains open.

### Compression selection above the efficient-segment boundary

The production Rust-to-pinned-Python and pinned-Python-to-Rust compression
differential now sends compressible and deterministic incompressible payloads
of exactly `2 * MAX_EFFICIENT_SIZE`. Both cross the split boundary into two
full segments (Python-to-Rust also includes its metadata wire prefix).
The pinned `RNS/Resource.py` sender first selects the segment using the
uncompressed `MAX_EFFICIENT_SIZE` accounting, then applies bz2 independently
to each segment (subject to the 64 MiB whole-resource cap), setting
`compressed` only when that segment shrinks. Rust's production sender does the
same: segment accounting precedes per-segment compression. The receivers
verify the assembled payload digest and logical `total_size`; sender-side
compression choices are `true` for repeating bytes and `false` for the
deterministic incompressible payload. This closes only the above-
`MAX_EFFICIENT_SIZE` selection gap, not the broader #610 contract.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  rust_resource_compression_defaults_match_pinned_python \
  -- --ignored --nocapture --test-threads=1
# 1 passed; Rust-to-Python split cases matched compression flags, size, and digest
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_compression_defaults_match_rust \
  -- --ignored --nocapture --test-threads=1
# 1 passed; Python-to-Rust split cases matched sender flags, logical size, and digest
```

### Split compression with first-segment metadata

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`
`RNS/Resource.py::Resource.__init__` prepends encoded metadata to segment 1
before compression. The mixed-peer compression matrix now includes a
compressible 2 MiB source with MessagePack metadata and verifies the assembled
Python receiver's exact data digest, metadata, and total-size accounting. The
metadata prefix changes this transfer to three segments; Python's reported
compression flag belongs to the final 15-byte tail and is correctly false
because bzip2 framing would expand it. A focused Rust preparation test decrypts
the first-segment advertisement and confirms that metadata-bearing segment is
compressed. This closes only the combined split/metadata/compression case; the
broad #610 acceptance remains partial.

```text
cargo test -p reticulum-rs-transport --lib \
  split_resource_compresses_first_segment_with_metadata -- --nocapture
# 1 passed; first-segment advertisement is compressed and transfer has 3 segments

TMPDIR=/dev/shm \
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  rust_resource_compression_defaults_match_pinned_python \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed; Python assembled exact data/metadata/total size; final tail segment
# correctly reported uncompressed
```

## Pinned-Python cancellation after the first split segment

The Rust sender now has a later-segment cancellation regression against frozen
Reticulum `99de23c040d507e3fefca19e87b182302902725d`. Python accepts the first
part of a split Resource, then cancels while the second segment is in flight;
Rust observes the terminal `OutboundRejected`. This proves cancellation is
handled after transfer progress, rather than only before or during the first
part. It is one focused fault trace, not the complete segment/callback matrix.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  rust_sender_observes_pinned_python_cancel_on_second_resource_segment \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed
```

## Missing-fragment retry exhaustion after partial progress

Both production Rust and frozen Python now have executable regressions for
this transition. The Rust `ResourceManager` test accepts the first of two
advertised parts, leaves the second absent, advances its injected clock past
the configured retry interval, and verifies the exact terminal
`retry_limit_exhausted` failure with one received part and no retained inbound
state.

The ignored pinned-Python test invokes the reference
`Resource._Resource__watchdog_job` directly on a two-part `Resource.__new__`
fixture with one received part, `retries_left == 0`, and an expired missing-part
deadline. In a separate Python subprocess it replaces the module's clock and
sleep functions, does not start the watchdog thread, and uses an inactive stub
Link so the real `Resource.cancel()` takes its receiver cleanup path without
network I/O. It asserts exactly one cancel from `TRANSFERRING`, terminal
`FAILED`, removal from the Link's incoming-resource list, one fake clock read,
and only the no-op post-transition sleep request. This is a deterministic
state-machine differential, not an end-to-end network timeout test. Cross-peer
timeout timing and the rest of the #610 failure matrix remain open.

Hosted lane ownership is intentionally split by reference pin. The generic
`python-channel-interop` HIL case inherits canonical Reticulum 1.5.2
(`ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`) and skips only this regression.
The exact ignored test is instead run by Verify's dedicated
`Verify frozen 1.5.4 missing-part Resource retry exhaustion` step with
`RETICULUM_PY_REPO` set to `Reticulum-parity`, checked out at
`99de23c040d507e3fefca19e87b182302902725d`. Its in-test revision assertion
remains enabled; the canonical 1.5.2 checkout and unrelated interop cases are
unchanged.

```text
cargo test -p reticulum-rs-transport --lib \
  resource_manager_exhausts_missing_fragment_retries_after_partial_progress
# 1 passed

RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_cancels_after_missing_part_retry_budget \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed against frozen Reticulum 99de23c040d507e3fefca19e87b182302902725d
```

## Resource compression size-limit boundary

The focused 2026-09-23 differential checks the inclusive compression size
limit through production Link/Resource transfers in both directions. Frozen
Reticulum `99de23c040d507e3fefca19e87b182302902725d`
(`RNS/Resource.py::Resource.__init__`) sets the default limit to 64 MiB,
attempts `bz2.compress` when `data_size <= auto_compress_limit`, and sets the
compressed flag only when the result is smaller. Rust and pinned Python both
compressed the deterministic 64 MiB repeating-byte payload. Each receiver
reported logical/accounted size 67,108,864 and the exact SHA-256 of the
uncompressed bytes. Rust also rejects a 64 MiB + 1 send before advertisement,
matching its production Resource admission limit. Since that rejection
prevents an above-limit Rust-to-Python transfer, the reference's uncompressed
decision specifically above the 64 MiB compression cap is not observed end to
end. This separate cap-edge evidence remains open.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  resource_compression -- --ignored --nocapture --test-threads=1
# 4 passed; threshold boundary verified in both directions, default cases retained
```

## First-segment metadata boundary

The focused `pinned_python_resource_metadata_boundary_matches_rust_accounting`
case places the Python MessagePack metadata block across the first segment
boundary. The deterministic payload size is `MAX_EFFICIENT_SIZE -
metadata_wire_size + 1`, so pinned Python reports `total_size =
MAX_EFFICIENT_SIZE + 1` and exactly two segments. Rust observes accepted
segment indexes `[1, 2]`, reassembles the exact encoded `python-meta` value,
and its content SHA-256 matches the pinned Python sender's report. This
confirms the receiver's boundary accounting and reassembly. It does not by
itself inspect the transmitted first-segment payload bytes or prove their
metadata placement. It is a focused mixed-peer boundary case; it does not
close broader size/chunking or request/response selection.

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_metadata_boundary_matches_rust_accounting \
  -- --ignored --nocapture --test-threads=1
# 1 passed; two exact segment indexes, metadata, total size, and content digest
```

### Rust-to-Python first-segment metadata bytes

`rust_to_python_split_resource_first_segment_metadata_wire_matches_reference`
uses the production Rust `Transport::send_resource_with_compression` path and
the pinned Python Link/Resource receiver over loopback. Its test-only Python
wrapper scopes capture to `Resource.assemble` and observes the argument to
`Identity.full_hash` immediately before the pinned implementation parses and
strips metadata. It removes only the trailing four-byte Resource random hash
from that hash input and records each decoded/decompressed segment payload;
the pinned checkout itself is not modified.

Rust sends an uncompressed two-segment Resource with MessagePack metadata.
The test verifies both segment captures and checks the complete first-segment
payload SHA-256 against the expected three-byte metadata length, encoded
metadata, and first data slice. It also compares the exact metadata-prefix
bytes, the immediately following 32 content bytes, and the completed
Resource's digest/metadata. This observes the Resource segment payload after
packet reassembly and Link decryption (and after decompression, disabled for
this case), not encrypted Link packets or individual Resource part packets.
No production protocol behavior was changed.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  rust_to_python_split_resource_first_segment_metadata_wire_matches_reference \
  -- --ignored --exact --nocapture --test-threads=1
# 1 passed against frozen Reticulum 99de23c040d507e3fefca19e87b182302902725d
```

The same exact ignored test is wired into Verify's pinned-1.5.4 job as
“Verify frozen 1.5.4 first-segment Resource metadata wire placement.”

## Link request/response packet-versus-Resource selection

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`
`RNS/Link.py::handle_request` defines a deterministic ordinary-response
boundary: pack `[request_id, response]`; send one `Response` packet when the
packed envelope length is `<= link.mdu`, otherwise send it as a response
Resource. A file-handle response is always a metadata-bearing Resource,
independent of its size. The Resource transfer matrix does not establish this
Link-level selection behavior.

Rust's `Transport::send_response` applies that rule using the negotiated Link
MDU, returns `None` for a sent packet and the Resource hash for a selected
Resource, and propagates packet-send and Resource-send errors. Mixed-peer
request/response cases exercise production Links in both directions, including
ordinary small and oversized responses and a metadata-bearing Python file
response. The exact inclusive boundary is covered separately below with
production peer sessions at `mdu - 1`, `mdu`, and `mdu + 1`.

```text
TMPDIR=/dev/shm \
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs/.tmp/python-refs/Reticulum-99de23c \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  request_response -- --ignored --nocapture --test-threads=1
# 8 passed; packet and Resource response paths in both directions, including
# the production MDU-1 / MDU / MDU+1 selection boundary

RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  rust_to_python_file_response_resource_roundtrip \
  -- --ignored --nocapture --test-threads=1
# 1 passed; exact file bytes and decoded metadata
```

### Exact negotiated-MDU boundary (production mixed-peer sessions)

Three isolated one-request Python client processes connect sequentially to one
Rust TCP service. After each Link negotiates, the Python client sizes its
request so the Rust response envelope `[request_id, response]`, encoded with
MessagePack, is exactly `MDU-1`, `MDU`, or `MDU+1`. Rust verifies the packed
envelope size against the Python peer's reported negotiated MDU and asserts
packet selection for the first two cases and a Resource hash for the last.
The Python request is Resource-backed due to its size; the response is
observed by the Python request callback, and outbound Resource completion is
required for `MDU+1`. All three responses match exact bytes, UTF-8 size, and
SHA-256.

This matches pinned Reticulum
`99de23c040d507e3fefca19e87b182302902725d`, `RNS/Link.py::handle_request`:
ordinary responses use a packet when packed response length is `<= mdu`, and
a response Resource above it. These are three one-request peer sessions; no
multi-hop HIL job was run.

```text
TMPDIR=/dev/shm \
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs/.tmp/python-refs/Reticulum-99de23c \
  LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  python_to_rust_request_response_matches_exact_negotiated_mdu_boundary \
  -- --ignored --nocapture --test-threads=1
# 1 passed; three sessions at negotiated MDU-1, MDU, MDU+1
```

### Link-establishment timeout recovery after a dropped request

The pinned-Python regression
`pinned_python_link_establishment_timeout_after_dropped_request` drops the
initial Link request, verifies the Rust Link reaches its establishment timeout,
restores the same carrier, and requires a fresh Link ID before sending a
256-byte one-part Resource from Rust to Python. The Python acknowledgement
matches the expected digest. This verifies one production recovery path; it
does not establish split-Resource timeout recovery or the broader #610 failure
matrix.

```text
TMPDIR=/dev/shm \
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs/.tmp/python-refs/Reticulum-99de23c \
LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  pinned_python_link_establishment_timeout_after_dropped_request \
  -- --ignored --nocapture --test-threads=1
# 1 passed; fresh production Link and matching 256-byte Resource digest
```

## 64 MiB outbound admission versus compression boundary

Pinned Reticulum `99de23c040d507e3fefca19e87b182302902725d`
`RNS/Resource.py::Resource.__init__` treats 64 MiB as the automatic
compression threshold, not as an outbound admission limit. A sparse-file
probe ran the production constructor with `advertise=False` and a synthetic
large-MDU Link. It built only the first segment for each case, without
materializing a 64 MiB payload or sending fragments:

```text
size=67108864  total_size=67108864  segments=65  compressed=true
status=NONE  advertised=false  parts_built=1
size=67108865  total_size=67108865  segments=65  compressed=false
status=NONE  advertised=false  parts_built=1
```

The initial Rust preparation regression expected 64 MiB + 1 to fail. The
production change below supersedes that assertion: outbound reader and
byte-slice senders now admit known-size sources beyond the compression
threshold, disable automatic compression based on the full source size, and
retain lazy segmentation. The existing pinned-Python probe suppresses network
advertisement; no full mixed-peer 64 MiB + 1 transfer is claimed.

```text
RETICULUM_PY_REPO=/home/pgiuseppe/Documents/LXMF-rs-issue-605/.tmp/python-refs/Reticulum \
  LXMF_PYTHON_BIN=python3 \
  cargo test -p reticulumd --test python_channel_interop \
  pinned_python_resource_compression_cap_boundary_probe \
  -- --ignored --nocapture --test-threads=1
# 1 passed; pinned Python production Resource constructor, sparse source,
# first segment only, no advertisement

cargo test -p reticulum-rs-transport --lib \
  compression_threshold_does_not_cap_outbound_resource_admission \
  -- --nocapture
# 1 passed; exact-cap and +1 production reader advertisements

cargo test -p reticulum-rs-transport --lib \
  reader_backed_send_admits_one_byte_above_compression_threshold_lazily \
  -- --nocapture
# 1 passed; exact logical size/segment count, uncompressed, first segment only

cargo test -p reticulum-rs-transport --lib resource::tests
# 75 passed; focused Resource unit suite, including lazy 64 MiB + 1 source

cargo fmt --all -- --check
# passed

cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
# passed

tools/scripts/check-module-size.sh
# module-size checks: ok

TMPDIR=/home/pgiuseppe/.codex/worktrees/issue-610-resource-metadata-ack/target \
  tools/scripts/check-boundaries.sh
# boundary checks: ok (two existing legacy-boundary notices); /tmp was over
# its per-user quota, so the script's temporary metadata file used target/

git diff --check
# passed
```

## Outbound rejection and cancellation through the daemon consumer

The focused daemon regressions `production_daemon_consumer_persists_peer_resource_rejection`
and `production_daemon_consumer_cleans_cancelled_resource_without_success_receipt`
exercise `spawn_inbound_worker` with events emitted by a production `Transport`
over paired in-memory interfaces and an active Link; they do not call the
private terminal-event handlers directly.

- For rejection, the peer accepts the Resource advertisement and holds its
  first request. It sends a Link-encrypted `ResourceReceiverCancel` packet
  through its public `Transport::send_packet` path. The daemon consumer emits
  one `rejected` receipt, clears Resource tracking, and the existing receipt
  persistence consumer stores `rejected` as the message status.
- For local cancellation, the public `sdk_cancel_message_v2` RPC first records
  `cancelled`; public `Transport::cancel_resource` then emits the real
  `OutboundCancelled` event. The daemon consumer clears tracking, leaves the
  status `cancelled`, and emits no success receipt.

Validation on the candidate worktree: `reticulumd` binary tests 474/474,
`reticulum-rs-transport` library tests 834/834, strict Clippy for both packages,
formatting, module-size, and diff checks passed. The boundary script could not
evaluate its allowlists: the unchanged `load_allowlisted_edges` jq call refers
to `$key` without passing `--arg key`, so the configured allowlists appear empty.
No dependency manifests changed in this slice.

This adds production-daemon consumer evidence for outbound rejection and local
cancellation only. It does not establish the full callback/status/cleanup
matrix or complete issue #610.

## Outbound timeout through the production daemon consumer

`production_daemon_consumer_reports_resource_timeout_and_cleans_tracking` uses
the same production daemon worker and active in-memory Link, with a one-second
Resource retry interval and one-retry budget. The peer receives and holds the
Resource request without returning fragments, deterministically driving the
sender to its timeout event. The consumer emits a `resource-failed` receipt
with the exact message ID and resource hash, clears the tracking entry, and
persists `failed: resource transfer timed out`; no completion is reported.
This closes the prior test-layer gap between the direct timeout-handler unit
test and the live daemon event consumer. No production mismatch was observed;
broader terminal-event and reconnect coverage remains open.

Focused validation:

```text
cargo test -p reticulumd --bin reticulumd \
  production_daemon_consumer_reports_resource_timeout_and_cleans_tracking \
  -- --nocapture
# 1 passed
```

## Deterministic partial inbound failure through the daemon consumer

`daemon_reports_partial_inbound_resource_failure_after_gated_link_teardown`
uses the production `Transport`, active Link, and `spawn_inbound_worker` with a
test-only gate on the paired in-memory interface channels. The gate forwards
the Resource advertisement and first Resource data packet, then withholds later
Resource packets while continuing to forward control packets. The test waits
for the receiver's actual nonzero/incomplete `Progress` event and for a later
Resource packet to reach the gate before the peer sends LinkClose. No sleep or
packet-arrival race determines the injection point.

The daemon transport emits exactly one `InboundFailed` for the transfer with
nonzero but incomplete part counts; it emits no `Complete`. The daemon worker
produces no receipt. An existing `sending: link resource` record keeps that
status and its empty content, with no delivered message added to the store.
The identical payload then completes with exact bytes on a fresh Link, showing
that a follow-up receive succeeds after the failed Link is removed. This is
focused proof for one partial-transfer Link teardown path; it does not establish
every Resource failure reason, callback/status consumer, or the broader #610
acceptance matrix.

Focused validation on the #638 candidate worktree:

```text
cargo test -p reticulumd --bin reticulumd \
  daemon_reports_partial_inbound_resource_failure_after_gated_link_teardown \
  -- --nocapture
# 1 passed

TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd
# 473 passed

cargo clippy -p reticulumd --bin reticulumd --tests --no-deps -- -D warnings
cargo fmt --all -- --check
TMPDIR=/dev/shm tools/scripts/check-module-size.sh
git diff --check
# all passed
```

## Peer cancellation of a partial inbound Resource through the daemon consumer

At PR #638 candidate `16a232b86bd326291f354b8b6ae1c036b47c7982`,
`daemon_observes_remote_cancel_of_partial_inbound_resource_without_false_delivery`
uses the production `Transport`, active Link, and `spawn_inbound_worker`. A
test-only interface gate forwards the advertisement and first Resource part,
holds a later data packet, then allows the peer's public
`Transport::cancel_resource` call to send its initiator-cancel control packet.
The daemon observes a correlated `InboundFailed(reason=remote_cancelled)` with
nonzero but incomplete progress, and no `Complete`. The existing message keeps
its pre-transfer status and empty content; no receipt or delivered record is
created. A second Resource then completes with exact bytes over that same
still-active Link, demonstrating that cancellation removed the partial
receiver state without requiring Link teardown.

This matches pinned Reticulum revision
`99de23c040d507e3fefca19e87b182302902725d`:
`RNS/Resource.py:1090-1123` sets `FAILED`, removes the Resource through
`resource_concluded`, and calls the callback; `RNS/Link.py:1112-1119` routes
`RESOURCE_ICL` to the matching inbound Resource. It establishes this one
daemon-consumer cancellation path, not every callback/status consumer or the
unchecked compound acceptance item.

Focused validation on the isolated PR-head worktree:

```text
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd \
  daemon_observes_remote_cancel_of_partial_inbound_resource_without_false_delivery \
  -- --nocapture
# 1 passed; 474 filtered out
```

## Remaining acceptance boundary

The following #610 requirements remain unverified and are intentionally not
represented as complete:

- broader timeout/reconnect coverage beyond the reciprocal traces, while the
  two pinned-Python matrices cover loss, duplication, reordering, and complete
  missing-fragment terminal failure in both directions;
- callbacks/status transitions observed through every library and daemon
  consumer after each injected failure; outbound completion, timeout-failure,
  rejection, local cancellation, partial inbound teardown, and peer
  cancellation of a partial inbound Resource now have focused daemon-consumer
  regressions;
- hosted, physical-interface, public-network, and long-running soak evidence.

The current conclusion is therefore: collision regeneration, shutdown cleanup,
window-bounded fragment admission, deterministic local loss/duplication/
reordering recovery, split cancellation cleanup, bidirectional pinned-Python
cancellation terminal events, bidirectional pinned-Python loss/duplication/
reordering and missing-fragment terminal evidence, reader-backed bounded source
retention plus a pinned-Python split reader transfer, a two-carrier pinned-
Python split Resource forwarding trace with an exact remote callback digest,
bidirectional release-profile mixed-peer transfers, the pinned-Python
receiver-shutdown terminal-failure trace, and the independent `rns-rs`
loss/timeout/latency slice, and exact 50 MiB bidirectional peak-RSS evidence
under a fixed process budget, and the pinned-Python file-like-reader fault
trace are implemented with local evidence; the broader Resource failure
contract remains partial pending broader timeout/reconnect, every consumer
callback/status assertion, and hosted/physical/soak coverage. The Python reference reader
exception is observed as a background preparation failure followed by the
sender's bounded timeout, rather than an explicit Resource `FAILED` callback;
that reference behavior is retained in the evidence rather than normalized.
