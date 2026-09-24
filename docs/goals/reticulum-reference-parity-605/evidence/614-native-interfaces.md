# #614 native interface runtimes and Windows BLE evidence

Status: **partial / hardware-unverified**. The bounded pairing implementation
originated at `f753c4d7` on `codex/issue-605-parity`; the follow-up hosted
Windows query/test evidence below is from `ca6b6bba` on
`corvo/issue-614-windows-ci`. This does not claim the full #614 or #605
acceptance gate.

## Reference and ownership

- Forward reference: Reticulum
  `99de23c040d507e3fefca19e87b182302902725d` (`1.5.4-dev`), primarily
  `RNS/Interfaces/RNodeInterface.py` and the native interface modules.
- Rust owners: `crates/libs/rns-transport/src/iface` for native carrier
  runtimes and `crates/apps/reticulumd` for production interface startup.

## Implemented behavior

| Area | Implemented and tested behavior | Status |
| --- | --- | --- |
| Windows paired-device lookup | The Windows BLE backend asks WinRT for the paired-device selector, enumerates `DeviceInformation`, extracts only strict Bluetooth address suffixes from each device ID, and filters scan candidates by the paired address before configured ID, alias, or service matching. Deterministic coverage verifies a stale paired address cannot authorize a different scanned device, while a matching address remains eligible. A Windows-only hosted test queries the native paired-device list and compares Rust suffix extraction against the pinned reference rule without logging device addresses. | Linux parser/filter tests verified; hosted native query/test passed on `ca6b6bba`; actual stale Windows pairing removal and physical paired-device behavior unverified |
| Windows backend boundary | The resolver is target-gated and uses the existing `btleplug` scan/connect/service-discovery path; Android's configured-peripheral path is unchanged. The implementation does not use `btleplug`'s unsupported Windows `add_peripheral` address shortcut. | local code verified |
| Runtime cleanup and read states | BLE notification timeout is an idle read: it returns no event while preserving the live session, allowing a later notification to be delivered. Native notification-stream EOF is tracked separately, resets the session, and is returned to the caller as a `next_notification` backend error. A deterministic scripted-backend regression exercises idle → data → EOF without hardware. This aligns with the pinned Python loop, which treats an empty BLE receive queue as no bytes and continues polling; connection/read exceptions remain terminal. | focused fake-backend regression; native carrier unverified |
| BLE worker recovery after notification EOF | A deterministic backend ends the first worker session with native-style notification-stream EOF. The production worker closes that session before its bounded reconnect, obtains a fresh backend, and closes the recovered session on cancellation. | focused fake-backend worker regression; native stream and carrier unverified |
| BLE partial setup and retry | Deterministic backends fail once during notification setup and during connection acquisition, recording cleanup before a second startup. Each runtime reconnects successfully, becomes connected, and releases the recovered session on close. This verifies reusable-runtime cleanup/retry after a backend-reported partial setup/connect failure; it does not execute native GATT service discovery or prove Windows/macOS/Android cancellation behavior. | focused fake-backend regressions; native carrier unverified |
| BLE service-discovery cancellation recovery | The production interface worker receives a deterministic backend cancellation error during its service-discovery/connect phase, closes that partial session before retrying, creates exactly one fresh backend after the configured 1 ms backoff, completes notification subscription/startup writes, and closes the recovered session when stopped. The test is radio-free and bounds both recovery and shutdown. It models a backend-reported cancellation result; it does not cancel/drop the Rust startup future or exercise native GATT cancellation/cleanup. | focused fake-backend worker regression; native carrier unverified |
| BLE startup cancellation cleanup | The production interface worker selects both interface cancellation tokens while `runtime.startup()` is pending. On cancellation it drops the in-flight setup future, closes the runtime backend, then exits instead of remaining blocked in connect/service discovery. A deterministic backend held indefinitely in `connect()` verifies prompt worker exit and backend close. This is token-driven software cancellation; task abortion and native GATT cancellation are not covered. | focused fake-backend regression; native GATT behavior unverified |
| BLE worker detection fallback | A private backend factory, used only by this worker and defaulting to the unchanged native backend constructor, lets a deterministic fake run through the actual worker loop. With CMD_DETECT withheld, the configured bounded deadline emits deferred radio configuration; a scripted disconnect verifies cleanup before the worker creates a fresh backend, and cancellation closes that second session. This is software fault injection only, not physical BLE support or device evidence. Behavior was compared with pinned Python's five-second `ble_detect_timeout` wait at `99de23c040d507e3fefca19e87b182302902725d`; Rust's configured fallback remains separately bounded and tested without waiting five seconds. | focused fake-backend worker regression; no physical device |
| Interface inventory | The daemon has explicit startup branches for TCP/backbone, local TCP/Unix, UDP, AutoInterface, serial, Weave, KISS/AX.25, pipe, I2P, Meshtastic, BLE, LoRa, and RNodeMulti aliases; unknown kinds record an explicit unsupported-kind failure. | source inspection; cross-platform/live evidence open |
| Strict I2P startup after SAM handshake rejection | The production `startup_i2p` path runs with strict startup enabled against a deterministic local SAM peer. The peer accepts the production HELLO command but replies with `RESULT=I2P_ERROR`; startup returns no runtime handle, registers no interface with the production `InterfaceManager`, and records one startup failure containing the SAM preflight context and rejection text. | focused fake-SAM daemon-startup regression; does not cover destination creation failure, established-session packet flow, real router behavior, or public I2P connectivity |
| Native Windows CI | The PR workflow runs the `rnode-ble` library test filter on `windows-latest`, compiling the target-gated WinRT resolver, executing deterministic paired-ID/runtime tests, and invoking the real WinRT paired-device query. A runner with no paired radios may validly return an empty set; this does not verify physical pairing. | passed on `ca6b6bba` (18 tests); physical paired-RNode behavior remains unverified |
| AutoInterface software lifecycle and native enumeration API smoke | Loopback-only library and daemon-binary regressions cover explicit stop, socket release, and same-port restart. A separate smoke regression calls the production `if_addrs::get_if_addrs()`-backed link-local enumerator and checks each returned candidate has a name and only IPv6 link-local addresses; an empty result is accepted for hosts without eligible NICs. It opens no sockets, sends no multicast, and mutates no OS state. | focused software lifecycle and host-enumeration API checks; native carrier behavior, multicast observation, cross-platform enumeration behavior, daemon process/signal integration, and physical carrier behavior remain unverified |
| Daemon-configured AutoInterface ownership and restart | The daemon retains each started `AutoDiscoveryRuntime` stop handle separately from the status refresher, carries it through bootstrap into `BootstrapContext`, then awaits `stop()` before removing the InterfaceManager host channel after RPC shutdown. A deterministic plan-builder seam is limited to configured-interface startup; production still calls the unchanged native `build_native_startup_plan`. The regression parses a real `AutoInterface` TOML stanza, maps its config, injects loopback listener bindings, invokes `startup_configured_interfaces`, verifies running/status handles and channel registration, calls the same async shutdown helper as daemon main, confirms socket release/channel removal, and restarts on the same ports. | focused software-only configured-startup/shutdown/restart regression; full process signal/RPC integration, native enumeration, platform coverage, and physical carrier behavior remain unverified |
| AutoInterface partial-startup socket rollback | A loopback library regression lets production startup bind its discovery sockets, then injects an invalid data-listener address so startup returns an error before yielding a runtime handle. It immediately binds the same discovery port with a standard UDP socket, repairs the data address, and successfully starts/stops the same plan. This demonstrates cleanup of already-bound discovery sockets on a later bind failure. | focused Unix loopback regression; does not test native interface enumeration, multicast delivery, Windows/macOS socket semantics, or full daemon process/signal teardown |
| AutoInterface per-device carrier echo loss/recovery | A deterministic library regression primes the production peer-job timeout state, passes a valid group token through `process_discovery_datagram` for one adopted test device, advances the production peer-job path past the configured echo deadline without another echo, and then supplies a valid echo to recover. It asserts the observable status changes and exact `carrier_lost`/`carrier_recovered` events name only that device. In the same regression, the actual AutoDiscoveryRuntime binds loopback sockets, awaits explicit `stop()`, proves discovery/data ports can be rebound, and restarts/stops on those same ports. | focused software regression; does not exercise native multicast receipt, OS carrier detection, native link-local enumeration, cross-platform semantics, or physical carrier behavior |
| Strict I2P daemon startup after SAM handshake rejection | The production `startup_i2p` path is run with strict startup enabled against a deterministic local SAM peer. The peer accepts the production HELLO command but replies with `RESULT=I2P_ERROR`; startup returns no runtime handle, registers no interface with the production `InterfaceManager`, and records one startup failure containing the SAM preflight context and rejection text. | focused fake-SAM daemon-startup regression; does not cover destination creation failure, established-session packet flow, real router behavior, or public I2P connectivity |

## Commands and results

The initial implementation checks below ran in the isolated
`codex/issue-605-parity` worktree at `f753c4d7`:

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

Follow-up checks for PR #634 ran in the isolated
`corvo/issue-614-windows-ci` worktree at `ca6b6bba`:

```text
cargo fmt --all -- --check                                      PASS
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  rnode_ble -- --nocapture                                      PASS (17 Linux tests)
cargo clippy -p reticulum-rs-transport --features rnode-ble \
  --all-targets --no-deps -- -D warnings                       PASS
tools/scripts/check-module-size.sh                              PASS
git diff --check                                                 PASS
```

The additional loopback lifecycle slice ran in the same isolated worktree:

```text
cargo test -p reticulum-rs-transport auto_runtime_stop_releases_loopback_sockets_for_restart --lib -- --nocapture PASS (1 test)
cargo test -p reticulum-rs-transport auto_runtime_startup_failure_releases_discovery_socket_before_retry --lib -- --nocapture PASS (1 test)
cargo test -p reticulum-rs-transport auto_runtime_reports_per_device_echo_loss_and_recovery_and_restarts_cleanly --lib -- --nocapture PASS (1 test)
cargo test -p reticulum-rs-transport auto --lib                      PASS (114 tests, current head)
cargo fmt --all -- --check                                           PASS
cargo clippy -p reticulum-rs-transport --all-targets --no-deps -- -D warnings PASS
tools/scripts/check-module-size.sh                                    PASS
git diff --check                                                     PASS
```

The software-only BLE idle/EOF regression ran on the current PR #634 head:

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  native_stream_eof_is_distinct_from_idle_and_idle_read_recovers -- --nocapture PASS
```

The worker-level EOF recovery regression also ran on the current PR #634
worktree. It injects native-style EOF through the private fake backend factory,
asserts the first backend closes before the worker reconnects, and verifies
cancellation closes the recovered session; it does not exercise native GATT:

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  worker_closes_eof_session_before_reconnecting_with_fresh_backend -- --nocapture PASS
```

The BLE partial-setup cleanup and reconnect regression ran on the current
PR #634 follow-up head:

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
failed_partial_setup_is_closed_before_runtime_reconnects -- --nocapture PASS
```

The additional partial-connect cleanup/retry regression runs through the same
private backend boundary. The pinned Python implementation uses a BLE client
context manager around connection, service lookup, and notification setup, so
an exception unwinds the client context before its worker attempts another
connection; this fake verifies the Rust runtime's bounded equivalent for
backend-owned partial state, not native service-discovery cleanup:

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  failed_partial_connect_is_closed_before_runtime_retries -- --nocapture PASS
```

The software-only worker service-discovery cancellation/recovery regression
ran on the current PR branch. It injects a backend-reported cancellation at
discovery, asserts partial-session cleanup precedes exactly one fresh-backend
retry, observes successful subscription/startup writes, then stops the worker
and checks recovered-session cleanup. The configured backoff is 1 ms and the
overall recovery/stop waits are bounded at one second. It does not model
cancellation by dropping the Rust startup future or native GATT cancellation:

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  worker_cleans_up_cancelled_service_discovery_before_bounded_retry -- --nocapture PASS
```

On the dispatched PR #634 head `acf53c19`, the Linux BLE-focused library
filter ran all 22 BLE runtime tests successfully. This includes the fake
partial-connect cleanup/retry and worker service-discovery cancellation/retry
cases above. Formatting, transport-library clippy, module-size, and diff checks
also passed on that head. These results exercise deterministic software
backends only; they do not establish native GATT cleanup or physical-radio
behavior.

The software-only worker detection-timeout regression ran on the current PR
#634 branch. It withholds `CMD_DETECT`, observes fallback configuration after
the configured deadline, then cancels the worker and verifies backend closure.
This is fault injection through the real software worker path; no physical BLE
device is accessed. It also injects a post-fallback disconnect and verifies
cleanup followed by fresh-backend reuse:

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib \
  worker_detection_timeout_sends_fallback_and_closes_backend_on_cancel -- --nocapture PASS
```

The startup-cancellation regression holds the backend inside `connect()`,
cancels the production worker's interface token, and verifies that the pending
setup is dropped, backend cleanup runs, and the worker exits within one second.
The backend-reported service-discovery cancellation/retry test remains green:

```text
cargo test -p reticulum-rs-transport --features rnode-ble --lib cancelling_during_ble_startup_closes_the_partial_backend -- --nocapture PASS
cargo test -p reticulum-rs-transport --features rnode-ble --lib worker_cleans_up_cancelled_service_discovery_before_bounded_retry -- --nocapture PASS
cargo test -p reticulum-rs-transport --features rnode-ble --lib rnode_ble -- --nocapture PASS (24 tests)
```

The daemon activation lifecycle regression ran in the same worktree:

```text
cargo test -p reticulumd --bin reticulumd daemon_auto_activation_stops_adapter_and_restarts_on_owned_loopback_ports -- --nocapture PASS (1 test)
cargo test -p reticulumd --bin reticulumd daemon_auto_toml_ports_reach_production_adapter_and_are_reusable_after_restart -- --nocapture PASS (1 test)
cargo test -p reticulumd --bin reticulumd auto_ -- --nocapture PASS (3 tests)
cargo clippy -p reticulumd --all-targets --no-deps -- -D warnings PASS
cargo fmt --all -- --check PASS
tools/scripts/check-module-size.sh PASS
cargo run -p xtask -- architecture-checks PASS
tools/scripts/check-boundaries.sh PASS
git diff --check PASS
```

The daemon-owned configured-startup/shutdown regression was added on PR #634:

```text
cargo test -p reticulumd --bin reticulumd configured_daemon_auto_shutdown_releases_sockets_and_allows_restart -- --nocapture PASS (1 test)
cargo test -p reticulumd --bin reticulumd auto_ -- --nocapture PASS (5 tests)
cargo clippy -p reticulumd --all-targets --no-deps -- -D warnings PASS
cargo fmt --all -- --check PASS
tools/scripts/check-module-size.sh PASS
git diff --check PASS
```

The per-device AutoInterface carrier echo regression and final checks ran on
the current PR #634 working head:

```text
cargo test -p reticulum-rs-transport auto_runtime_reports_per_device_echo_loss_and_recovery_and_restarts_cleanly --lib -- --nocapture PASS (1 test)
cargo test -p reticulum-rs-transport auto --lib PASS (114 tests)
cargo fmt --all -- --check PASS
cargo clippy -p reticulum-rs-transport --all-targets --no-deps -- -D warnings PASS
TMPDIR=/dev/shm tools/scripts/check-boundaries.sh PASS
TMPDIR=/dev/shm cargo run -p xtask -- architecture-checks PASS
tools/scripts/check-module-size.sh PASS
git diff --check PASS
```

The pinned Python `AutoInterface.peer_jobs()` removes each timed-out peer from
`self.peers` and calls `detach()`/`teardown()` on its spawned interface
(`.tmp/python-refs/Reticulum-99de23c/RNS/Interfaces/AutoInterface.py`, lines
371–390). Rust already expired the peer-table record, but its transport bridge
retained the corresponding virtual interface and outbound socket route. The
peer-job path now prunes only expired peer routes and stops those virtual
interfaces. A deterministic regression expires one peer while asserting a
second peer's interface and route remain available:

```text
TMPDIR=/dev/shm cargo test -p reticulum-rs-transport auto --lib PASS (115 tests)
TMPDIR=/dev/shm cargo test -p reticulumd --bin reticulumd configured_daemon_auto_shutdown_releases_sockets_and_allows_restart -- --nocapture PASS (1 test)
cargo clippy -p reticulum-rs-transport -p reticulumd --all-targets --no-deps -- -D warnings PASS
cargo fmt --all -- --check PASS
tools/scripts/check-module-size.sh PASS
git diff --check PASS
```

This closes a software peer-expiry lifecycle gap only. It does not add native
carrier or physical-device evidence.

The hosted [Windows RNode BLE job](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/35803940756/job/107000373907)
passed on commit `ca6b6bbab13007ca6d9adb55b525ce978f763043` and ran 18 tests,
including `native_windows_paired_device_query_matches_reference_id_suffixes`
and `extracts_sorted_unique_addresses_from_reference_device_ids`. The new
WinRT query test is target-gated and was not executed by the local Linux run.

The Windows target is installed, but this Linux host has neither a MinGW
compiler/sysroot nor Windows SDK headers. The normal target check stopped in
`bzip2-sys` because `x86_64-w64-mingw32-gcc` is unavailable. A retry with the
available `clang`/`llvm-ar` stopped before Rust crate checking because the
Windows C headers `stdlib.h` and `stdio.h` were unavailable. Therefore the
original local run has no Windows compilation result. The hosted lane supplies
Windows software compile/test evidence for the recorded commit; it does not
establish physical Windows pairing or carrier behavior.

## Deliberate remaining gaps

- A native Windows build and paired RNode test still need to prove bonded
  selection, stale paired references, native partial service discovery,
  physical detection timeout, native cancellation, and bounded cleanup. The software worker
  fault-injection test now covers timeout fallback and cancellation cleanup,
  but proves no native GATT or physical-device behavior. The software runtime reconnect
  regressions above cover retry after notification-setup failure and a
  backend-reported service-discovery cancellation, not native GATT cancellation
  or cleanup. A separate token-driven test now proves the worker drops a pending
  startup future and closes its backend; forced task abortion and native GATT
  cancellation are not covered. The EOF/idle semantic distinction
  is covered by the software regression above, not a native trace.
- Worker recovery after native-style notification EOF now has deterministic
  backend-factory coverage. Native stream termination, platform cleanup, and
  paired-device recovery remain unverified.
- AutoInterface now has a host-enumeration API smoke check, but still needs
  native carrier echo observation/recovery, plus stop/restart evidence on each supported host. The
  new deterministic software regression exercises the production peer-job and
  status paths for one device's missing/returning authenticated echo, while the
  loopback runtime proves explicit task/socket teardown and same-port restart.
  Neither test runs the full `bootstrap::bootstrap` plus daemon RPC/signal
  shutdown path or establishes a native-interface or cross-platform trace.
- The complete TCP, local/shared, UDP, pipe, serial/KISS/AX.25, RNode,
  Weave, I2P, mobile, and other reference-family matrix remains only partly
  covered by local source/tests. Hosted/native platform combinations and
  physical carrier evidence remain open under #614/#616.
- No live Python↔Rust interface transcript, external client trace, or paired
  hardware evidence was available in this run.
