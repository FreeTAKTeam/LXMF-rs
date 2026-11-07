# Path snapshot allocation amplification (#663)

A constrained synthetic run with actual RCH retained 100,000 cached announces.
At 19:31:56 UTC on 2026-11-07, a persistence pass coincided with anonymous memory
rising from 242.16 MB to 368.94 MB, RSS from 265.76 MB to 391.50 MB, and cgroup
memory from 679.31 MB to 797.85 MB. One `memory.high` event was added; memory
returned close to its previous level about four seconds later. The persistence
report examined 100,002 packets, skipped 100,000 known versions, wrote two and
completed in 244.133 ms. WAL size and propagation maintenance counters did not
change. Another pass reproduced the transient allocation without another high
event. This is a timing correlation, not a causal profiler trace or evidence of
continuing high-event growth.

Source inspection found avoidable scratch allocations in the metadata encoding:
`PathTable::encode_python_entries` constructed an owned nested `rmpv::Value`
tree for the entire retained table, including eight-value arrays per row and
cloned binary fields/random blobs, before encoding that tree into another
buffer. Tunnel encoding did the same. Save collection also moved all exported
paths into a second growing metadata vector. Packet bodies already used shared
`Arc` ownership; this change does not repeat the earlier packet-clone fix.

The replacement writes MessagePack directly from borrowed metadata fields into
the existing output buffer. Active path filtering now retains entries in place,
and metadata vectors are released as soon as they have been encoded. One owned
metadata snapshot and the final encoded buffers remain deliberate: a consistent
checkpoint can be written after releasing runtime locks. This is not incremental
metadata persistence; traversal, fingerprinting and output size still scale with
retained state. No queues, timeouts, memory limits or cache retention policies
change.

The binary format remains identical: array shapes/order, binary tags, f64 bits,
shortest unsigned hops, newest 64 active random blobs, all supplied tunnel blobs,
and optional hashes. Independent pre-change encoder oracles check exact bytes,
including NaN payloads, negative zero, array header boundaries and multiple
ordered tunnel paths. Snapshot locks, shared packet ownership, packet-before-
metadata publication, error restoration and reference-aware cleanup gating are
unchanged. Files still use the existing `tokio::fs::write`; no new atomic rename
or crash-consistency guarantee is claimed.

## Controlled allocation evidence

The [recorded counters and output hashes](path-snapshot-allocation-663.json)
come from fresh exact-test processes using a safe `cap` allocator around
`std::alloc::System`, enabled only in the transport library test binary with no
allocation limit. Each case creates 100,000 owned metadata entries with one
random blob before sampling codec work. The baseline uses the encoders from
`7ee7cc91` with the same test-only measurement fixture.

| Codec | Encoded bytes | Baseline peak extra requested bytes | Candidate peak extra requested bytes |
| --- | ---: | ---: | ---: |
| Active paths | 13,749,957 | 75,765,824 | 25,165,824 |
| Tunnels | 16,699,957 | 93,171,520 | 20,971,520 |

Peak codec scratch fell by 66.8% for paths and 77.5% for tunnels. Candidate peak
extra allocation equals only the retained output-buffer capacity. Cumulative
requests fell from 100,931,632 to 50,331,632 bytes for paths and 114,143,008 to
41,943,008 bytes for tunnels. Each baseline/candidate pair produced the exact
same encoded length and SHA-256 checksum. The probe enforces a peak budget of
twice output length plus 1 MiB, which the old full-tree encoder exceeds.

Reproduce one isolated case per fresh test process:

```sh
cargo test -p reticulum-rs-transport --lib \
  transport::reticulum_path_store::snapshot_allocation_tests::persistence_codec_allocation_probe \
  -- --exact --ignored --nocapture
LXMF_SNAPSHOT_CODEC=tunnels cargo test -p reticulum-rs-transport --lib \
  transport::reticulum_path_store::snapshot_allocation_tests::persistence_codec_allocation_probe \
  -- --exact --ignored --nocapture
```

These are requested Rust allocation measurements, not resident-memory or cgroup
accounting. They exclude input construction, snapshot collection, packet
fingerprinting, filesystem I/O and cleanup; the in-place filtering and earlier
metadata release provide additional unquantified savings. Debug elapsed times
are diagnostic single samples. Rebuilt-daemon retained-state validation must
separately establish the cgroup effect; no production plateau is inferred here.
