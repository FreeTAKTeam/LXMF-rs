# Independent interoperability evidence

Python Reticulum 1.5.2 at `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`
and pinned Python LXMF remain the compatibility reference for active release
gates. This directory records a separate evidence axis against independently
authored implementations; it does not replace the Python parity inventory.

The canonical command is:

```bash
cargo xtask interop-independent --peer rns-rs --level release
cargo xtask interop-independent --peer reticulum-go --level release
```

`.github/workflows/independent-interop.yml` runs bounded rns-rs evidence on pull
requests, both peers nightly, and the strongest profiles on release tags. Every
run uploads JSON, Markdown, logs/configuration in a raw tarball, and a checksum.
Tag runs also publish standalone combined JSON/Markdown/HTML release assets.

## Current release evidence

The [v0.14.0 release record](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.14.0)
ties these assets to commit `7822b608f2cd2debd8170895bc580d01dda5356e`:

- [Structured interoperability results](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.14.0/v0.14.0-independent.json),
  [readable matrix](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.14.0/v0.14.0-independent.md),
  and [standalone report](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.14.0/v0.14.0-independent.html).
- Checksummed per-peer raw reports and retained peer divergences in the same
  release assets. Reticulum-Go remains a supported-subset check.
- [Performance JSON](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.14.0/lxmf-rs-performance.json)
  and [performance dashboard](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.14.0/lxmf-rs-performance.html).

The checked-in generated [performance page](../performance.md) retains its
v0.10.1 dataset. Do not relabel those measurements as v0.14.0 or as current-main
results.

## Historical evidence

Earlier versioned evidence remains available:

- [`v0.10.1-independent.json`](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.10.1/v0.10.1-independent.json)
  — release-tag structured peer results for RNS 1.5.2 at
  `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`;
- [`v0.10.1-independent.md`](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.10.1/v0.10.1-independent.md)
  — concise release-tag interoperability matrix;
- [`v0.10.1-independent.html`](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.10.1/v0.10.1-independent.html)
  — standalone release-tag report with the complete JSON embedded;
- [v0.10.1 release assets](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.10.1)
  — checksummed raw peer bundles and per-peer checksums;
- [`v0.10.1 performance JSON`](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.10.1/lxmf-rs-performance.json)
  and [`v0.10.1 performance dashboard`](https://github.com/FreeTAKTeam/LXMF-rs/releases/download/v0.10.1/lxmf-rs-performance.html)
  — the bounded stable release performance dataset;

- [`v0.10.0-independent.json`](v0.10.0-independent.json) — complete structured
  peer results, readiness axes, pins, limitations, and five-sample independent
  performance evidence;
- [`v0.10.0-independent.md`](v0.10.0-independent.md) — concise human-readable
  matrix;
- [`v0.10.0-independent.html`](v0.10.0-independent.html) — standalone report
  with the complete JSON embedded;
- [`v0.9.9-independent.json`](v0.9.9-independent.json) — complete structured
  peer results, readiness axes, pins, limitations, and five-sample independent
  performance evidence;
- [`v0.9.9-independent.md`](v0.9.9-independent.md) — concise human-readable
  matrix;
- [`v0.9.9-independent.html`](v0.9.9-independent.html) — standalone report with
  the complete JSON embedded;
- [`../performance/v0.9.9.json`](../performance/v0.9.9.json) and
  [`../performance/v0.9.9.html`](../performance/v0.9.9.html) — historical
  performance dataset and dashboard;
- [`../performance/v0.10.1.json`](../performance/v0.10.1.json) and
  [`../performance/v0.10.1.html`](../performance/v0.10.1.html) — historical stable
  performance dataset and dashboard;
- [`../performance/v0.10.0.json`](../performance/v0.10.0.json) and
  [`../performance/v0.10.0.html`](../performance/v0.10.0.html) — historical
  stable performance dataset and dashboard;
- hosted raw interop evidence from workflow
  [`33254264125`](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264125)
  and the v0.10.1 release assets, plus historical workflow
  [`32736249030`](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/32736249030).
  The v0.10.1 checksummed raw performance evidence is published by workflow
  [`33254264175`](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/33254264175);
  historical checksummed raw performance evidence is from workflow
  [`32736249025`](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/32736249025).

Actions artifacts are retention-bound. The same workflows attach the public
reports, raw bundles, and checksums to GitHub Releases on tags so permanent
release evidence remains tied to the exact released commit.

The rns-rs gate allows only the named, Python-controlled peer divergences in
`tools/scripts/independent_interop_gate.py`: local-destination path response,
Channel proof, peer activation of an LXMF-rs-initiated Link, and the two teardown
rows blocked by those failures. Reticulum-Go unsupported rows are explicit API
or transport capability boundaries. Any new failure, block, missing required
scenario, build failure, or harness failure makes CI fail.

Published evidence covers two-node and multi-hop traffic, exact 1 MiB and 50 MiB
Resources where supported, all-LXMF-rs and mixed five-node chains, route gravity
and rebalancing, boundary policy, endpoint/intermediary/daemon restart,
shared-instance attachment and reconnection, deterministic loss/latency/
duplication/reordering, and same-runner performance measurements. Physical-radio
HIL, public-network soak, and named third-party applications remain separate
evidence tracks.
