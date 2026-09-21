# Issue #610: Resource collision, shutdown, and mixed-peer evidence

Status: implemented but unproven. This is a forward-candidate software slice;
it does not promote the full #610 acceptance contract or close parent issue
#605.

## Reference and candidate

- Candidate branch: `codex/issue-605-parity`.
- Historical evidence candidate: `4ebaf762236e03df8ae56fd55696bd51c2e3de46`.
- Current branch carrying this behavior: `42ef3f29`; later documentation
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
  a Python receiver cancels a Rust split send and Rust emits one
  `OutboundCancelled` terminal event, while a Python sender cancels after
  advertisement and Rust emits `InboundFailed(reason=remote_cancelled)`.
  The Python sender also reports its own `FAILED` callback status.

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
```

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

- mixed-Python fault-injection evidence for loss, duplication, reordering,
  missing fragments, and link-timeout recovery or terminal failure; the new
  pinned-Python cancellation cases cover both terminal directions, while the
  full pinned-Python matrix and cross-implementation duplicate/reorder trace
  remain open;
- peak-RSS measurements for the 50 MiB mixed-peer transfers and a bounded
  memory report across the full matrix;
- mixed-Python fault-injection evidence for the reader/file adapter, including
  loss, duplication, reordering, cancellation, and terminal reader failure;
  the split reader-backed success path is now covered, while the failure
  matrix remains open;
- callbacks/status transitions observed through every library and daemon
  consumer after each injected failure;
- hosted, physical-interface, public-network, and long-running soak evidence.

The current conclusion is therefore: collision regeneration, shutdown cleanup,
window-bounded fragment admission, deterministic local loss/duplication/
reordering recovery, split cancellation cleanup, bidirectional pinned-Python
cancellation terminal events, reader-backed bounded source retention plus a
pinned-Python split reader transfer, a two-carrier pinned-Python split Resource
forwarding trace with an exact remote callback digest, bidirectional
release-profile mixed-peer transfers, and the independent `rns-rs`
loss/timeout/latency slice are implemented with local evidence; the broader
Resource failure and bounded-memory contract remains
partial pending the full pinned-Python fault matrix, resource-usage evidence,
and hosted/physical/soak coverage.
