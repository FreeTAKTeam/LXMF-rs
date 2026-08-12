# Performance

<!-- GENERATED: tools/scripts/performance_docs.py -->

Dataset: [`docs/performance/v0.9.9.json`](performance/v0.9.9.json). All numbers below originate from release SHA `7199c4038a3ba786abb4dfbc95cbd6cd16ed9116`.
The standalone release dashboard is available at [the latest GitHub Release asset](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest/download/lxmf-rs-performance.html); the release asset is the public source for the complete matrix.

## Methodology

The report uses `5` interleaved comparison runs and `2` isolated resource runs. Fixtures and process setup are completed before timed regions. Results are medians; p95 and p99 retain tail visibility. Rust/Python ranking is evidence, not a release threshold.

## Environment

- Timestamp: `2026-08-12T03:17:48Z`
- Release SHA: `7199c4038a3ba786abb4dfbc95cbd6cd16ed9116`
- Python Reticulum: `b48b96e61676504e0a4e527b33b9a0b4495c6872`
- Python LXMF: `727830cefda83d9c6e3982b48675425f3f988f9c`
- Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`
- Python: `Python 3.12.13`
- CPU: `AMD EPYC 9V74 80-Core Processor`
- OS/kernel: `Linux runnervmzvulz 6.17.0-1022-azure #22-Ubuntu SMP Mon Jul 27 17:24:03 UTC 2026 x86_64 x86_64 x86_64 GNU/Linux`
- Profile: `report`

## Protocol/core and transport hot paths

| Workload | Class | Payload | Batch | Rust p50 | Python p50 | Rust/Python | Rust variability | Python variability | Rust RSS | Python RSS |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| LXMF message decode | protocol_core | 21 | 1 | 285 ns | 8.33 ms | 29222.35x | 0.91% | 1.07% | 30.8 MiB | 31.2 MiB |
| LXMF message encode | protocol_core | 12 | 1 | 370 ns | 2.17 ms | 5864.16x | 1.05% | 0.71% | 24.9 MiB | 30.6 MiB |
| LXMF large message decode | protocol_core | 2048 | 1 | 455 ns | 8.31 ms | 18252.45x | 1.57% | 0.95% | 21.4 MiB | 30.6 MiB |
| LXMF large message encode | protocol_core | 2048 | 1 | 762 ns | 2.19 ms | 2869.52x | 1.73% | 0.48% | 15.3 MiB | 30.5 MiB |
| LXMF resource-sized message decode | protocol_core | 16384 | 1 | 1.54 us | 8.41 ms | 5469.65x | 8.18% | 1.80% | 10.4 MiB | 31.1 MiB |
| LXMF resource-sized message encode | protocol_core | 16384 | 1 | 3.10 us | 2.25 ms | 725.21x | 1.46% | 0.23% | 7.7 MiB | 31.3 MiB |
| Reticulum packet pack | protocol_core | 128 | 1 | 29 ns | 3.99 us | 138.76x | 0.91% | 0.75% | 257.7 MiB | 36.6 MiB |
| Reticulum packet unpack | protocol_core | 128 | 1 | 109 ns | 2.33 us | 21.42x | 0.45% | 0.90% | 71.8 MiB | 40.6 MiB |
| Reticulum resource segmentation 16 KiB | transport_hotpath | 16384 | 43 | 2.35 us | 6.85 us | 2.91x | 8.64% | 0.28% | 8.6 MiB | 34.1 MiB |
| Reticulum resource reassembly 16 KiB | transport_hotpath | 16384 | 43 | 455 ns | 881 ns | 1.94x | 1.04% | 2.16% | 21.7 MiB | 57.4 MiB |
| Reticulum announce create | protocol_core | 22 | 1 | 22.96 us | 2.17 ms | 94.31x | 0.14% | 0.56% | 6.1 MiB | 30.7 MiB |
| Reticulum announce validate | protocol_core | 22 | 1 | 52.00 us | 8.16 ms | 156.89x | 1.06% | 0.47% | 5.9 MiB | 30.7 MiB |
| Reticulum announce validate batch 64 | protocol_core | 22 | 64 | 3.36 ms | 522.84 ms | 155.62x | 0.44% | 0.16% | 5.9 MiB | 31.0 MiB |
| Reticulum identity sign | protocol_core | 2048 | 1 | 30.10 us | 2.19 ms | 72.74x | 0.05% | 1.70% | 5.6 MiB | 30.7 MiB |
| Reticulum identity verify | protocol_core | 2048 | 1 | 50.88 us | 8.20 ms | 161.11x | 1.27% | 1.38% | 5.6 MiB | 30.6 MiB |
| Reticulum identity encrypt | protocol_core | 2048 | 1 | 93.57 us | 15.85 ms | 169.42x | 0.08% | 0.96% | 5.7 MiB | 30.7 MiB |
| Reticulum identity decrypt | protocol_core | 2048 | 1 | 70.05 us | 18.96 ms | 270.66x | 0.17% | 2.65% | 5.8 MiB | 30.7 MiB |
| Reticulum resource request window | transport_hotpath | - | 6 | 33 ns | 4.15 us | 127.23x | 0.17% | 0.72% | 229.4 MiB | 36.2 MiB |

## Rust SDK transport comparison

In-process latency is normalized per call from fixed 100-call batches to avoid timer-resolution noise; ZeroMQ, HTTP, and Unix measurements time individual daemon requests.

| Operation | In-process p50 | ZeroMQ p50 | HTTP p50 | Unix p50 | ZeroMQ/HTTP | ZeroMQ/Unix |
|---|---:|---:|---:|---:|---:|---:|
| negotiate | 1.32 us | 132.06 us | 257.74 us | 151.55 us | 1.95x | 1.15x |
| snapshot | 670 ns | 124.35 us | 243.94 us | 184.76 us | 1.96x | 1.49x |
| status | 35 ns | 103.36 us | 205.62 us | 107.34 us | 1.99x | 1.04x |
| poll_events | 528 ns | 75.03 us | 175.51 us | 85.51 us | 2.34x | 1.14x |
| operation_registry | UNSUPPORTED | 901.92 us | 1.16 ms | 1.11 ms | 1.29x | 1.23x |
| router_stats | 23 ns | 91.01 us | 204.33 us | 98.61 us | 2.25x | 1.08x |

## Same-topology end-to-end comparison

These matched sender workloads use the same two-node loopback TCP topology with one Rust and one pinned-Python endpoint. Startup and route warm-up are outside the timed enqueue-to-receiver-evidence boundary.

| Workload | Route | Payload | Rust p50 | Python p50 | Rust/Python | Rust p95 | Python p95 | Rust CPU | Python CPU | Rust RSS | Python RSS | Rust variability | Python variability |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Loopback TCP cold destination discovery | cold | 0 | 396.23 ms | 2655.01 ms | 6.70x | 396.25 ms | 2656.78 ms | 2.600s | 2.160s | 81.1 MiB | 80.9 MiB | 0.00% | 0.03% |
| Loopback TCP link setup | warm | 0 | 181.85 ms | 140.28 ms | 0.77x | 183.04 ms | 140.46 ms | 2.370s | 1.880s | 81.1 MiB | 80.9 MiB | 0.32% | 0.07% |
| Loopback TCP direct delivery | warm | 256 | 726.18 ms | 457.47 ms | 0.63x | 739.03 ms | 459.29 ms | 6.650s | 2.150s | 80.9 MiB | 80.9 MiB | 0.83% | 0.25% |
| Loopback TCP opportunistic delivery | warm | 256 | 725.80 ms | 457.00 ms | 0.63x | 741.76 ms | 457.55 ms | 6.060s | 2.120s | 81.0 MiB | 80.9 MiB | 0.62% | 0.12% |
| Loopback TCP propagated delivery | warm | 256 | 875.59 ms | 7872.01 ms | 8.99x | 891.47 ms | 11874.16 ms | 4.540s | 14.910s | 80.9 MiB | 80.9 MiB | 1.03% | 0.04% |
| Loopback TCP resource delivery | warm | 16384 | 725.02 ms | 1175.78 ms | 1.62x | 738.23 ms | 1187.82 ms | 6.680s | 2.870s | 81.1 MiB | 80.9 MiB | 0.31% | 0.19% |

## Independent rns-rs network comparison

The pinned rns-rs peer and LXMF-rs ran as independent processes on the same runner. The recorded peer SHA is `6c6d79b83516feff271d15c97d39dd1de7798afe`; same-runner evidence is `True`.

| Workload | rns-rs / LXMF-rs evidence | p50 | p99 | Variation |
|---|---|---:|---:|---:|
| Cold path convergence | rns-rs requester | 0.001 s | 0.001 s | 4.52% |
| Warm path lookup | rns-rs cache | 0.000529 s | 0.000617 s | 7.05% |
| Link establishment | rns-rs initiator -> LXMF-rs | 0.002 s | 0.103 s | 9.13% |
| Exact 1 MiB Resource | lxmf_rs sender, SHA `cba3982ca4b9` | 0.196 s | 0.198 s | 0.81% |
| Exact 1 MiB Resource | rns_rs sender, SHA `cba3982ca4b9` | 0.202 s | 0.202 s | 0.03% |
| Exact 50 MiB Resource | lxmf_rs sender, SHA `649936cc2358` | 8.222 s | 8.224 s | 0.01% |
| Exact 50 MiB Resource | rns_rs sender, SHA `649936cc2358` | 8.731 s | 8.827 s | 0.01% |

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
python3 tools/scripts/performance_docs.py --release v0.9.9 --report target/criterion/python-impl-report/report.json
python3 tools/scripts/performance_docs.py --check
```
