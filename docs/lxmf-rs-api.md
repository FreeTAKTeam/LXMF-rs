# LXMF Rust API

This document describes the intended public API surface for `lxmf`.

## Stability policy

- Current crate version: `0.1.x`.
- While `0.x`, API changes can happen, but breaking changes should still be documented in release notes.
- Public API is defined as items exported from `src/lib.rs` and used by examples/tests in this repository.

## Core types

- `lxmf::Message`
  - High-level mutable message model for creating/parsing LXMF user payloads.
  - Use when you want ergonomic title/content/field access.
- `lxmf::message::Payload`
  - Msgpack payload representation (timestamp/title/content/fields/stamp).
- `lxmf::message::WireMessage`
  - Signed wire-format message representation with pack/unpack and encryption helpers.
- `lxmf::router::Router`
  - Outbound queue and propagation-node app-data behavior.
- `lxmf::propagation::PropagationNode`
  - Store/fetch and verification policy for propagation-mode messages.
- `lxmf::propagation::PropagationService`
  - Envelope ingestion service backed by `PropagationStore`.

## Message workflows

### Build and sign a wire message

```rust
use lxmf::message::{Payload, WireMessage};
use reticulum::identity::PrivateIdentity;

let destination = [0u8; 16];
let source = [1u8; 16];
let payload = Payload::new(0.0, Some(b"hello".to_vec()), Some(b"title".to_vec()), None, None);

let mut wire = WireMessage::new(destination, source, payload);
let signer = PrivateIdentity::new();
wire.sign(&signer)?;
let packed = wire.pack()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Parse a wire message

```rust
use lxmf::message::WireMessage;

let packed: Vec<u8> = vec![]; // from transport/storage
let parsed = WireMessage::unpack(&packed)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Propagation workflows

- Decode and inspect envelopes: `lxmf::propagation::unpack_envelope`.
- Validate proof-of-work stamps: `lxmf::propagation::validate_stamp`.
- Ingest propagation payloads: `lxmf::propagation::ingest_envelope`.

## Compatibility inputs

Python compatibility is tested using fixture-backed tests in
`tests/fixtures/python/lxmf` and parity tests under `tests/*parity*.rs`.
