# Link Channel Parity Notes

Last updated: 2026-03-18

This note records the active `rns-transport` channel behavior implemented on the
`codex/live-channel-integration` branch while bringing Rust closer to Python
Reticulum `RNS/Channel.py` and `RNS/Link.py`.

It is intentionally narrower than the full compatibility issue list. The purpose
is to document the live transport/channel invariants that are easy to lose while
iterating.

## Implemented Invariants

### Channel open/proof gating

- `PacketContext::Channel` is only acknowledged with a link proof if the receiving
  link has an open channel consumer.
- In practice, the Rust implementation currently treats registered channel
  handlers as the “open channel” signal.
- Once a channel consumer is open, Rust now proves channel packets before decode
  and frame acceptance, matching Python’s retry behavior more closely.
- Duplicate, out-of-order, or malformed channel frames therefore do not suppress
  the transport proof that the sender is waiting for.
- Links can now hold explicit channel-open state even before any handler is
  attached, which is closer to Python’s `get_channel()` semantics.
- Channel packets without a registered consumer are dropped from the channel path
  and are not proved.

### Channel receive path

- Channel frames are not forwarded onto the generic `LinkEvent::Data` /
  `received_data` stream.
- Channel receive handling is isolated from the generic link payload path and is
  closer to Python `Link` behavior, where channel traffic is consumed by the link
  channel object rather than packet callbacks.
- Duplicate channel frames are ignored.
- Out-of-order channel frames are buffered and delivered contiguously.
- Invalid channel frames are rejected without crashing link processing.
- Channel handler panics are contained and logged instead of unwinding the link
  receive path.
- Channel handlers are ordered and short-circuit on the first handler that
  returns `true`.
- Channel handlers can be removed explicitly, which is required for buffer-like
  consumers that attach and detach over the lifetime of a link.
- Typed channel registrations now reject Python-reserved system message ids by
  default unless the message type explicitly opts into system-message use.

### Channel send path

- Channel sends use the bound link ingress iface directly once the link is active.
- The send path no longer depends on generic route lookup for transient link ids.
- A failed direct dispatch marks the channel message failed immediately.

### Channel delivery state

- Channel messages are tracked as `Sent`, `Delivered`, or `Failed` on the owning
  `Link`.
- A valid incoming `LinkProof` marks the matching channel sequence delivered.
- Link close/restart transitions all pending channel messages to `Failed`.

### Channel retry and timeout behavior

- Pending channel packets carry retry metadata on the `Link`.
- The existing transport link-maintenance loop polls channel retry deadlines.
- The maintenance loop now wakes earlier when a channel retry deadline is sooner
  than the normal one-second sweep.
- Timed-out channel packets are retransmitted directly on the bound iface.
- Retry exhaustion fails all pending channel messages and closes the link.
- Retry timeout uses the Python-style shape:
  - base timeout derived from `max(rtt * 2.5, 0.025)`
  - exponential factor `1.5^(tries-1)`
  - scaled by outstanding channel work

### Channel flow control

- Channel send readiness is no longer a fixed `1`/`2` rule only.
- The link now owns a channel send window that:
  - starts from the Python slow/non-slow profile
  - grows on successful channel delivery
  - shrinks on retry timeout
- Slow links still start at a single outstanding channel slot.
- Non-slow links start at two outstanding channel slots.

## Current Rust Approximation

The current implementation is intentionally narrower than Python’s full
`Channel` object:

- Rust uses link-owned handler registration and sequence tracking rather than a
  standalone `Channel` object obtained from `Link.get_channel()`.
- Rust implements direct send, delivery tracking, receive ordering, duplicate
  suppression, retry scheduling, and window adjustment.
- Rust does not yet expose the richer Python message-factory API or the full
  adaptive window promotion behavior expected from sustained medium/fast rounds.
- Rust still models “channel open” as “at least one registered handler”, whereas
  Python models it as the existence of the link-owned `Channel` object itself.
  Rust now has an explicit open/close channel state, but it is still driven by
  the transport handle rather than a persistent link-owned `Channel` object.
- Rust still does not expose a real `StreamDataMessage` / buffer layer equivalent
  to Python `RNS.Buffer`.

## Remaining Gaps

- A first-class public channel API on `Link`/`Transport` that more closely
  matches Python `Channel`.
- Full adaptive window promotion semantics for medium/fast sustained links.
- A clearer mapping between channel delivery callbacks and higher-level SDK/daemon
  semantics.
- Cross-process or daemon-visible channel abstractions if channel traffic becomes
  part of the public RPC surface.
- A fuller Rust equivalent of Python `RNS.Buffer`; current work has only laid the
  `StreamDataMessage` / raw reader-writer foundation.

## Buffer Foundation

- Rust now has a `channel_buffer` module with:
  - `StreamDataMessage`
  - `RawChannelReader`
  - `RawChannelWriter`
  - `Buffer::create_reader()`
  - `Buffer::create_reader_with_callback()`
  - `Buffer::create_writer()`
  - `Buffer::create_bidirectional_buffer()`
  - `Buffer::create_bidirectional_buffer_with_callback()`
- This is intentionally narrower than Python `RNS.Buffer`:
  - it provides the message format, attachable async reader, and chunking writer
  - it does not yet provide a full `std::io`-style buffered stream facade
  - raw writer backpressure now behaves like Python `RawChannelWriter.write()`: it returns a
    short write of `0` on `LinkNotReady` instead of surfacing a hard error
  - raw writer close is best-effort like Python: it waits briefly for send readiness, then does a
    single EOF send attempt and does not guarantee full drain

## Why This Note Exists

The channel work is easy to regress because several choices are subtle but
important:

- channel packets should not be acknowledged unless there is an active channel
  consumer;
- channel traffic should not silently leak back into the generic payload stream;
- direct link traffic should not rely on route-table behavior meant for ordinary
  single-destination packets;
- retry and window behavior should be defensible relative to the Python
  reference, not ad hoc.

If later refactors change any of those rules, update this note in the same PR.
