# LXMF-rs v0.16.0

This patch release aligns the daemon, SDK and 21 public packages on version
0.16.0. It includes reliability and resource-management changes merged since
v0.15.0.

## Changes

- Announce-cache persistence uses timer-driven dirty saves and bounded cleanup;
  packet cache cleanup preserves referenced packets.
- Native delivery receipt handling retains packet and Link bindings.
- ZeroMQ readiness reflects control-plane availability; timeout recovery and
  failure diagnostics distinguish stages of a request.
- WAL maintenance uses a dedicated connection and runs when the file reaches
  its threshold. Additional metrics expose maintenance and persistence activity.
- Propagation retention work is paged and bounded. Tokio is updated to 1.53.3.

## Upgrade and limitations

All public Rust packages share version 0.16.0. Rust 1.88 remains the workspace
minimum; the Bluetooth leaf retains Rust 1.85. Existing durable-broker backup,
restore and downgrade requirements in the [v0.14.0 migration guide](migrations/v0.14.0-zmq.md)
continue to apply. Preserve existing identities, configuration and databases
when replacing a daemon.

The fixes do not establish production resource stability, multi-hour delivery
or multi-cycle retention acceptance. Issues [#663](https://github.com/FreeTAKTeam/LXMF-rs/issues/663)
and [#678](https://github.com/FreeTAKTeam/LXMF-rs/issues/678) retain their
outstanding operational acceptance criteria. Hardware and public-network
validation remain separate from software CI.
