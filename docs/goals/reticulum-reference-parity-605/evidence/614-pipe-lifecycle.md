# Issue #614 PipeInterface software lifecycle evidence

These are bounded software-only regressions for the production `PipeInterface`
worker and configured daemon startup. They do not change the overall #614
status or establish physical interface acceptance.

## Scenario

On Linux, a temporary child script exits successfully on its first launch and
executes `cat` on the next launch. The test starts the real
`PipeInterface::spawn` worker through an `InterfaceContext`, waits for the
second child to reach `running` with exactly one recorded respawn, cancels the
interface token, and awaits worker completion. Final runtime status must be
`stopped`, closed, and still report exactly one respawn; a `kill -0` probe must
also confirm the second child's recorded PID is no longer live after the
worker joins.

No radio, network, hardware, or timing-sensitive external service is used. The
temporary script and marker reside in shared memory.

```text
cargo test -p reticulum-rs-transport \
  pipe_child_exit_respawns_and_interface_cancellation_reaps_child \
  --lib -- --nocapture                                           PASS (1 test)
```

This proves only the Linux software child-exit/restart and cancellation/reap
path.

## Configured daemon, peer packet loopback, and teardown

`tools/scripts/pipe-fake-subprocess-smoke.sh` starts `reticulumd` with a
configured PipeInterface whose deterministic local peer process executes
`cat`. The daemon's scheduled announce is HDLC-framed and written to the peer's
stdin; the peer echoes the packet on stdout through the production Pipe RX
worker. `rnstatus-rs` confirms configured startup/status and nonzero transport
RX and TX counters (the observed run recorded 167 bytes in each direction).
The smoke then sends SIGINT, waits for daemon exit, and verifies the peer PID
is no longer live. No public network, hardware, or external service is used.

```text
TIMEOUT_SECS=30 tools/scripts/pipe-fake-subprocess-smoke.sh  PASS
cargo test -p reticulumd --test pipe_fake_subprocess_smoke_contract -- --nocapture  PASS
```

The daemon/peer loopback and clean teardown are Linux software evidence only.
Windows/macOS Pipe subprocess behavior, independent remote-peer interoperability,
and physical interface acceptance remain unverified. Physical testing tracked
separately under #616 is explicitly excluded.
