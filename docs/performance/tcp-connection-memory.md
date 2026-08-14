# TCP connection memory

This report isolates the user-space memory retained by accepted
`TcpServer`/`TcpClient` streams. It does not treat Tokio tasks as operating-system
threads and does not include kernel TCP buffer memory in process RSS.

## Baseline allocation audit

At the default TCP MTU of 262,144 bytes, the stream implementation at revision
`93ec0eac4065fe2f7953cbe5fee67cace73eb4b4` requests the following buffers for
each accepted connection:

| Buffer | Requested bytes | Lifetime and commitment |
| --- | ---: | --- |
| decoded HDLC receive buffer | 262,144 | Allocated when the RX task starts; pages are touched as decoded output is written. |
| HDLC reassembly capacity | 1,048,576 | Reserved when the RX task starts by `Vec::with_capacity`; logical length and committed pages grow with received data. |
| TCP read buffer | 4,194,304 | Allocated when the RX task starts; the kernel writes into pages covered by each read. |
| encoded HDLC transmit buffer | 524,304 | Allocated before the TX task waits for work and reallocated on every TX-loop iteration. |
| raw packet transmit buffer | 262,144 | Allocated with the encoded buffer and reallocated on every TX-loop iteration. |
| **total requested buffer storage** | **6,291,472 (6.00 MiB)** | Excludes allocator metadata, tasks, channels, packet queues and socket state. |

The malformed-input limit is 1,048,608 bytes: twice the worst-case encoded
wire length for one MTU-sized frame. The `mtu * 16` TCP read buffer is not used
as a protocol limit. HDLC reassembly already spans reads, and no test or
protocol rule requires a single read to contain sixteen MTUs.

Each live accepted stream also owns an interface task, RX task, TX task and
status task. It has a 128-entry bounded TX channel and shares the transport's
128-entry RX channel. Empty Tokio channels do not contain packet payloads, but
queued `TxMessage` values do: `PacketDataBuffer` is a `Vec<u8>`, so broadcast
`message.clone()` currently copies packet payload data once per selected
interface. This fan-out issue is separate from the fixed per-connection stream
buffers.

Kernel send/receive buffers, TCP control structures and file-descriptor tables
are charged by the operating system and are not visible in process RSS. Their
sizes depend on platform defaults, autotuning and traffic history.

## Repeatable measurement

Build and run the release-profile scenarios with:

```sh
python3 tools/scripts/tcp_connection_memory_benchmark.py \
  --counts 100,500,1000 \
  --activities idle,small \
  --json-out artifacts/tcp-connection-memory.json
```

The runner starts a fresh process for every count/activity pair. On Linux it
records `VmRSS`, `VmHWM` and `VmSize` from `/proc/self/status`; on every platform
it records Tokio live-task counts, transferred packets/bytes, elapsed time, CPU
ticks where available, and broadcast dispatch failures. Client sockets live in
the same process, so their small and consistent overhead is included in both
baseline and optimized measurements.

Measurements below use the release profile on the same host for both builds:
Linux 7.0.0-28-generic x86_64, rustc 1.96.0. The client sockets are in the
benchmark process, so slopes include the client-side Tokio socket objects as
well as the accepted server interfaces.

## Results

The idle figures are sampled after a 300 ms settle period. A slope is
`(connected - pre-connect baseline) / connections`.

| Idle connections | Baseline RSS slope | Optimized RSS slope | Baseline `VmSize` slope | Optimized `VmSize` slope | Tokio tasks |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 100 | 35.84 KiB/conn | 28.76 KiB/conn | 6,165.56 KiB/conn | 1.56 KiB/conn | 403 |
| 500 | 33.46 KiB/conn | 25.82 KiB/conn | 6,164.98 KiB/conn | 0.98 KiB/conn | 2,003 |
| 1,000 | 33.09 KiB/conn | 25.34 KiB/conn | 6,164.97 KiB/conn | 0.96 KiB/conn | 4,003 |

At 1,000 idle connections, RSS falls from 37,268 KiB to 29,520 KiB. More
importantly, the connection-driven `VmSize` increase falls from 6,164,968 KiB
to 960 KiB. RSS was already much lower than the requested buffer total because
zero-filled and capacity-only allocations do not commit every page
immediately. `VmSize` exposes the address-space reservation that RSS alone
misses.

After one 85-byte wire frame is received from every connection, the 1,000
connection RSS slope is 33.09 KiB before and 25.56 KiB after. Lazy buffer growth
therefore does not recreate the old fixed reservation for normal packets.

Maximum-frame measurements use one 262,146-byte HDLC frame per connection:

| 100 maximum frames | Baseline | Optimized |
| --- | ---: | ---: |
| Peak RSS slope | 715.56 KiB/conn | 713.24 KiB/conn |
| Post-transfer `VmSize` slope | 6,168.12 KiB/conn | 416.76 KiB/conn |
| Throughput | 5,118 packets/s | 5,373 packets/s |
| Aggregate process CPU ticks | 11 | 19 |

The maximum-frame RSS peak is intentionally not eliminated: receiving a real
maximum frame must commit reassembly and decode storage. The optimized build
stops retaining unrelated maximum-size TCP and TX buffers. CPU ticks are Linux
process ticks and are coarse for this short, parallel workload; maximum-frame
growth performs allocation work that the baseline paid at connection startup.

For normal 64-byte payloads at 1,000 connections, the same zero-settle run
reported 60,849 packets/s before and 58,509 packets/s after, with one aggregate
CPU tick in both runs. The 3.8% difference is within the scheduling resolution
of this short loopback test; longer application benchmarks should be used for
capacity planning.

A single 64-byte broadcast with 100 accepted clients matched and enqueued to
all 101 selected interfaces with zero enqueue failures. Use `--broadcasts N`
to exercise queue saturation; the JSON reports matched, sent and failed
interface counts.

## Allocation changes

An idle accepted connection now requests 32 KiB for TCP reads and an initial
4 KiB HDLC reassembly capacity. The decoded RX buffer and both TX buffers start
empty, reducing direct idle buffer requests from 6.00 MiB to about 36 KiB.

- The TCP read buffer is independent of MTU. TCP streaming and HDLC reassembly
  preserve frames split across reads and multiple frames in one read.
- HDLC reassembly starts at 4 KiB and grows as encoded bytes arrive. Its
  malformed-stream limit remains derived from the MTU and worst-case HDLC
  escaping. A partial frame after the last flag is retained only while it can
  still be valid.
- The decoded buffer grows only when a complete frame is found and remains
  capped by the configured MTU.
- TX raw and HDLC buffers are allocated on the first outgoing packet, sized
  from that packet, and retained for reuse. Only the newly encoded logical
  slice is written, so cancellation, failed writes and smaller later packets
  cannot expose stale bytes.
- Dispatch length checks use `Packet::serialized_len()` instead of allocating
  a serialized packet solely to measure it.

`Vec` growth can temporarily hold the old and new allocation while a large
reassembly buffer is moved. That temporary peak occurs only after receiving a
large partial frame; normal packets remain within the initial capacity. The
logical malformed backlog is bounded and is trimmed after each 32 KiB read.

## Remaining limits

- Accepted streams still use four Tokio tasks each. This is not an OS
  thread-per-connection model, but task state and scheduling remain linear in
  the client count.
- Each client retains a 128-entry TX channel. Queued packets consume memory,
  and slow readers can fill queues and kernel send buffers.
- `PacketDataBuffer` is a `Vec<u8>`. `InterfaceManager` broadcast
  `message.clone()` therefore copies payload storage once per target interface.
  Converting packet ownership to shared immutable storage is a separate,
  routing-wide change and was not mixed into this buffer patch.
- `InterfaceManager` locking and sequential fan-out work grow with the number
  of interfaces even when payload copies are small.
- Kernel socket buffers and TCP control blocks are outside process RSS and are
  controlled by each operating system. File-descriptor limits remain an
  independent ceiling on Linux and macOS; Windows has its own handle and socket
  resource limits.
- No client limit was added. Current behavior remains unlimited, and runtime
  status exposes cumulative accepted connections rather than active/rejected
  counts. Admission control can be added separately as optional protection; it
  is not a substitute for the allocation reduction.

## Validation performed

The following commands passed on the measured Linux host:

```sh
cargo fmt --all -- --check
cargo clippy -p reticulum-rs-transport --all-targets --all-features \
  --no-deps -- -D warnings
cargo test -p reticulum-rs-transport --no-default-features
cargo test -p reticulumd --test code_quality_issue_369
cargo test -p reticulumd \
  --test backbone_python_channel_interop_contract \
  --test backbone_selector_backpressure_probe_contract
tools/scripts/check-boundaries.sh
tools/scripts/check-module-size.sh
```

The transport suite passed 636 library tests plus all package integration
tests. Live TCP and Backbone channel round trips passed in both directions
against the local Python Reticulum 1.3.8 checkout:

```sh
RETICULUM_PY_REPO=../Reticulum \
  cargo test -p reticulumd --test python_channel_interop \
  backbone_channel_roundtrip -- --ignored --nocapture
RETICULUM_PY_REPO=../Reticulum \
  cargo test -p reticulumd --test python_channel_interop \
  _to_rust_channel_roundtrip -- --ignored --nocapture
RETICULUM_PY_REPO=../Reticulum \
  cargo test -p reticulumd --test python_channel_interop \
  rust_to_python_channel_roundtrip -- --ignored --exact --nocapture
```

Cross-target checks passed without platform-specific event APIs. Zig was used
only to compile bundled C dependencies while checking from Linux:

```sh
env CC_x86_64_pc_windows_gnu='zig cc -target x86_64-windows-gnu' \
  AR_x86_64_pc_windows_gnu='zig ar' CRATE_CC_NO_DEFAULTS=1 \
  cargo check -p reticulum-rs-transport --lib --no-default-features \
  --target x86_64-pc-windows-gnu
env CC_x86_64_apple_darwin='zig cc -target x86_64-macos' \
  AR_x86_64_apple_darwin='zig ar' CRATE_CC_NO_DEFAULTS=1 \
  cargo check -p reticulum-rs-transport --lib --no-default-features \
  --target x86_64-apple-darwin
```
