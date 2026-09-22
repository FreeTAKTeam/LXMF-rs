# #623 Python↔Rust wire-conformance evidence

Status: **partial / unverified**. This increment adds a permanent, byte-level
lane for the pinned Python Reticulum/LXMF target and records its negative
cases. It does not close #623, #615, or the #605 parity epic: the broader live
Resource/LXMF matrix, multi-hop/restart/fault coverage, hosted artifacts, and
physical/client evidence remain separate acceptance work.

## Contract and ownership

- Python Reticulum target: `99de23c040d507e3fefca19e87b182302902725d`
  (`1.5.4-dev`).
- Python LXMF target: `727830cefda83d9c6e3982b48675425f3f988f9c`.
- Committed corpus: `tools/interop/python-rust-wire-conformance-v1.json`.
- Python harness: `tools/scripts/python_wire_conformance.py`.
- Rust decoder gate: `crates/libs/test-support/tests/wire_conformance.rs`.
- Local entry point: `cargo xtask interop`.
- Implementation commit: `40bcee8c` (`feat(interop): add Python-Rust wire conformance lane`).
- Verify CI runs the Python harness and Rust decoder test and uploads the
  machine-readable report under `python-rust-wire-conformance-${run_id}`.

The corpus records direction, wire kind, exact encoded bytes, expected decoded
fields, generator pins, and the Rust fixture source. The harness fails closed
when either pinned checkout is absent, dirty, or at a different revision. Its
report records the candidate commit, both reference revisions, toolchain,
vector ID, direction, byte length/hash, and result.

## Encoded behavior covered

The committed vectors exercise Python→Rust plain Reticulum packet bytes and a
Python→Rust LXMF wire message. The existing Rust-generated LXMF fixture is
decoded by the pinned Python reference, giving the reciprocal Rust→Python
direction. The same Python harness additionally replays the committed Rust
identity, announce, LXMF, link-request, link-proof, and link-RTT byte vectors;
the Rust `reticulum_spec_vectors` test remains the Rust-side decoder and
cryptographic/serialization check for those vectors.

Deliberately corrupted vectors cover empty packet data, hop count `128`, a
truncated LXMF MessagePack payload, and a reserved MessagePack code. Both Rust
packet decoders now reject empty data and `PATHFINDER_M` at decode time, which
matches the pinned Python `Packet.unpack()` behavior instead of deferring the
malformed frame to later transport admission.

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree against the
clean pinned Python checkouts listed above. The report outputs are ignored
`target/` artifacts and are not treated as committed release evidence.

```text
python3 tools/scripts/python_wire_conformance.py --self-test                         PASS
python3 tools/scripts/python_wire_conformance.py \
  --output target/interop/python-rust-wire-conformance/local/report.json              PASS
  (16 encoded/negative records; 0 failures; exact Python pins)
cargo test -p test-support --test wire_conformance -- --nocapture                    PASS
  (4 passed; 0 failed; 0 ignored)
cargo test -p reticulum-rs-core --lib packet::tests -- --nocapture                   PASS
  (2 passed; 0 failed)
cargo test -p reticulum-rs-transport --lib packet::tests -- --nocapture               PASS
  (6 passed; 0 failed)
cargo run -p xtask -- interop \
  --output target/interop/python-rust-wire-conformance/xtask/report.json              PASS
  (Python report pass; 4 Rust conformance tests passed; 0 failed; 0 ignored)
tools/scripts/check-boundaries.sh                                                    PASS
tools/scripts/check-module-size.sh                                                   PASS
```

The Verify workflow now repeats the byte-level Python harness with the exact
`Reticulum-parity` and `LXMF` checkouts, then runs the same Rust test and
uploads its report. This is an executable gate and artifact path, not a claim
that a local run substitutes for hosted exact-head CI.

## Remaining acceptance gaps

- The lane's byte corpus is intentionally bounded. It does not yet replace
  the 30-case live Python/Rust compatibility matrix or prove every Resource
  size, loss/reorder/duplication/cancellation, restart, and multi-hop case.
- The static vector corpus covers representative identity, packet, announce,
  proof, link, LXMF, and MessagePack paths; broader cryptographic and feature
  boundary expansion remains tracked under #615/#605.
- Hosted Verify artifacts, release-candidate retention, cross-platform runs,
  public/multi-hop networks, and physical HIL are not evidenced by this local
  run. #616 remains hardware-unverified.
