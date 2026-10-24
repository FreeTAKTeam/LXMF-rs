# Embedded local propagation

The `lxmf-propagation` library owns propagation SQL, payload normalization,
standard binary-ID/transfer-budget parsing, retention and the sender-only durable
store. Daemon adapters retain their existing writer, transactions, authentication,
peering/custody accounting and bounded inventory scanner. They delegate shared
storage operations instead of opening another connection or importing a daemon.

The embedded feature cannot import RPC dispatch or ZeroMQ; CI checks both its
feature tree and the native `lxmf-runtime` consumer. `LocalPropagation` in that
runtime uses the existing identity, transport, Link/Resource sender and lifecycle.
It starts no polling worker. One prepared signed wire and transient are reused
through direct attempts and local admission. Direct recovery reconstructs that
same signed wire from a sender-encrypted recovery blob using existing LXMF
identity encryption. This internal recovery blob is never sent as a Paper message.

The library-only `retrieve_for` API accepts an identity authenticated by the host's
native Link, derives its recipient destination, and rechecks current permission
against the original validated wire. It returns one recipient ciphertext within
the supplied payload budget, excluding expired/terminal entries and retaining
custody until proof. A runtime guard holds the shared transfer admission until
the host observes response completion or cancellation. The API registers no
endpoint, capability, announce or polling worker; optional phone-hosted retrieval
remains a separate, field-gated integration.

The 26,214,400-byte quota counts both recipient ciphertext and encrypted recovery
material. The seven-day admission deadline never refreshes on retries, uploads or
duplicate preparation; telemetry can expire earlier. Expiry precedes telemetry
coalescing/eviction, then oldest telemetry precedes other traffic, with a recipient
occupying at least half the configured capacity preferred within each class.
Expired and evicted entries retain visible terminal metadata. Status acknowledgement
hides a projected terminal notice but preserves its deduplication tombstone through
the original retention period. There are at most 4,096 active records and 8,192
active/status/tombstone records. Exhaustion refuses new admission rather than
silently dropping an unprojected notice. Projected tombstones age out after retention.

SQLite uses 4,096-byte pages, FULL synchronous rollback journaling and a database
page limit of `ceil((3 * payload_capacity + 8 MiB) / 4096)`. The factor allows the
legacy hex payload representation plus encrypted recovery bytes. Metadata has a
fixed row/field bound. The rollback journal is bounded by one before-image per
database page plus page headers, and is deleted at commit; no WAL can grow.
Retirement deletes payload/recovery and updates quota/state in one transaction.

Recovery has one attempt-scoped lease shared across direct and relay work. Its
transport guard releases ownership on completion or task cancellation. Callers
recheck current permission/sharing rules immediately before each later transmission.
A recipient proof binds both immutable message ID and transient ID. Resource/Link
completion alone does not prove a stock relay committed its payload: state becomes
`UploadedUnverified`, both encrypted copies remain, and direct recipient recovery
remains available. A restart during an upload has the same uncertain outcome and
suppresses blind relay re-upload; the copy remains until proof, expiry or eviction.

Local extraction evidence: 680 daemon binary tests and 238 daemon propagation
unit tests passed (one existing propagation test ignored); new storage tests cover
restart, quota and metadata saturation, actual SQLite-full rollback, stale attempt
completion, late proof/duplicate expiry, coalescing, and replay after projection.
Pinned Python baseline RNS `ea98db4f` and LXMF `727830ce` passed the existing
`propagated_rust_to_python` and `propagation_remote_download_rust_to_python`
smokes through the shared daemon store. These are host interop checks; REM bridge,
UI and physical LoRa acceptance remain separate gates.

The approved receiver-confirmed `/get` correction stops writing `transferred` or
`transfer_limited` marks when a response is constructed. Authenticated receiver
`haves` remain the retirement path. Existing historical marks are not rewritten;
older Rust relays need the correction before safe capped retrieval is used.
Schema, peering `/offer` and authenticated recipient filtering are unchanged.

`InProcessBackend::fetch_propagated_messages` is the daemon-free recipient client.
It uses native retained RequestReceipts, caps inventory and response Resources,
fetches one correlated transient at a time, validates the destination and sender
signature, and calls the host's durable importer. Failed or unproven imports
cannot send `haves`. Missing signer keys use bounded native path resolution;
otherwise the relay copy stays pending. The retained task holds host-supplied
admission through native cleanup after caller drop. Python `[]` and Rust `true`
confirmation responses are both supported. Existing REM response wrappers and
legacy destination-salt compatibility now live in compiled library code.

Current host validation includes 681 daemon tests, 50 shared/protocol/runtime
tests and the core wire suite. The explicit pinned-Python native-client test
proves failed import remains fetchable, a later durable save precedes `haves`,
and the stock relay then retires that copy. This test runs in the existing HIL
workflow. Source-size, feature isolation and strict workspace Clippy pass; none
of these checks substitutes for physical radio acceptance.
