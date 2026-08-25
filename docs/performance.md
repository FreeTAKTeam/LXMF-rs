# Performance

<!-- GENERATED: tools/scripts/performance_docs.py -->

Dataset: [`docs/performance/v0.10.0.json`](performance/v0.10.0.json). All numbers below originate from release SHA `5436ee715f94f81e18abb0808cfca52fcd7cc9bc`.
The standalone release dashboard is available at [the latest GitHub Release asset](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest/download/lxmf-rs-performance.html); the release asset is the public source for the complete matrix.

## Methodology

The report uses `5` interleaved comparison runs and `2` isolated resource runs. Fixtures and process setup are completed before timed regions. Results are medians; p95 and p99 retain tail visibility. Rust/Python ranking is evidence, not a release threshold.

## Environment

- Timestamp: `2026-08-25T01:33:45Z`
- Release SHA: `5436ee715f94f81e18abb0808cfca52fcd7cc9bc`
- Python Reticulum: `e32d4df754a7b87b1bf1bb0d08675d12ff505ae6`
- Python LXMF: `727830cefda83d9c6e3982b48675425f3f988f9c`
- Rust: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- Python: `Python 3.12.14`
- CPU: `AMD EPYC 7763 64-Core Processor`
- OS/kernel: `Linux runnervm76f27 6.17.0-1022-azure #22-Ubuntu SMP Mon Jul 27 17:24:03 UTC 2026 x86_64 x86_64 x86_64 GNU/Linux`
- Profile: `report`

## Protocol/core and transport hot paths

| Workload | Class | Payload | Batch | Rust p50 | Python p50 | Rust/Python | Rust variability | Python variability | Rust RSS | Python RSS |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| LXMF message decode | protocol_core | 21 | 1 | 225 ns | 7.87 ms | 35029.62x | 0.44% | 0.35% | 37.8 MiB | 31.3 MiB |
| LXMF message encode | protocol_core | 12 | 1 | 386 ns | 2.10 ms | 5432.72x | 0.41% | 0.13% | 24.0 MiB | 30.8 MiB |
| LXMF large message decode | protocol_core | 2048 | 1 | 378 ns | 7.93 ms | 20991.57x | 2.12% | 0.78% | 24.8 MiB | 31.3 MiB |
| LXMF large message encode | protocol_core | 2048 | 1 | 761 ns | 2.12 ms | 2783.14x | 2.90% | 0.17% | 15.5 MiB | 30.7 MiB |
| LXMF resource-sized message decode | protocol_core | 16384 | 1 | 1.19 us | 7.99 ms | 6737.40x | 2.25% | 0.52% | 11.8 MiB | 31.3 MiB |
| LXMF resource-sized message encode | protocol_core | 16384 | 1 | 13.48 us | 2.17 ms | 161.24x | 4.60% | 0.13% | 6.1 MiB | 30.9 MiB |
| Reticulum packet pack | protocol_core | 128 | 1 | 24 ns | 4.02 us | 170.78x | 0.62% | 0.75% | 313.5 MiB | 36.7 MiB |
| Reticulum packet unpack | protocol_core | 128 | 1 | 47 ns | 2.35 us | 49.81x | 0.98% | 0.42% | 158.6 MiB | 40.8 MiB |
| Reticulum resource segmentation 16 KiB | transport_hotpath | 16384 | 43 | 2.26 us | 6.41 us | 2.83x | 6.21% | 1.25% | 9.0 MiB | 34.6 MiB |
| Reticulum resource reassembly 16 KiB | transport_hotpath | 16384 | 43 | 376 ns | 921 ns | 2.45x | 3.97% | 0.98% | 24.9 MiB | 55.6 MiB |
| Reticulum announce create | protocol_core | 22 | 1 | 20.34 us | 2.10 ms | 103.42x | 0.24% | 0.07% | 6.4 MiB | 31.3 MiB |
| Reticulum announce validate | protocol_core | 22 | 1 | 49.30 us | 7.88 ms | 159.83x | 1.45% | 0.67% | 6.1 MiB | 31.6 MiB |
| Reticulum announce validate batch 64 | protocol_core | 22 | 64 | 3.21 ms | 504.74 ms | 157.28x | 1.43% | 0.14% | 5.9 MiB | 31.2 MiB |
| Reticulum identity sign | protocol_core | 2048 | 1 | 26.92 us | 2.09 ms | 77.45x | 0.07% | 0.40% | 5.9 MiB | 30.9 MiB |
| Reticulum identity verify | protocol_core | 2048 | 1 | 47.34 us | 7.90 ms | 166.95x | 0.03% | 0.17% | 6.0 MiB | 31.1 MiB |
| Reticulum identity encrypt | protocol_core | 2048 | 1 | 82.97 us | 15.45 ms | 186.17x | 0.21% | 0.23% | 5.7 MiB | 31.0 MiB |
| Reticulum identity decrypt | protocol_core | 2048 | 1 | 61.82 us | 17.52 ms | 283.48x | 0.05% | 0.39% | 5.9 MiB | 30.9 MiB |
| Reticulum resource request window | transport_hotpath | - | 6 | 30 ns | 4.10 us | 136.28x | 0.11% | 1.20% | 248.2 MiB | 36.6 MiB |

## Rust SDK transport comparison

In-process latency is normalized per call from fixed 100-call batches to avoid timer-resolution noise; ZeroMQ, HTTP, and Unix measurements time individual daemon requests.

| Operation | In-process p50 | ZeroMQ p50 | HTTP p50 | Unix p50 | ZeroMQ/HTTP | ZeroMQ/Unix |
|---|---:|---:|---:|---:|---:|---:|
| negotiate | 1.22 us | 159.69 us | 274.01 us | 210.23 us | 1.72x | 1.32x |
| snapshot | 552 ns | 168.07 us | 250.38 us | 158.02 us | 1.49x | 0.94x |
| status | 31 ns | 125.20 us | 220.67 us | 152.92 us | 1.76x | 1.22x |
| poll_events | 446 ns | 99.71 us | 197.10 us | 107.24 us | 1.98x | 1.08x |
| operation_registry | UNSUPPORTED | 895.35 us | 1.13 ms | 1.06 ms | 1.26x | 1.18x |
| router_stats | 21 ns | 115.57 us | 222.88 us | 112.95 us | 1.93x | 0.98x |

## Same-topology end-to-end comparison

These matched sender workloads use the same two-node loopback TCP topology with one Rust and one pinned-Python endpoint. Startup and route warm-up are outside the timed enqueue-to-receiver-evidence boundary.

| Workload | Route | Payload | Rust p50 | Python p50 | Rust/Python | Rust p95 | Python p95 | Rust CPU | Python CPU | Rust RSS | Python RSS | Rust variability | Python variability |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Loopback TCP cold destination discovery | cold | 0 | 388.88 ms | 2650.36 ms | 6.82x | 396.35 ms | 2651.77 ms | 2.500s | 2.090s | 82.3 MiB | 82.2 MiB | 0.23% | 0.04% |
| Loopback TCP link setup | warm | 0 | 172.83 ms | 137.36 ms | 0.79x | 174.41 ms | 137.81 ms | 2.270s | 1.810s | 82.1 MiB | 82.3 MiB | 0.11% | 0.29% |
| Loopback TCP direct delivery | warm | 256 | 705.16 ms | 449.66 ms | 0.64x | 710.57 ms | 450.71 ms | 6.400s | 2.050s | 82.2 MiB | 82.1 MiB | 0.12% | 0.20% |
| Loopback TCP opportunistic delivery | warm | 256 | 693.42 ms | 449.03 ms | 0.65x | 699.36 ms | 450.15 ms | 5.810s | 2.030s | 82.3 MiB | 82.5 MiB | 0.28% | 0.06% |
| Loopback TCP propagated delivery | warm | 256 | 849.61 ms | 7863.14 ms | 9.26x | 858.82 ms | 11863.17 ms | 4.340s | 7.320s | 82.2 MiB | 82.1 MiB | 0.15% | 0.02% |
| Loopback TCP resource delivery | warm | 16384 | 698.16 ms | 1020.01 ms | 1.46x | 700.86 ms | 1033.03 ms | 6.430s | 2.620s | 82.1 MiB | 82.4 MiB | 0.39% | 0.13% |

## Independent rns-rs network comparison

The pinned rns-rs peer and LXMF-rs ran as independent processes on the same runner. The recorded peer SHA is `6c6d79b83516feff271d15c97d39dd1de7798afe`; same-runner evidence is `True`.

| Workload | rns-rs / LXMF-rs evidence | p50 | p99 | Variation |
|---|---|---:|---:|---:|
| Cold path convergence | rns-rs requester | 0.001 s | 0.001 s | 3.85% |
| Warm path lookup | rns-rs cache | 0.000542 s | 0.000561 s | 1.56% |
| Link establishment | rns-rs initiator -> LXMF-rs | 0.002 s | 0.103 s | 4.37% |
| Exact 1 MiB Resource | lxmf_rs sender, SHA `cba3982ca4b9` | 0.191 s | 0.195 s | 1.15% |
| Exact 1 MiB Resource | rns_rs sender, SHA `cba3982ca4b9` | 0.202 s | 0.202 s | 0.02% |
| Exact 50 MiB Resource | lxmf_rs sender, SHA `649936cc2358` | 7.703 s | 7.805 s | 0.02% |
| Exact 50 MiB Resource | rns_rs sender, SHA `649936cc2358` | 7.216 s | 7.417 s | 0.03% |

Exactly 1000 active Links: **UNSUPPORTED** — pinned rns-rs public create_link surface did not complete exactly 1000 real Links within the bounded 300-second workload; no smaller count substituted.

## 100-node chain scale tests

Exploratory single-host scale results are stored in [`docs/performance/100-node-chain-2026-07-20.json`](performance/100-node-chain-2026-07-20.json). Each run created `100` nodes in a linear chain over `99` simulated media at `1` Mbit/s, a `500`-byte MTU, `1` ms propagation per medium, and `0.0%` configured loss. The `98` interior nodes acted as transports.

After a `60`-second route warm-up, the endpoints sent `3` concurrent `256`-byte opportunistic messages in each direction. RTT columns report p50 across delivered samples; a dash means no sample was delivered. Readiness required all 100 nodes to be running, connected, and addressed.

| Composition | Ready | n0 -> n99 p50 | n99 -> n0 p50 | Delivered | Media TX | Result |
|---|---:|---:|---:|---:|---:|---:|
| 100 Python | 1.761 s | 25.180 s (Python -> Python) | - (Python -> Python) | 3/6 | 35,662 | fail |
| 50 Python / 50 Rust | 1.517 s | 0.456 s (Python -> Rust) | 0.770 s (Rust -> Python) | 6/6 | 4,411 | pass |
| 100 Rust | 0.253 s | 0.112 s (Rust -> Rust) | 0.192 s (Rust -> Rust) | 6/6 | 1,368 | pass |

The all-Python reverse direction delivered `0/3` samples, logged `SENDFAIL noidentity`, and reached the `121`-second action timeout; the missing RTT is not zero.

These are single runs per composition, not a repeated benchmark distribution. The simulator and all nodes shared one host, so scheduler and simulator overhead affect the measurements. The Rust binary came from LXMF-rs `f6f8407f0645ec251efdb9dc37149aaea78ce8e9`; the Python references were Reticulum `15320e4d2cfabb143c1db20ca887e275fd521585` and LXMF `727830cefda83d9c6e3982b48675425f3f988f9c`. The reticulated harness working tree contained uncommitted changes, so these results are exploratory evidence rather than a release threshold.

## Limitations

- Scheduler noise, CPU frequency changes, and host background work affect tails and resource readings.
- Cryptographic workloads include randomness where the implementation requires it; fixture construction remains outside timed regions.
- Python wins must be reported without suppression. Ratios below 1.0 mean Python was faster.
- Hardware, public-network, and human-operated workflows are intentionally deferred to v1.0 and are not represented here.

## Reproduce

```bash
cargo xtask python-impl-bench-report
python3 tools/scripts/e2e_performance.py --profile report
python3 tools/scripts/independent_performance.py --samples 5 --links 1000
python3 tools/scripts/performance_docs.py --release v0.10.0 --report target/criterion/python-impl-report/report.json
python3 tools/scripts/performance_docs.py --check
```
