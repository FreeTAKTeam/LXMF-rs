# Issue #610: Resource collision, shutdown, and mixed-peer evidence

Status: implemented but unproven. This is a forward-candidate software slice;
it does not promote the full #610 acceptance contract or close parent issue
#605.

## Reference and candidate

- Candidate branch: `codex/issue-605-parity`.
- Historical evidence candidate: `4ebaf762236e03df8ae56fd55696bd51c2e3de46`.
- Current branch carrying this behavior: `53d0f14c`; later documentation
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
  exercises the Rust `send_resource_from_reader` API with a split
  `MAX_EFFICIENT_SIZE + 257` payload and verifies the exact cross-peer digest.
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
- The same real-carrier matrix now uses a Rust `Read + Send + Sync` source and
  covers dropped, duplicated, reordered, and completely missing Resource
  data. A separate reader-backed trace injects an error while building the
  second segment after the Python receiver has acknowledged the first.

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
themselves prove the separate link-timeout, reader-adapter, memory, or hosted
acceptance contracts. The shutdown trace separately proves that an admitted
split Resource is failed when its pinned Python receiver process exits.
The reader-backed matrix uses the same transfer sizes and fault proxy as the
owned-buffer reverse matrix, while the reader-failure trace proves a later
source error becomes a terminal Rust failure after Python has accepted the
transfer.

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

## Remaining acceptance boundary

The following #610 requirements remain unverified and are intentionally not
represented as complete:

- mixed-Python link-timeout recovery or terminal-failure evidence beyond the
  explicit peer-process shutdown trace; the two pinned-Python matrices now
  cover loss, duplication, reordering, and complete missing-fragment terminal
  failure in both directions;
- peak-RSS measurements for the 50 MiB mixed-peer transfers and a bounded
  memory report across the full matrix;
- mixed-Python fault-injection evidence for reader cancellation and a true
  file-backed adapter; reader-backed loss, duplication, reordering, and
  terminal reader failure are now covered, while those remaining adapter
  roles are open;
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
loss/timeout/latency slice are implemented with local evidence; the broader
Resource failure and bounded-memory contract remains partial pending
link-timeout and reader-adapter fault traces, resource-usage evidence, and
hosted/physical/soak coverage.
