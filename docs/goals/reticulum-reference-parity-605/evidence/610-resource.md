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
  a Python receiver cancels a Rust reader-backed split send and Rust emits one
  `OutboundCancelled` terminal event, while a Python sender cancels after
  advertisement and Rust emits `InboundFailed(reason=remote_cancelled)`.
  The Python sender also reports its own `FAILED` callback status.
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
  # 1 passed; split Resource crosses two Python endpoints over one forwarding Rust transport

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

## Remaining acceptance boundary

The following #610 requirements remain unverified and are intentionally not
represented as complete:

- broader timeout/reconnect coverage beyond the reciprocal traces, while the
  two pinned-Python matrices cover loss, duplication, reordering, and complete
  missing-fragment terminal failure in both directions;
- callbacks/status transitions observed through every library and daemon
  consumer after each injected failure;
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
