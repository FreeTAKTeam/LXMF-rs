# Independent interoperability evidence

Python Reticulum 1.4.2 and pinned Python LXMF remain the compatibility
reference. This directory records a separate evidence axis against independently
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
