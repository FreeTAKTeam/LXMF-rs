# Checkpoint reference-set lifetime (#663)

This follow-up shortens one temporary ownership span. It does not remove the set's
construction or establish a daemon memory improvement. The earlier
[capacity evidence](checkpoint-snapshot-capacity-663.md) at `b72feec8` remains
unchanged and measures another interval. These decreases must not be added into
a combined peak, RSS, cgroup or reclaim prediction. Runtime qualification remains
separate on the follow-up PR.

The checkpoint formerly retained a duplicate `HashSet` of current packet hashes
through cache-file processing, both metadata writes and cleanup. The synchronous
`retain_current_files` helper now owns that set and drops it before returning,
before any following packet-cache or metadata I/O. Its size and construction are
the same as before.

After the retain pass, the fingerprint map contains only current snapshot keys.
Each successful packet iteration then inserts its current hash. Once that loop
finishes, the map's keys equal the active/tunnel reference union. Repeated hashes
on different interfaces preserve the last fingerprint value without changing
membership. Post-save cleanup can therefore use the existing fingerprint-map
membership helper instead of holding the set. Both variants delegate to the same
bounded cleanup engine with unchanged batch and age policy.

Cleanup still runs only after all cache writes and both metadata writes succeed.
Failure state remains ineligible for periodic cleanup. Existing error-state restore,
unknown-file verification and known-file fast paths are unchanged; no atomic-file,
streaming, cache-advice, checkpoint-frequency or retention-policy change is made.

## Controlled ownership evidence

[The evidence record](checkpoint-reference-set-lifetime-663.json) binds source
fingerprints and local raw-log hashes. Two fresh default-release processes model
100,000 current hashes and 100,003 packet references, including three repeated
interface variants, ten initially missing current map keys and one orphan key.
The new mode executes the production retaining helper. The reference mode keeps
the same set alive through the old publication boundary. There is no filesystem
or service in this model; the publication marker is an ownership boundary rather
than a measured daemon write interval.

| Requested allocation | Old retained span | New scoped span |
| --- | ---: | ---: |
| Set construction total | 4,325,392 B | 4,325,392 B |
| Construction global peak above fixture baseline | 4,325,392 B | 4,325,392 B |
| Live after retaining / at modeled publication | 4,325,392 B | 0 B |
| Live after final set release | 0 B | 0 B |

The fixture prior high-water is zero above the measurement baseline in both
processes. Values come from safe `cap` instrumentation around `System`: they are
requested allocations, not resident bytes or native allocator retention. The
construction peak is unchanged. No overall checkpoint-peak or sustained resource
benefit is inferred.

Tiny filesystem controls compare map/set cleanup membership, including a repeated
fingerprint, and force cache-file, destination-table and tunnel-table write errors.
They verify no premature cleanup, the periodic-cleanup gate, successful recovery
and the existing verification/known-file behavior. Signed active/tunnel restore and
bounded reference-aware cleanup controls also pass. All 19 focused budget controls and
14 transport persistence/capacity/codec/state checks, strict library/test Clippy and
architecture checks pass. The full isolated library suites remain recorded with
their original source commits; they are not relabeled as new full-suite runs.

Run each ownership mode alone:

```sh
LXMF_REFERENCE_SET_MODE=scoped cargo test -p reticulum-rs-transport --release --lib \
  transport::reticulum_path_store::lifetime_tests::reference_set_lifetime_allocation_probe \
  -- --exact --ignored --nocapture --test-threads=1
```
