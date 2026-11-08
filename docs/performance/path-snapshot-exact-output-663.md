# Exact path-snapshot output allocation (#663)

The direct codec in `c37f4c88` removed the MessagePack object tree, but its
growing output buffer still reserved 25,165,824 bytes for a 13,700,005-byte
100,000-route snapshot. The constrained actual-RCH regular stage continued to
add four `memory.high` events across four checkpoints (+3/0/0/+1). Anonymous
memory rose about 76 MB at the second checkpoint. That correlation motivates
reducing measured scratch ownership; it does not identify file-cache ownership.
Post-stage residency measurement was unavailable because the restart guard
ended the lane before its measurement boundary. No cache conditioning occurred.

The encoder now runs the same generic MessagePack writer first against a checked,
allocation-free byte counter, then against one reserved output buffer. Both
passes borrow the immutable metadata snapshot after runtime locks are released.
Reservation failure returns `OutOfMemory`, count overflow remains an error, and
the finished length must match the count. Wire format, packet sharing, write
ordering, cleanup and the shutdown save fence remain unchanged. The extra pass
trades metadata traversal time for lower transient allocation.

[Recorded measurements](path-snapshot-exact-output-663.json) use five alternating
fresh debug-test processes for each implementation on CPUs 2 and 3, after all
measured services had stopped. The native path table exports 100,000 routes with
one random blob each. The production packet-reference tuple layout uses a shared
dummy packet to isolate scratch ownership. Counters wrap `System` using safe
test-only `cap` instrumentation without an allocation limit.

| Measurement | c37 growing output | Exact output |
| --- | ---: | ---: |
| Metadata vector plus random blobs | 21,971,520 B | 21,971,520 B |
| Packet-reference vector | 7,340,032 B | 7,340,032 B |
| Output bytes | 13,700,005 B | 13,700,005 B |
| Output capacity observed | 25,165,824 B | 13,700,005 B |
| Peak extra requested bytes, including snapshot metadata | 54,477,376 B | 43,011,557 B |
| Cumulative requested bytes, through cleanup-set construction | 112,279,264 B | 75,647,637 B |
| Median export + references + encode duration | 168.60 ms | 228.97 ms |

The stage peak falls 11,465,819 bytes (21.0%). These are requested allocations,
not RSS or cgroup charges. Peak columns are cumulative process high-water minus
pre-export live allocation; candidate fixture-construction high-water was only
8,027,242 bytes above that baseline. Timing stops after encoding and excludes
the later metadata drop and cleanup-set construction. The debug median increases
60.37 ms; release daemon behavior requires separate measurement. Packet body
allocation, cache filtering, fingerprint work, filesystem I/O, lock latency and
native allocator retention are outside this probe.

Separate 100,000-entry codec probes request exactly one output allocation:
13,749,957 bytes for paths and 16,699,957 bytes for tunnels. Both retain the
[previous exact checksums](path-snapshot-allocation-663.json). Seven focused
tests compare legacy bytes and exercise nested tunnel order, float bits,
array-header boundaries, active/tunnel random-blob policies, count overflow,
reservation failure and unchanged output pointer/capacity after reservation.
The ignored probes enforce peak and cumulative allocation budgets.

Run each probe alone in a fresh process (the allocator peak cannot be reset):

```sh
cargo test -p reticulum-rs-transport --lib \
  transport::reticulum_path_store::snapshot_allocation_tests::persistence_snapshot_stage_probe \
  -- --exact --ignored --nocapture
```

For codec-only paths and tunnels, use the commands in the
[original allocation record](path-snapshot-allocation-663.md). No retained-state
resource plateau is inferred. The c37 regular stage passed its 20-minute delivery,
window and four-Topic checks but failed the strict local pressure gate. Its
combined restart lane failed a count-only guard; offline identity analysis found
100,000 retained routes across active and tunnel state, including legitimate
bounded-cache replacement of three seeded identities. That finding does not
turn the failed lane into a pass. A new immutable build, exact retained-identity
recovery checks and the hours-long resource lanes remain required. The issue's
acceptance is no continuing swap/reclaim growth, not zero lifetime high events.
