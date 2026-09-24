# Issue #614 PipeInterface software lifecycle evidence

This is a bounded software-only regression for the production `PipeInterface`
worker. It does not change the overall #614 status or establish physical
interface acceptance.

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
path. Windows/macOS subprocess behavior, daemon-level Pipe startup/shutdown,
packet exchange across a Pipe peer, and physical interface acceptance remain
unverified. Physical testing tracked separately under #616 is explicitly
excluded.
