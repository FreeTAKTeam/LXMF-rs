# #614 native interface runtimes and Windows BLE evidence

Status: **partial / hardware-unverified**. This records the bounded software
increment at candidate commit `f753c4d7` on `codex/issue-605-parity`; it does
not claim the full #614 or #605 acceptance gate.

## Reference and ownership

- Forward reference: Reticulum
  `99de23c040d507e3fefca19e87b182302902725d` (`1.5.4-dev`), primarily
  `RNS/Interfaces/RNodeInterface.py` and the native interface modules.
- Rust owners: `crates/libs/rns-transport/src/iface` for native carrier
  runtimes and `crates/apps/reticulumd` for production interface startup.

## Implemented behavior

| Area | Implemented and tested behavior | Status |
| --- | --- | --- |
| Windows paired-device lookup | The Windows BLE backend asks WinRT for the paired-device selector, enumerates `DeviceInformation`, extracts only strict Bluetooth address suffixes from each device ID, and filters scan candidates by the paired address before configured ID, alias, or service matching. An empty paired set therefore cannot silently select an unbonded scan result. | local verified; native Windows unverified |
| Windows backend boundary | The resolver is target-gated and uses the existing `btleplug` scan/connect/service-discovery path; Android's configured-peripheral path is unchanged. The implementation does not use `btleplug`'s unsupported Windows `add_peripheral` address shortcut. | local code verified |
| Runtime cleanup | Existing BLE startup still clears stale session state, stops scans after selection or timeout, subscribes before startup writes, and aggregates unsubscribe/scan-stop/disconnect failures during cleanup. This increment applies the pairing constraint before those existing connect/reconnect paths. | local state-machine tests; native carrier unverified |
| Interface inventory | The daemon has explicit startup branches for TCP/backbone, local TCP/Unix, UDP, AutoInterface, serial, Weave, KISS/AX.25, pipe, I2P, Meshtastic, BLE, LoRa, and RNodeMulti aliases; unknown kinds record an explicit unsupported-kind failure. | source inspection; cross-platform/live evidence open |

## Commands and results

All commands ran in the isolated `codex/issue-605-parity` worktree.

```text
cargo fmt --all -- --check                                      PASS
git diff --check                                                 PASS
cargo test -p reticulum-rs-transport --features rnode-ble \
  --lib rnode_ble -- --nocapture                                PASS (16 tests)
cargo test -p reticulum-rs-transport --features rnode-ble --tests PASS
  (813 library tests and 169 integration tests)
cargo clippy -p reticulum-rs-transport --features rnode-ble \
  --lib --all-targets --no-deps -- -D warnings                  PASS
```

The Windows target is installed, but this Linux host has neither a MinGW
compiler/sysroot nor Windows SDK headers. The normal target check stopped in
`bzip2-sys` because `x86_64-w64-mingw32-gcc` is unavailable. A retry with the
available `clang`/`llvm-ar` stopped before Rust crate checking because the
Windows C headers `stdlib.h` and `stdio.h` were unavailable. Therefore this
increment has no Windows compilation result from this host.

## Deliberate remaining gaps

- A native Windows build and paired RNode test still need to prove bonded
  selection, stale paired references, partial service discovery, EOF versus
  idle reads, detection timeout, cancellation, reconnect, and bounded cleanup.
- AutoInterface still needs native link-local enumeration, socket reuse,
  carrier-loss recovery, and stop/restart evidence on each supported host;
  the library-level runtime tests are not a substitute for those platform
  traces or for retaining a daemon stop handle through its full lifecycle.
- The complete TCP, local/shared, UDP, pipe, serial/KISS/AX.25, RNode,
  Weave, I2P, mobile, and other reference-family matrix remains only partly
  covered by local source/tests. Hosted/native platform combinations and
  physical carrier evidence remain open under #614/#616.
- No live Python↔Rust interface transcript, external client trace, or paired
  hardware evidence was available in this run.
