# Preserve the final route checkpoint before interface retirement (#663)

Retained-state restart testing exposed a separate daemon lifecycle defect:
configured interface shutdown removed the interface identities used by path
export before the final path-table flush. A healthy in-memory route could
therefore be written as an empty `destination_table` on graceful shutdown.
This invalidated restart qualification even though the ordinary checkpoint
encoder and cached announce remained valid.

Both daemon shutdown branches now finish persistence before destructive
interface retirement. A context-owned asynchronous semaphore serializes regular
saves, cleanup and the final checkpoint; a shared close flag fences delayed
background work after that checkpoint. The regular worker exits instead of
later overwriting the saved paths using the retired interface map. No blocking
mutex is held across file I/O. Existing packet-before-metadata ordering and
reference-aware cleanup remain inside the transport persistence owner.

The boundary is a consistent snapshot before interface retirement, not a promise
that native arrivals after snapshot capture appear in the final route cache.
There is no new transport-quiescence API, file-rename atomicity, custody meaning,
retained-history limit or deadline change. Existing persistence errors remain
logged according to the existing shutdown contract.

The [recorded regression evidence](path-shutdown-checkpoint-663.json) uses a real
daemon with a named, enabled UDP interface and a valid signed announce. It checks
the learned route through RPC, sends SIGINT, checks the Python-compatible saved
route/interface hash and cached packet, then starts a second daemon with the same
identity/configuration and verifies restoration before sending another announce.
A frozen baseline binary failed because the saved table was `Array([])`; the
candidate passes. The four process replay/restart cases pass both with default
features and without ZeroMQ. A queued-save unit regression waits for a dirty
trigger to be consumed, finishes persistence, retires the interface and verifies
that queued saves/cleanup cannot overwrite the checkpoint.

Focused validation also preserves the existing cache maintenance, shutdown
failure reporting, packet replay and durable custody tests. This establishes
correctness for the owned process regression; a rebuilt retained 100k-route
qualification run still needs to demonstrate the combined memory/restart result.
