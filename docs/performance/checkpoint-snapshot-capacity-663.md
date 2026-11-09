# Checkpoint snapshot vector capacities (#663)

This change reduces avoidable owned-vector capacity during path-table capture.
It preserves the checkpoint format, route/packet identity, checkpoint frequency,
file publication, cleanup and shutdown ordering. Its evidence is a synthetic
source-level allocation/call-cost experiment, not daemon RSS or sustained resource
qualification. Paired runtime verdicts belong to the follow-up PR.

The original collector first exported every interface-mapped active route, then
removed rows without a cached packet. That retained a geometrically grown metadata
vector even for heavily filtered tables. Its separate packet-reference vector also
grew geometrically. The new private collector counts actual eligible active rows
`K` and every cached tunnel reference `T` under the same existing handler/interface
guards. It checks the sum, reserves active metadata for `K` and packet references
for `K + T`, then fills them in the original active-then-tunnel iteration order.
Only the fill clones random blobs and packet `Arc`s. It never reserves from the
raw route count when most rows are filtered out.

The public `export_python_entries(FnMut)` contract still invokes its lookup once
per visited route. The internal collector instead uses a documented stable view
for its two traversals while the guards remain held. One pure row materializer
keeps timestamp/expiry/field mapping shared. Both passes use the same captured
monotonic/Unix time. Tunnel references retain detached/missing-interface fallback,
missing-mode-but-known-hash behavior and duplicate hash/interface multiplicity.
Reservation failures propagate before canonical-file I/O. Snapshot encoding,
metadata lifetimes after encoding, dirty-file verification and cleanup remain
unchanged; no streaming, cache advice or configuration policy is introduced.

## Controlled evidence

[The evidence record](checkpoint-snapshot-capacity-663.json) contains all numeric
summaries, source/executable fingerprints and raw-log hashes. Thirty-six fresh
processes ran three matched ABBA blocks per case on CPU2, using the same default
release test executable. The legacy reference is the old collector algorithm
compiled alongside the candidate. Every process compares exact metadata, bytes,
iteration order and shared packet identity on its own unchanged fixture after the
measured operation. HashMap order is randomized across processes; output hashes
are not expected to match between different process fixtures.

| Case | Actual K/T | Metadata capacity bytes, old → new | Packet-reference capacity bytes, old → new | Capture median, old → new |
| --- | ---: | ---: | ---: | ---: |
| Dense | 100,000 / 0 | 20,971,520 → 16,000,000 | 7,340,032 → 5,600,000 | 79.12 → 82.59 ms |
| Sparse cache | 10 / 0 | 20,971,520 → 1,600 | 896 → 560 | 25.85 → 5.20 ms |
| Mixed interfaces/tunnels | 20,000 / 60 | 5,242,880 → 3,200,000 | 1,835,008 → 1,123,360 | 25.32 → 30.62 ms |

The dense capture-plus-encoding peak requested allocation falls from 43,061,510
to 36,349,958 bytes, a 6,711,552-byte reduction. Its capture allocation churn falls
from 57,622,240 to 22,600,000 requested bytes. Dense interface lookup calls increase
from 100,000 to 200,000; mixed calls increase from 100,004 to 200,004. The extra
walk has a CPU cost: the observed dense median rises 3.47 ms and mixed rises
5.30 ms. Six-sample ranges are retained in the JSON; these medians do not establish
a daemon lock-hold or latency guarantee. Sparse capture benefits from avoiding
metadata/blob clones for missing cache entries.

The safe test allocator wraps `System`. Values are requested layouts, not RSS,
cgroup charges or native allocator retention. In the sparse candidate the global
high-water comes from fixture construction, not capture; retained/cumulative
allocation and vector capacities isolate that case's improvement. Timing covers
capture only and excludes fixture creation, encoding and subsequent oracle/digest
work. The interface view is synthetic; the cache and collector are real native
implementations. No production data, services or cache conditioning are used.

Candidate pointer/capacity pairs stay unchanged from reservation through fill in
all 18 measured candidate processes. Tests do not promise the portable equality
`Vec.capacity() == len`: allocators may round reservations. A tolerant owned-vector
slack regression fails against the old collector and passes against the candidate;
it is a local allocation test, not a daemon memory acceptance gate.

The full transport library passes 1,095 tests (17 ignored), including signed cache
restore/rejection, file verification, reference-aware cleanup and restart cases.
Six focused capacity/parity cases also pass in the final executable. Strict
all-feature library/test Clippy and format/module/include/boundary checks pass.
DBus development metadata was supplied through a verified owned native prefix for
Clippy only; the default-release probes had no such settings. Setup failures and
the genuine allocation RED are distinguished in the evidence record.

Run one probe per fresh process, selecting `legacy` or `candidate` and `dense`,
`sparse` or `mixed`:

```sh
LXMF_CHECKPOINT_MODE=candidate LXMF_CHECKPOINT_CASE=dense \
  cargo test -p reticulum-rs-transport --release --lib \
  transport::reticulum_path_store::snapshot::tests::checkpoint_capture_allocation_probe \
  -- --exact --ignored --nocapture --test-threads=1
```
