# March 2026: gRPC Surface for `reticulumd`

Status: draft release notes

This release introduces a first-class gRPC surface for `reticulumd` and makes
it the preferred API for new non-browser integrations.

## Summary

The repository now ships:

- a live gRPC server in `reticulumd`
- protobuf contracts under `api/proto`
- build-time generated Rust bindings
- server reflection for `grpcurl` and other reflection-aware tools
- auth and TLS parity with the existing HTTP RPC policy
- an official Rust client crate
- a small operator wrapper for common tasks

JSON-RPC remains supported for compatibility and browser-oriented tooling.

## New gRPC Services

The currently exposed service surface includes:

- `lxmf.runtime.v1.RuntimeService`
- `lxmf.command.v1.CommandService`
- `lxmf.delivery.v1.DeliveryService`
- `lxmf.admin.v1.InterfaceAdminService`
- `lxmf.topics.v1.TopicService`
- `lxmf.attachments.v1.AttachmentService`
- `lxmf.events.v1.EventService`
- `lxmf.identity.v1.IdentityService`
- `lxmf.markers.v1.MarkerService`
- `lxmf.peers.v1.PeerService`

## New Operator Paths

- Quickstart: `docs/grpc-getting-started.md`
- Full runbook: `docs/runbooks/grpc.md`
- Rust client crate: `crates/libs/lxmf-grpc-client`
- Small CLI wrapper: `cargo run -p rns-tools --bin rngrpc -- ...`

## Migration Guidance

- New non-browser integrations should prefer gRPC.
- Existing JSON-RPC clients do not need to migrate immediately.
- Browser tools should continue using `/rpc/json` unless a dedicated gRPC-web
  path is introduced later.

The formal support stance is documented in:

- `docs/contracts/grpc-adoption-and-migration.md`

## Operational Notes

- gRPC now supports dedicated `--grpc-tls-cert`, `--grpc-tls-key`, and
  `--grpc-tls-client-ca` flags.
- Reflection is enabled by default.
- CI now validates the proto surface, gRPC transport tests, and the client/tool
  crates.

## Follow-On Work

The remaining work is productization and coverage, not transport bootstrap:

- fill any remaining domain gaps needed by external users
- consider publishing a non-Rust generated client workflow
- continue tightening release/docs coverage around migration and support stance
