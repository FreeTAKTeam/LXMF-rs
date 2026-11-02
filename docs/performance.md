# Performance

<!-- GENERATED: tools/scripts/performance_docs.py -->

Dataset: [`docs/performance/main-2026-11-01.json`](performance/main-2026-11-01.json). All numbers below originate from commit `c59f35f62fb3196eeb391c943b840f0ad53ca2f3`.
Release dashboards published before this methodology (up to and including v0.14.0) overstate Rust's advantage; this page supersedes them.

## Methodology

The report uses `5` interleaved comparison runs and `3` isolated resource runs. Fixtures and process setup are completed before timed regions. Results are medians; p95 and p99 retain tail visibility. Rust/Python ranking is evidence, not a release threshold.

Workloads do the same work on both sides: LXMF encode signs and decode verifies the Ed25519 signature, as `LXMessage.pack()` and `LXMessage.unpack_from_bytes()` do; announce validation re-verifies every call instead of reusing Python's per-packet validation cache. Python runs with the PyCA `cryptography` backend that a normal `pip install rns` uses; the benchmark refuses to run on RNS's pure-Python fallback. Earlier datasets did not meet these rules and overstated Rust's advantage.

## Environment

- Timestamp: `2026-11-02T01:04:21Z`
- Commit: `c59f35f62fb3196eeb391c943b840f0ad53ca2f3`
- Python Reticulum: `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`
- Python LXMF: `727830cefda83d9c6e3982b48675425f3f988f9c`
- Rust: `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
- Python: `Python 3.14.4`
- CPU: `Intel(R) Core(TM) Ultra 7 165H`
- OS/kernel: `Linux pgiuseppe-AI 7.0.0-38-generic #38-Ubuntu SMP PREEMPT_DYNAMIC Fri Sep  4 09:10:14 UTC 2026 x86_64 GNU/Linux`
- Profile: `report`

## Protocol/core and transport hot paths

| Workload | Class | Payload | Batch | Rust p50 | Python p50 | Rust/Python | Rust variability | Python variability | Rust RSS | Python RSS |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| LXMF message decode + verify | protocol_core | 21 | 1 | 33.00 us | 89.44 us | 2.71x | 3.31% | 3.83% | 6.7 MiB | 51.7 MiB |
| LXMF message encode + sign | protocol_core | 12 | 1 | 13.83 us | 37.22 us | 2.69x | 2.18% | 2.80% | 6.9 MiB | 51.4 MiB |
| LXMF large message decode + verify | protocol_core | 2048 | 1 | 36.59 us | 94.59 us | 2.59x | 3.71% | 2.61% | 6.8 MiB | 51.5 MiB |
| LXMF large message encode + sign | protocol_core | 2048 | 1 | 21.18 us | 45.26 us | 2.14x | 3.62% | 2.46% | 6.9 MiB | 51.7 MiB |
| LXMF resource-sized message decode + verify | protocol_core | 16384 | 1 | 63.36 us | 119.79 us | 1.89x | 3.12% | 2.56% | 6.7 MiB | 51.4 MiB |
| LXMF resource-sized message encode + sign | protocol_core | 16384 | 1 | 65.72 us | 85.59 us | 1.30x | 3.47% | 1.76% | 6.5 MiB | 51.7 MiB |
| Reticulum packet pack | protocol_core | 128 | 1 | 15 ns | 2.07 us | 141.84x | 2.19% | 1.26% | 502.5 MiB | 59.7 MiB |
| Reticulum packet unpack | protocol_core | 128 | 1 | 26 ns | 1.22 us | 46.47x | 3.69% | 0.49% | 282.1 MiB | 67.5 MiB |
| Reticulum resource segmentation 16 KiB | transport_hotpath | 16384 | 43 | 1.10 us | 2.98 us | 2.71x | 3.77% | 1.65% | 13.1 MiB | 55.1 MiB |
| Reticulum resource reassembly 16 KiB | transport_hotpath | 16384 | 43 | 223 ns | 541 ns | 2.43x | 2.22% | 1.85% | 38.6 MiB | 91.8 MiB |
| Reticulum announce create | protocol_core | 22 | 1 | 14.20 us | 32.70 us | 2.30x | 1.41% | 1.80% | 6.9 MiB | 51.6 MiB |
| Reticulum announce validate | protocol_core | 22 | 1 | 34.83 us | 81.25 us | 2.33x | 2.28% | 2.62% | 6.8 MiB | 51.4 MiB |
| Reticulum announce validate batch 64 | protocol_core | 22 | 64 | 2.22 ms | 5.36 ms | 2.42x | 1.26% | 0.94% | 6.6 MiB | 51.5 MiB |
| Reticulum identity sign | protocol_core | 2048 | 1 | 18.53 us | 32.78 us | 1.77x | 4.43% | 1.24% | 6.7 MiB | 51.5 MiB |
| Reticulum identity verify | protocol_core | 2048 | 1 | 35.29 us | 75.92 us | 2.15x | 4.95% | 0.56% | 6.5 MiB | 51.7 MiB |
| Reticulum identity encrypt | protocol_core | 2048 | 1 | 52.48 us | 70.98 us | 1.35x | 1.30% | 1.88% | 6.5 MiB | 51.5 MiB |
| Reticulum identity decrypt | protocol_core | 2048 | 1 | 38.35 us | 39.52 us | 1.03x | 0.26% | 1.91% | 6.7 MiB | 51.6 MiB |
| Reticulum resource request window | transport_hotpath | - | 6 | 21 ns | 2.31 us | 111.06x | 5.68% | 2.94% | 355.2 MiB | 58.4 MiB |

## Rust SDK transport comparison

In-process latency is normalized per call from fixed 100-call batches to avoid timer-resolution noise; ZeroMQ, HTTP, and Unix measurements time individual daemon requests.

| Operation | In-process p50 | ZeroMQ p50 | HTTP p50 | Unix p50 | ZeroMQ/HTTP | ZeroMQ/Unix |
|---|---:|---:|---:|---:|---:|---:|
| negotiate | 745 ns | 159.36 us | 213.01 us | 189.49 us | 1.34x | 1.19x |
| snapshot | 358 ns | 88.80 us | 125.61 us | 118.12 us | 1.41x | 1.33x |
| status | 21 ns | 78.02 us | 113.98 us | 83.09 us | 1.46x | 1.06x |
| poll_events | 280 ns | 58.21 us | 114.76 us | 80.24 us | 1.97x | 1.38x |
| operation_registry | UNSUPPORTED | 542.27 us | 648.63 us | 660.93 us | 1.20x | 1.22x |
| router_stats | 29 ns | 74.86 us | 123.42 us | 85.82 us | 1.65x | 1.15x |

## Same-topology end-to-end comparison

These matched sender workloads use the same two-node loopback TCP topology with one Rust and one pinned-Python endpoint. Startup and route warm-up are outside the timed enqueue-to-receiver-evidence boundary. Both columns run both nodes, so whole-harness CPU and RSS cannot be attributed to one implementation and are not published here.

| Workload | Route | Payload | Rust p50 | Python p50 | Rust/Python | Rust p95 | Python p95 | Rust variability | Python variability |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Loopback TCP cold destination discovery | cold | 0 | 479.48 ms | 2769.66 ms | 5.78x | 492.19 ms | 2776.90 ms | 0.72% | 0.13% |
| Loopback TCP link setup | warm | 0 | 253.00 ms | 136.47 ms | 0.54x | 259.98 ms | 138.67 ms | 2.76% | 1.61% |
| Loopback TCP direct delivery | warm | 256 | 1151.76 ms | 451.34 ms | 0.39x | 1179.96 ms | 458.07 ms | 0.82% | 1.03% |
| Loopback TCP opportunistic delivery | warm | 256 | 1155.64 ms | 450.58 ms | 0.39x | 1170.90 ms | 456.74 ms | 1.08% | 0.67% |
| Loopback TCP propagated delivery | warm | 256 | 1177.70 ms | 7875.80 ms | 6.69x | 1204.52 ms | 7908.48 ms | 2.28% | 0.13% |
| Loopback TCP resource delivery | warm | 16384 | 1153.12 ms | 827.17 ms | 0.72x | 1171.12 ms | 838.01 ms | 1.23% | 0.26% |

## Independent rns-rs network comparison

The pinned rns-rs peer and LXMF-rs ran as independent processes on the same runner. The recorded peer SHA is `6c6d79b83516feff271d15c97d39dd1de7798afe`; same-runner evidence is `True`.

| Workload | rns-rs / LXMF-rs evidence | p50 | p99 | Variation |
|---|---|---:|---:|---:|
| Cold path convergence | rns-rs requester | 0.002 s | 0.002 s | 2.12% |
| Warm path lookup | rns-rs cache | 0.000896 s | 0.001042 s | 8.43% |
| Link establishment | rns-rs initiator -> LXMF-rs | 0.104 s | 0.105 s | 0.18% |
| Exact 1 MiB Resource | lxmf_rs sender, SHA `cba3982ca4b9` | 0.176 s | 0.179 s | 1.03% |
| Exact 1 MiB Resource | rns_rs sender, SHA `cba3982ca4b9` | 0.203 s | 0.203 s | 0.06% |
| Exact 50 MiB Resource | lxmf_rs sender, SHA `649936cc2358` | 6.481 s | 6.676 s | 1.75% |
| Exact 50 MiB Resource | rns_rs sender, SHA `649936cc2358` | 5.698 s | 6.003 s | 1.69% |

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
python3 tools/scripts/performance_docs.py --release main-2026-11-01 --report target/criterion/python-impl-report/report.json
python3 tools/scripts/performance_docs.py --check
```
