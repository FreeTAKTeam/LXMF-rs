# Retain source identities referenced by detached tunnels (#663)

The retained-state restart gate found a received encrypted SDK message marked
`source_identity_unknown`. The saved announcement and public keys were valid:
restore loaded the identity from the detached tunnel, but periodic destination
cleanup removed it because the saved active route's dynamic interface was absent.
Inbound signature verification reads the transport destination map, so a durable
announce-identity database row did not prevent the failure.

The prior `74c019c3` full retained-state run passed its read-only identity proof:
100,000 distinct destinations across active and tunnel state, 99,997 retained
seed destinations, and 100,000 valid signed cached announces. That route/cache
proof did not establish source authentication after periodic cleanup. Its
20-minute regular pressure gate remains failed: checkpoint high-event deltas
were +6/0/0/0. A finite early episode and flat later checkpoints do not establish
hours-long stability or erase that strict local failure. No cache policy, memory
budget, deadline or acceptance predicate changes are part of this correction.

Cleanup now includes retained tunnel destinations in its existing reference set.
The iterator borrows the tunnel table and adds no second destination collection.
Normal cleanup expires stale tunnel paths before pruning identities. An identity
therefore survives while any tunnel references it and is released when the last
reference expires. Active routes, local destinations, prepared packet proofs and
link/backchannel references keep their existing behavior. Announcement validation,
signature verification, admission, custody, persistence ordering and timeouts are
unchanged.

The [evidence record](tunnel-identity-cleanup-663.json) includes a deterministic
signed-cache regression. It saves a valid announcement referenced by two tunnels,
restores without a mapped active interface, checks the exact public/verifying keys
after pruning and one tunnel removal, then verifies release after final expiry.
The old code fails at the post-prune identity lookup. All three destination-cache
tests pass with the correction, as do the four expired/malformed/missing/mismatched
cached-tunnel restore rejection tests. Scoped transport/daemon all-target,
all-feature Clippy, formatting and the include ratchet pass.

An independent local TCP control uses two real daemons, RCH and a scoped public-SDK
identity with empty histories. It drains scoped retransmissions, pauses only the
owned remote daemon through primary restart and 15 seconds of cleanup, then
resumes it before admitting a fresh bidirectional pair. The primary identity,
database, configuration and ports persist across restart; no scoped SDK
reannouncement, route deletion or cache rewrite occurs. The same driver asserts
an unmapped saved active route and a retained tunnel before sending:

| Check | Frozen `74c019c3` release | Corrected debug binary |
| --- | --- | --- |
| Active paths / retained tunnel paths before sending | 0 / 1 | 0 / 1 |
| Source identities after cleanup / removed delta | 0 / 1 | 1 / 0 |
| Persisted public/verifying keys unchanged | yes | yes |
| Fresh SDK-to-RCH signature | unknown, unchecked | verified, checked, valid |
| Fresh reverse signature | verified | verified |
| Unchanged 60-second authentication gate | fails | passes in 5.35 seconds |
| Owned daemon/RCH final exit codes | all zero | all zero |

The initially connected baseline control passed because automatic route recovery
retained an active path; it is preserved as a non-reproducing control. The asserted
detached comparison uses the same driver SHA, with only the daemon and declared
expected cleanup outcome changed. Full driver/binary hashes, row identifiers,
pre-send assertions and artifact hashes are recorded in the JSON.

Re-run the repository regression with
`cargo test -p reticulum-rs-transport --lib destination_map_prune` and the rejection
controls with
`cargo test -p reticulum-rs-transport --lib reticulum_tunnel_table_restore_skips`.
The local runtime driver and its frozen helper dependencies remain in the recorded
scratch artifacts. This establishes the focused native authentication behavior;
final release CI, retained-state resource qualification and multi-hour acceptance
remain separate gates. It does not establish physical-client interoperability.
