# reticulum-rs-bluetooth

Protocol-neutral Bluetooth contracts, bounded owner-scoped events and generation-scoped
session admission. Import as `rns_bluetooth`; package name is `reticulum-rs-bluetooth`.
Rust edition 2024, minimum Rust 1.85. This leaf does not depend on SDK or network crates.

The default build has no native Bluetooth dependency. `bluez` selects Linux bluer 0.17.4;
`android` exposes the version gate for the first-party Android companion. Backend support
and acceptance are recorded in `docs/goals/shared-bluetooth/STATUS.md` in the repository.
A feature compiling is not evidence of a working backend or physical support.

The synchronous contracts run on I/O workers, not UI/async executor threads. Close and
cancel must remain callable while another operation waits. Owners supply limits and
deadlines; native adapters enforce them even if a caller abandons a wait. No automatic
connect/retry/replay, application fragmentation, peer identity or authorization is provided.
