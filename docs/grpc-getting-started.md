# gRPC Getting Started

This guide is the shortest path to using the new gRPC surface in `reticulumd`.

If you want the full operator reference, examples for every currently exposed
service, or TLS/mTLS details, use the runbook at
`docs/runbooks/grpc.md`.

## What gRPC Covers Today

The live gRPC surface currently includes:

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

This is the preferred path for new non-browser integrations.

## Start the Daemon

Local plaintext development:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 127.0.0.1:4243 \
  --grpc 127.0.0.1:50051 \
  --db reticulum.db
```

If you want TLS or mTLS on the gRPC listener, add:

```bash
--grpc-tls-cert /path/server.pem \
--grpc-tls-key /path/server.key \
--grpc-tls-client-ca /path/ca.pem
```

## Inspect the API with `grpcurl`

Server reflection is enabled, so you can inspect services without manually
passing proto files.

List services:

```bash
grpcurl -plaintext 127.0.0.1:50051 list
```

Describe the topic service:

```bash
grpcurl -plaintext 127.0.0.1:50051 describe lxmf.topics.v1.TopicService
```

Fetch a runtime snapshot:

```bash
grpcurl \
  -plaintext \
  -d '{"includeCounts":true}' \
  127.0.0.1:50051 \
  lxmf.runtime.v1.RuntimeService/GetSnapshot
```

## Use the Official Rust Client

The workspace includes a generated client crate at
`crates/libs/lxmf-grpc-client`.

There is also a minimal standalone example at:

- `examples/grpc-hello-rust`

Smoke-test it against a local daemon:

```bash
LXMF_GRPC_ENDPOINT=http://127.0.0.1:50051 \
cargo run -p lxmf-grpc-client --example smoke
```

If token auth is enabled:

```bash
LXMF_GRPC_ENDPOINT=https://127.0.0.1:50051 \
LXMF_GRPC_BEARER_TOKEN=<token> \
cargo run -p lxmf-grpc-client --example smoke
```

## Use the Small Operator Wrapper

For quick tasks without writing code:

```bash
cargo run -p rns-tools --bin rngrpc -- snapshot
cargo run -p rns-tools --bin rngrpc -- topics list --limit 10
cargo run -p rns-tools --bin rngrpc -- interfaces list
cargo run -p rns-tools --bin rngrpc -- events poll --max 8
cargo run -p rns-tools --bin rngrpc -- markers list --limit 10
```

It accepts:

- `--endpoint <url>`
- `--bearer-token <token>`

or the environment variables:

- `LXMF_GRPC_ENDPOINT`
- `LXMF_GRPC_BEARER_TOKEN`

## Recommended Usage Stance

Use gRPC for:

- new service-to-service integrations
- generated typed clients
- operational tooling that benefits from reflection and streaming

Keep JSON-RPC for:

- compatibility
- existing operator flows
- browser-oriented tooling that still depends on `/rpc/json`

## Next Reference

After you have the basics working, move to:

- `docs/runbooks/grpc.md`
