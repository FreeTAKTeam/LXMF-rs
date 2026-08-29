# Performance

<!-- GENERATED: tools/scripts/performance_docs.py -->

Dataset: [`docs/performance/v0.10.1.json`](performance/v0.10.1.json). All numbers below originate from release SHA `25a976945cb335dff3be692981151c8741a5fdeb`.
The standalone release dashboard is available at [the latest GitHub Release asset](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest/download/lxmf-rs-performance.html); the release asset is the public source for the complete matrix.

## Methodology

The report uses `5` interleaved comparison runs and `2` isolated resource runs. Fixtures and process setup are completed before timed regions. Results are medians; p95 and p99 retain tail visibility. Rust/Python ranking is evidence, not a release threshold.

## Environment

- Timestamp: `2026-08-29T13:52:47Z`
- Release SHA: `25a976945cb335dff3be692981151c8741a5fdeb`
- Python Reticulum: `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`
- Python LXMF: `727830cefda83d9c6e3982b48675425f3f988f9c`
- Rust: `rustc 1.98.0 (88d9e12ae 2026-08-18)`
- Python: `Python 3.12.14`
- CPU: `AMD EPYC 7763 64-Core Processor`
- OS/kernel: `Linux runnervmgx7h7 6.17.0-1022-azure #22-Ubuntu SMP Mon Jul 27 17:24:03 UTC 2026 x86_64 x86_64 x86_64 GNU/Linux`
- Profile: `report`

## Protocol/core and transport hot paths

| Workload | Class | Payload | Batch | Rust p50 | Python p50 | Rust/Python | Rust variability | Python variability | Rust RSS | Python RSS |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| LXMF message decode | protocol_core | 21 | 1 | 225 ns | 7.99 ms | 35459.28x | 0.95% | 0.62% | 37.6 MiB | 31.4 MiB |
| LXMF message encode | protocol_core | 12 | 1 | 385 ns | 2.10 ms | 5450.73x | 0.30% | 0.35% | 24.1 MiB | 31.0 MiB |
| LXMF large message decode | protocol_core | 2048 | 1 | 370 ns | 7.96 ms | 21536.64x | 2.22% | 0.10% | 25.0 MiB | 31.0 MiB |
| LXMF large message encode | protocol_core | 2048 | 1 | 773 ns | 2.12 ms | 2743.59x | 2.71% | 0.34% | 15.2 MiB | 31.0 MiB |
| LXMF resource-sized message decode | protocol_core | 16384 | 1 | 6.40 us | 8.04 ms | 1256.77x | 5.72% | 0.26% | 6.5 MiB | 31.0 MiB |
| LXMF resource-sized message encode | protocol_core | 16384 | 1 | 17.94 us | 2.18 ms | 121.34x | 13.99% | 0.26% | 5.9 MiB | 31.1 MiB |
| Reticulum packet pack | protocol_core | 128 | 1 | 24 ns | 3.99 us | 168.19x | 0.34% | 0.23% | 311.1 MiB | 36.9 MiB |
| Reticulum packet unpack | protocol_core | 128 | 1 | 47 ns | 2.38 us | 50.76x | 0.00% | 0.04% | 159.7 MiB | 40.8 MiB |
| Reticulum resource segmentation 16 KiB | transport_hotpath | 16384 | 43 | 2.09 us | 6.37 us | 3.05x | 3.43% | 1.10% | 9.3 MiB | 34.8 MiB |
| Reticulum resource reassembly 16 KiB | transport_hotpath | 16384 | 43 | 386 ns | 942 ns | 2.44x | 0.81% | 1.06% | 24.4 MiB | 55.9 MiB |
| Reticulum announce create | protocol_core | 22 | 1 | 20.38 us | 2.10 ms | 103.27x | 0.17% | 0.72% | 6.2 MiB | 31.7 MiB |
| Reticulum announce validate | protocol_core | 22 | 1 | 48.97 us | 8.02 us | 0.16x | 1.03% | 0.62% | 6.0 MiB | 34.1 MiB |
| Reticulum announce validate batch 64 | protocol_core | 22 | 64 | 3.17 ms | 507.58 us | 0.16x | 0.13% | 0.69% | 5.8 MiB | 31.0 MiB |
| Reticulum identity sign | protocol_core | 2048 | 1 | 26.94 us | 2.10 ms | 77.80x | 0.05% | 2.40% | 6.0 MiB | 31.1 MiB |
| Reticulum identity verify | protocol_core | 2048 | 1 | 47.51 us | 7.82 ms | 164.69x | 0.33% | 0.42% | 5.8 MiB | 31.5 MiB |
| Reticulum identity encrypt | protocol_core | 2048 | 1 | 82.83 us | 15.53 ms | 187.55x | 0.13% | 0.29% | 5.9 MiB | 31.0 MiB |
| Reticulum identity decrypt | protocol_core | 2048 | 1 | 61.91 us | 17.99 ms | 290.58x | 0.10% | 0.79% | 5.9 MiB | 31.1 MiB |
| Reticulum resource request window | transport_hotpath | - | 6 | 30 ns | 4.10 us | 136.30x | 0.66% | 0.49% | 247.9 MiB | 36.7 MiB |

## Rust SDK transport comparison

In-process latency is normalized per call from fixed 100-call batches to avoid timer-resolution noise; ZeroMQ, HTTP, and Unix measurements time individual daemon requests.

| Operation | In-process p50 | ZeroMQ p50 | HTTP p50 | Unix p50 | ZeroMQ/HTTP | ZeroMQ/Unix |
|---|---:|---:|---:|---:|---:|---:|
| negotiate | 1.21 us | 161.20 us | 280.12 us | 172.28 us | 1.74x | 1.07x |
| snapshot | 540 ns | 157.87 us | 262.76 us | 161.58 us | 1.66x | 1.02x |
| status | 31 ns | 127.93 us | 217.06 us | 121.52 us | 1.70x | 0.95x |
| poll_events | 433 ns | 102.48 us | 200.60 us | 97.78 us | 1.96x | 0.95x |
| operation_registry | UNSUPPORTED | 912.22 us | 1.15 ms | 1.07 ms | 1.26x | 1.18x |
| router_stats | 21 ns | 118.69 us | 209.34 us | 144.14 us | 1.76x | 1.21x |

## Same-topology end-to-end comparison

These matched sender workloads use the same two-node loopback TCP topology with one Rust and one pinned-Python endpoint. Startup and route warm-up are outside the timed enqueue-to-receiver-evidence boundary.

| Workload | Route | Payload | Rust p50 | Python p50 | Rust/Python | Rust p95 | Python p95 | Rust CPU | Python CPU | Rust RSS | Python RSS | Rust variability | Python variability |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Loopback TCP cold destination discovery | cold | 0 | 397.50 ms | 2659.66 ms | 6.69x | 399.20 ms | 2660.93 ms | 2.680s | 2.210s | 81.5 MiB | 81.5 MiB | 0.32% | 0.02% |
| Loopback TCP link setup | warm | 0 | 179.79 ms | 138.59 ms | 0.77x | 185.81 ms | 139.37 ms | 2.360s | 1.870s | 81.4 MiB | 81.3 MiB | 1.85% | 0.17% |
| Loopback TCP direct delivery | warm | 256 | 735.76 ms | 453.36 ms | 0.62x | 757.71 ms | 456.14 ms | 6.630s | 2.100s | 81.3 MiB | 81.3 MiB | 1.42% | 0.22% |
| Loopback TCP opportunistic delivery | warm | 256 | 726.49 ms | 453.55 ms | 0.62x | 748.47 ms | 457.40 ms | 6.040s | 2.140s | 81.2 MiB | 81.4 MiB | 1.54% | 0.42% |
| Loopback TCP propagated delivery | warm | 256 | 900.28 ms | 7866.12 ms | 8.74x | 914.56 ms | 7869.93 ms | 4.560s | 6.880s | 81.3 MiB | 81.3 MiB | 1.13% | 0.02% |
| Loopback TCP resource delivery | warm | 16384 | 737.43 ms | 1028.66 ms | 1.39x | 760.30 ms | 1038.86 ms | 6.700s | 2.720s | 81.2 MiB | 81.4 MiB | 1.36% | 0.37% |

## Independent rns-rs network comparison

The pinned rns-rs peer and LXMF-rs ran as independent processes on the same runner. The recorded peer SHA is `6c6d79b83516feff271d15c97d39dd1de7798afe`; same-runner evidence is `True`.

| Workload | rns-rs / LXMF-rs evidence | p50 | p99 | Variation |
|---|---|---:|---:|---:|
| Cold path convergence | rns-rs requester | 0.001 s | 0.001 s | 5.69% |
| Warm path lookup | rns-rs cache | 0.000537 s | 0.000560 s | 4.30% |
| Link establishment | rns-rs initiator -> LXMF-rs | 0.002 s | 0.103 s | 4.56% |
| Exact 1 MiB Resource | lxmf_rs sender, SHA `cba3982ca4b9` | 0.212 s | 0.216 s | 0.34% |
| Exact 1 MiB Resource | rns_rs sender, SHA `cba3982ca4b9` | 0.202 s | 0.202 s | 0.05% |
| Exact 50 MiB Resource | lxmf_rs sender, SHA `649936cc2358` | 8.732 s | 8.834 s | 0.04% |
| Exact 50 MiB Resource | rns_rs sender, SHA `649936cc2358` | 7.215 s | 7.420 s | 1.38% |

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
python3 tools/scripts/performance_docs.py --release v0.10.1 --report target/criterion/python-impl-report/report.json
python3 tools/scripts/performance_docs.py --check
```
