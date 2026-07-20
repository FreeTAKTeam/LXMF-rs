# Performance

<!-- GENERATED: tools/scripts/performance_docs.py -->

Dataset: [`docs/performance/v0.9.5.json`](performance/v0.9.5.json). All numbers below originate from release SHA `c4fd18761e41caf2f7d2c7307d49c37aa6dc43ca`.

## Methodology

The report uses `5` interleaved comparison runs and `3` isolated resource runs. Fixtures and process setup are completed before timed regions. Results are medians; p95 and p99 retain tail visibility. Rust/Python ranking is evidence, not a release threshold.

## Environment

- Timestamp: `2026-07-16T13:58:22Z`
- Release SHA: `c4fd18761e41caf2f7d2c7307d49c37aa6dc43ca`
- Python Reticulum: `15320e4d2cfabb143c1db20ca887e275fd521585`
- Python LXMF: `727830cefda83d9c6e3982b48675425f3f988f9c`
- Rust: `rustc 1.96.0 (ac68faa20 2026-05-25)`
- Python: `Python 3.14.4`
- CPU: `Intel(R) Core(TM) Ultra 7 165H`
- OS/kernel: `Linux pgiuseppe-AI 7.0.0-27-generic #27-Ubuntu SMP PREEMPT_DYNAMIC Thu Jun 18 19:13:49 UTC 2026 x86_64 GNU/Linux`
- Profile: `report`

## Protocol/core and transport hot paths

| Workload | Class | Payload | Batch | Rust p50 | Python p50 | Rust/Python | Rust variability | Python variability | Rust RSS | Python RSS |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| LXMF message decode | protocol_core | 21 | 1 | 150 ns | 86.56 us | 576.59x | 3.25% | 2.39% | 53.8 MiB | 38.1 MiB |
| LXMF message encode | protocol_core | 12 | 1 | 260 ns | 36.27 us | 139.59x | 2.57% | 2.31% | 33.4 MiB | 38.5 MiB |
| LXMF large message decode | protocol_core | 2048 | 1 | 250 ns | 92.94 us | 371.58x | 3.37% | 2.93% | 34.4 MiB | 38.2 MiB |
| LXMF large message encode | protocol_core | 2048 | 1 | 500 ns | 43.43 us | 86.95x | 2.08% | 2.00% | 19.7 MiB | 38.2 MiB |
| LXMF resource-sized message decode | protocol_core | 16384 | 1 | 525 ns | 114.06 us | 217.07x | 3.11% | 0.28% | 20.0 MiB | 38.1 MiB |
| LXMF resource-sized message encode | protocol_core | 16384 | 1 | 1.60 us | 82.33 us | 51.31x | 2.40% | 2.06% | 10.2 MiB | 38.1 MiB |
| Reticulum packet pack | protocol_core | 128 | 1 | 14 ns | 1.70 us | 119.10x | 4.30% | 2.06% | 514.7 MiB | 51.8 MiB |
| Reticulum packet unpack | protocol_core | 128 | 1 | 59 ns | 1.11 us | 18.78x | 3.83% | 4.97% | 128.5 MiB | 59.5 MiB |
| Reticulum resource segmentation 16 KiB | transport_hotpath | 16384 | 43 | 1.16 us | 2.96 us | 2.55x | 2.68% | 4.26% | 11.8 MiB | 46.1 MiB |
| Reticulum resource reassembly 16 KiB | transport_hotpath | 16384 | 43 | 220 ns | 534 ns | 2.42x | 1.31% | 3.00% | 38.3 MiB | 81.0 MiB |
| Reticulum announce create | protocol_core | 22 | 1 | 14.10 us | 33.24 us | 2.36x | 3.40% | 3.19% | 6.1 MiB | 38.8 MiB |
| Reticulum announce validate | protocol_core | 22 | 1 | 35.86 us | 86.20 us | 2.40x | 6.11% | 0.31% | 5.7 MiB | 38.2 MiB |
| Reticulum announce validate batch 64 | protocol_core | 22 | 64 | 2.33 ms | 5.47 ms | 2.35x | 1.83% | 1.14% | 5.7 MiB | 38.3 MiB |
| Reticulum identity sign | protocol_core | 2048 | 1 | 18.73 us | 32.17 us | 1.72x | 2.35% | 2.68% | 6.1 MiB | 38.6 MiB |
| Reticulum identity verify | protocol_core | 2048 | 1 | 34.58 us | 75.32 us | 2.18x | 0.94% | 0.91% | 5.6 MiB | 38.4 MiB |
| Reticulum identity encrypt | protocol_core | 2048 | 1 | 54.38 us | 68.92 us | 1.27x | 1.05% | 0.89% | 5.6 MiB | 38.2 MiB |
| Reticulum identity decrypt | protocol_core | 2048 | 1 | 39.79 us | 39.64 us | 1.00x | 0.49% | 0.48% | 5.7 MiB | 38.3 MiB |
| Reticulum resource request window | transport_hotpath | - | 6 | 21 ns | 2.02 us | 97.50x | 1.49% | 2.38% | 355.4 MiB | 49.2 MiB |

## Rust SDK transport comparison

| Operation | ZeroMQ p50 | HTTP p50 | Unix p50 | ZeroMQ/HTTP | ZeroMQ/Unix |
|---|---:|---:|---:|---:|---:|
| negotiate | 206.86 us | 188.93 us | 115.57 us | 0.91x | 0.56x |
| snapshot | 254.90 us | 193.38 us | 154.42 us | 0.76x | 0.61x |
| status | 208.15 us | 257.42 us | 94.15 us | 1.24x | 0.45x |
| poll_events | 176.76 us | 135.50 us | 88.36 us | 0.77x | 0.50x |
| operation_registry | 1.19 ms | 1.31 ms | 1.20 ms | 1.10x | 1.00x |
| router_stats | 194.43 us | 126.00 us | 88.01 us | 0.65x | 0.45x |

## Same-topology end-to-end comparison

These matched sender workloads use the same two-node loopback TCP topology with one Rust and one pinned-Python endpoint. Startup and route warm-up are outside the timed enqueue-to-receiver-evidence boundary.

| Workload | Route | Payload | Rust p50 | Python p50 | Rust/Python | Rust p95 | Python p95 | Rust CPU | Python CPU | Rust RSS | Python RSS | Rust variability | Python variability |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Loopback TCP cold destination discovery | cold | 0 | 369.44 ms | 2648.70 ms | 7.17x | 400.40 ms | 2858.14 ms | 2.280s | 2.150s | 80.2 MiB | 80.4 MiB | 1.18% | 1.22% |
| Loopback TCP direct delivery | warm | 256 | 513.84 ms | 447.86 ms | 0.87x | 545.52 ms | 461.80 ms | 4.790s | 1.610s | 80.6 MiB | 80.1 MiB | 1.64% | 1.72% |
| Loopback TCP opportunistic delivery | warm | 256 | 652.30 ms | 517.23 ms | 0.79x | 1565.55 ms | 748.35 ms | 6.465s | 3.100s | 80.5 MiB | 80.6 MiB | 18.19% | 16.77% |
| Loopback TCP propagated delivery | warm | 256 | 721.21 ms | 7858.45 ms | 10.90x | 890.08 ms | 7873.64 ms | 3.440s | 6.480s | 80.4 MiB | 80.2 MiB | 3.49% | 0.07% |
| Loopback TCP resource delivery | warm | 16384 | 513.15 ms | 817.19 ms | 1.59x | 596.14 ms | 836.28 ms | 4.670s | 1.870s | 80.4 MiB | 80.4 MiB | 0.99% | 0.57% |

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
python3 tools/scripts/performance_docs.py --release v0.9.5 --report target/criterion/python-impl-report/report.json
python3 tools/scripts/performance_docs.py --check
```
