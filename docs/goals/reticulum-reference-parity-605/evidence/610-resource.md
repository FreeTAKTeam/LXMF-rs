# Issue #610: Resource collision, shutdown, and mixed-peer evidence

Status: implemented but unproven. This is a forward-candidate software slice;
it does not promote the full #610 acceptance contract or close parent issue
#605.

## Reference and candidate

- Candidate branch: `codex/issue-605-parity`.
- Candidate commit: `fa84c13291d661486192227ca082a359a0d13a49`.
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
- The Python interop helper can send a deterministic file-backed resource and
  reports an exact SHA-256 digest. Release-profile mixed-peer tests cover
  empty, one-byte, just-below-efficient, split, and 50 MiB resources in both
  Rust-to-Python and Python-to-Rust directions.

## Local evidence

The following checks passed on the candidate:

```text
cargo fmt --all -- --check
cargo clippy -p reticulum-rs-transport --lib --all-features --no-deps -- -D warnings
cargo test -p reticulum-rs-transport --all-features --lib  # 810 passed
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
```

The release runs completed in approximately 10.00 seconds and 11.53 seconds.
The debug Rust-to-Python 50 MiB preparation run exceeded the 180-second
interactive limit while the smaller cases passed; the same protocol and
checksum completed in the optimized release profile. This is recorded as a
profile/environment limitation, not as a passing debug large-transfer claim.

## Remaining acceptance boundary

The following #610 requirements remain unverified and are intentionally not
represented as complete:

- deterministic fault-injection evidence for loss, duplication, reordering,
  missing fragments, cancellation at multiple segment positions, and link
  timeout recovery or terminal failure;
- peak-RSS measurements for the 50 MiB mixed-peer transfers and a bounded
  memory report across the full matrix;
- a Rust public reader/file adapter equivalent to the reference's stream/file
  sender surface, if that API is required by downstream consumers; the current
  matrix proves the Python file-backed sender path and the Rust owned-buffer
  path, not a new Rust reader API;
- callbacks/status transitions observed through every library and daemon
  consumer after each injected failure;
- hosted, physical-interface, public-network, and long-running soak evidence.

The current conclusion is therefore: collision regeneration, shutdown cleanup,
and bidirectional release-profile mixed-peer transfers are implemented with
local evidence; the broader Resource failure and bounded-memory contract
remains partial pending targeted fault-injection and resource-usage evidence.
