# gRPC Adoption and Migration Policy

Status: Active

This document defines how external and internal users should choose between the
new gRPC API surface and the existing JSON-RPC surface exposed by `reticulumd`.

## Policy

For new non-browser integrations, gRPC is the preferred API.

JSON-RPC remains supported for:

- compatibility with existing clients
- browser-oriented tools and `/rpc/json`
- legacy operator flows that have not yet migrated

This is a migration policy, not an immediate removal notice. JSON-RPC is not
deprecated for `0.1.x`, but it is no longer the recommended starting point for
new service-to-service integrations.

## Recommended API Choice

Use gRPC when you want:

- generated typed clients
- server reflection
- service-to-service RPC
- streaming APIs
- a stable contract rooted in protobuf

Use JSON-RPC when you need:

- browser-friendly transport
- compatibility with existing scripts or tools
- parity with legacy operator/admin workflows that are not yet available on gRPC

## Current gRPC Coverage

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

This is sufficient for early adopter and internal integration use.

## Migration Status

### Ready on gRPC

- runtime negotiation and snapshot
- remote command invoke/reply/session inspection
- outbound message send/status/cancel
- interface listing and mutation
- topic creation and pagination
- attachment lifecycle, including chunked upload/download
- event polling and server streaming
- identity discovery/contact/presence/bootstrap paths
- marker create/list/update/delete

### Still Compatibility-Important on JSON-RPC

The following remain compatibility-important until parity is explicitly
documented elsewhere:

- browser-driven tooling built around `/rpc/json`
- any daemon method not yet exposed as a named gRPC service method
- older automation/operator flows still centered on JSON-RPC request bodies

## Support Stance

For `0.1.x`:

- gRPC is preferred for new integrations
- JSON-RPC remains supported
- additive gRPC service growth is expected
- removals or compatibility breaks require normal support-window notice under
  `docs/contracts/support-policy.md`

## Migration Guidance

1. New internal or external service integrations should start on gRPC.
2. Existing JSON-RPC clients may continue to operate unchanged.
3. If you are building browser tooling, continue using `/rpc/json` unless a
   dedicated gRPC-web path is introduced.
4. If you are moving an integration from JSON-RPC to gRPC, prefer service-level
   remapping rather than generic payload tunneling.

## Operator References

- Quickstart: `docs/grpc-getting-started.md`
- Full runbook: `docs/runbooks/grpc.md`
- Canonical internal API ADR: `docs/adr/0009-grpc-canonical-internal-api.md`
