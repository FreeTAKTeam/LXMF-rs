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

The regression `pipe_child_is_terminated_when_worker_task_is_aborted` covers
the abnormal shutdown path exposed by the hosted smoke: it starts a child that
ignores closed stdin, aborts the production worker task, and confirms the
recorded child PID is no longer live. `kill_on_drop(true)` on the Tokio child
handle supplies termination when task cancellation skips the normal explicit
kill-and-reap path.

```text
cargo test -p reticulum-rs-transport --lib \
  pipe_child_is_terminated_when_worker_task_is_aborted -- --nocapture  PASS (1 test)
```

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
TIMEOUT_SECS=60 tools/scripts/pipe-fake-subprocess-smoke.sh  PASS
cargo test -p reticulumd --test pipe_fake_subprocess_smoke_contract -- --nocapture  PASS
```

The PR-level Verify workflow now runs this software smoke and uploads its
report/logs as `pipe-fake-subprocess-<run-id>`. The pre-fix hosted run exposed
the daemon-shutdown child leak. After adding child kill-on-drop and a bounded
two-second process-exit poll, the updated local smoke passed three consecutive
runs; the updated-head hosted rerun is pending.

The daemon/peer loopback and clean teardown are Linux software evidence only.
Windows/macOS Pipe subprocess behavior, independent remote-peer interoperability,
and physical interface acceptance remain unverified. Physical testing tracked
separately under #616 is explicitly excluded.
