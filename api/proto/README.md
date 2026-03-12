# LXMF Proto Layout

This directory holds the proposed protobuf source tree for the canonical internal gRPC API
described in `docs/adr/0009-grpc-canonical-internal-api.md`.

The initial tree is intentionally conservative:

- common transport contracts first
- runtime, delivery, and events as phase-1 services
- domain services staged after parity is proven
- admin/interface management modeled separately

These proto files are design scaffolding only until generation, lint, and CI wiring are added.

Current workflow:

- `cargo run -p xtask -- proto-check`
- `cargo run -p xtask -- proto-generate`

`proto-check` uses vendored `protoc` through `xtask` so contributors do not need a host protobuf
toolchain just to validate the tree.
