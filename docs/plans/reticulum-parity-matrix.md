# Reticulum Parity Matrix

Last verified: 2026-02-19 (`cargo fmt -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-features`)

Status legend: not-started | partial | done

| Python Module | Rust Module | Status |
| --- | --- | --- |
| `RNS/Reticulum.py` | `crates/libs/rns-core` | done |
| `RNS/Identity.py` | `crates/libs/rns-core` | done |
| `RNS/Destination.py` | `crates/libs/rns-core` | done |
| `RNS/Packet.py` | `crates/libs/rns-core` | done |
| `RNS/Transport.py` | `crates/libs/rns-transport` | done |
| `RNS/Link.py` | `crates/libs/rns-transport` | done |
| `RNS/Interfaces/*` | `crates/libs/rns-transport` | done |
| `RNS/Cryptography/*` | `crates/libs/rns-core` | done |
| `RNS/Resource.py` | `crates/libs/rns-transport` | done |
| `RNS/Channel.py` | `crates/libs/rns-transport` | done |
| `RNS/Buffer.py` | `crates/libs/rns-core` | done |
| `RNS/Discovery.py` | `crates/libs/rns-transport` | done |
| `RNS/Resolver.py` | `crates/libs/rns-core` | done |
| `RNS/Utilities/*` | `crates/libs/rns-core` | done |
| `RNS/CRNS/*` | `crates/apps/rns-tools` | done |
