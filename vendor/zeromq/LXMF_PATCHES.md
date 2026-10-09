# LXMF-rs ZeroMQ resource patch

Base: crates.io zeromq 0.6.0, MIT, original sources and license included.

The unchanged ZMTP wire implementation is used by the ZeroMQ SDK/daemon. Local
changes cap a frame and complete multipart at 16 MiB plus 64 KiB envelope overhead/32 parts before reserve;
cap all incomplete decoded frames at 128 MiB; cap active transport read halves
at 64; and retain at most 16 handshakes under each TCP accept owner. Excess
connections/frames close with a codec/socket error, so SDK mutations remain
Unknown and reconcile by operation ID. Dropping the accept owner closes its
uncompleted handshakes. No libzmq HWM option is assumed.

These are finite protocol/work bounds, not a process RSS guarantee. The runtime,
allocator and OS socket buffers need separate measurement in a paired soak.
