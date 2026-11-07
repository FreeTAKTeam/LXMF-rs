# Foundation checkpoint review — 2026-10-15

POST correctness and maintainability review: no remaining must-fix findings for
this explicitly incomplete Stage 0 checkpoint. This is not Stage 0 acceptance.

Corrections made during review:

- Classic EOF drains accepted bytes before reporting closure; explicit abort and
  overflow remain terminal. Reader threads cannot occupy operation executor slots.
- GATT reserves opening state before native creation. Late-created handles close;
  explicit close never acknowledges a pending native write as successful.
- Cleanup preserves original queue-overflow errors.
- SDK historical channel projections do not reject reconnects. Projection locks
  do not span native calls. Initial subscriptions share the connect operation ID
  and cancellation lifetime. Early events cannot resurrect a retired projection.
- Artifact verification checks package identity, embedded clean revision, hashes
  and absence of native libraries; CI invokes it on release AAR/package output.
- Existing Windows lock resolutions were preserved; the lock diff is additive.
- Packaged-crate MSRV builds use a fresh target directory too: normalized archive
  timestamps must not permit reuse of stale Cargo build fingerprints.

Local evidence: six core tests, seven with the Android version feature, six with
BlueZ dependencies; independent Rust 1.85 consumer and Android target check;
Rust 1.88 integration build; four SDK adapter and eight existing SDK contract
checks; two mobile platform fixture tests; ten Android JVM tests, strict lint,
debug/release assembly; five synthetic artifact-pair tests; strict affected-crate
Clippy, workspace formatting, boundary/source-size/package-license checks.

BlueZ dependency compilation is not a native backend test. Rust Android target
compilation covers the version contract only. CI is configured but has not run
on GitHub. See STATUS.md for all pending software and hardware gates.
