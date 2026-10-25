# LXMF-rs v0.15.0

v0.15.0 aligns the daemon, SDK and 21 public Rust packages on one release
train. It includes the LXMF 1.2.0 delivery work, interface lifecycle fixes,
Bluetooth foundations, bare-metal core support, storage improvements and
embedded offline custody merged since v0.14.0.

## Changes

- Delivery ownership covers retained Links and Resources, retries, receipts
  and propagation custody, with compatibility tests against pinned Python
  references. Custody admission remains distinct from recipient delivery.
- Native interfaces retain their ownership and teardown state. Fernet
  decrypt-only paths no longer require an operating-system random source;
  the core also builds for the supported bare-metal targets.
- `reticulum-rs-bluetooth` supplies protocol-neutral Bluetooth types and
  feature-isolated platform adapters. Physical carrier qualification remains
  specific to the recorded hardware and backend.
- `lxmf-propagation` shares storage, validation, retention and standard
  propagation handlers between daemon and embedded consumers. Its owned
  outbox keeps signed recovery material and bounded retention. R3AKT mission
  classification belongs to R3AKTClient; generic storage exposes
  `MessageKind::Application`, retaining persisted discriminator `1`.
- Propagation refill coalescing, writer fairness and timer-driven WAL
  maintenance reduce redundant storage work. Additional operation, memory,
  WAL and persistence metrics support the ongoing resource investigation.
- MeshChat's partial API includes saved interface and transport controls.
  This does not promote the proof of concept to full application parity.
- Publication includes both new crates in dependency order. The Python
  reference-pin validator accepts the paired Reticulum/LXMF README wording
  while continuing to reject a changed reference revision.
- The `rnx` direct-delivery smoke check recognizes proof-confirmed `delivered`
  from the retained Link owner, while rejecting another mode's nonterminal handoff.

## Upgrade

All 21 public packages, including the patched `reticulum-rs-zeromq`, use
version `0.15.0`. Rust 1.88 or newer remains required; the independent
Bluetooth leaf retains its Rust 1.85 contract. Reference revisions and
network wire formats remain at their existing tested boundaries.

For the custody API rename and ownership boundary, see
[application custody classification](migrations/propagation-application-kind.md).
Existing stored kind-1 records retain their state, expiry and signed-wire
recovery without migration or deletion. The durable-broker backup, restore
and downgrade requirements from [v0.14.0](migrations/v0.14.0-zmq.md) still apply.

## Qualification and known limitations

This release is scoped to software validation and published artifacts. The
release record links the exact-commit CI, Python/virtual HIL, independent
interoperability, performance, package and platform evidence. These checks
do not establish production resource stability or new physical-radio acceptance.

- [#678](https://github.com/FreeTAKTeam/LXMF-rs/issues/678) remains open:
  announce-cache cleanup is not wired into daemon persistence, and cache
  accumulation has exhausted a production filesystem. Operators must monitor
  cache growth and available disk space; this release does not repair it.
- [#663](https://github.com/FreeTAKTeam/LXMF-rs/issues/663) remains open:
  anonymous-memory attribution and bounds, poll-failure correlation and
  multi-hour production-shaped acceptance are incomplete. The existing
  ten-minute resource runs are smoke evidence, not a stable memory plateau.
- The complete hardware, third-party-client and public-network matrices
  remain separate operational evidence tracks. Optional native signing and
  Homebrew publication depend on the configured release credentials.

[Changes since v0.14.0](https://github.com/FreeTAKTeam/LXMF-rs/compare/v0.14.0...v0.15.0).
