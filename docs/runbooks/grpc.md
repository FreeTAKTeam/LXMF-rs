# gRPC Runbook

This runbook covers the current `reticulumd` gRPC surface and how to use it
with `grpcurl` or generated clients.

## Current Services

- `lxmf.runtime.v1.RuntimeService`
  - `Negotiate`
  - `GetSnapshot`
- `lxmf.command.v1.CommandService`
  - `InvokeCommand`
  - `ReplyCommand`
  - `GetCommandSession`
  - `ListCommandSessions`
- `lxmf.delivery.v1.DeliveryService`
  - `Send`
  - `GetStatus`
  - `Cancel`
- `lxmf.admin.v1.InterfaceAdminService`
  - `ListInterfaces`
  - `SetInterfaces`
  - `ReloadConfig`
- `lxmf.topics.v1.TopicService`
  - `CreateTopic`
  - `GetTopic`
  - `ListTopics`
  - `SubscribeTopic`
  - `UnsubscribeTopic`
  - `PublishTopic`
- `lxmf.attachments.v1.AttachmentService`
  - `StoreAttachment`
  - `GetAttachment`
  - `DeleteAttachment`
  - `DownloadAttachment`
  - `UploadStart`
  - `UploadChunk`
  - `UploadCommit`
  - `DownloadChunk`
  - `ListAttachments`
- `lxmf.events.v1.EventService`
  - `PollEvents`
  - `SubscribeEvents`
- `lxmf.identity.v1.IdentityService`
  - `ListIdentities`
  - `ActivateIdentity`
  - `ImportIdentity`
  - `ExportIdentity`
  - `ResolveIdentity`
  - `AnnounceNow`
  - `ListPresence`
  - `UpdateContact`
  - `ListContacts`
  - `BootstrapIdentity`
- `lxmf.markers.v1.MarkerService`
  - `CreateMarker`
  - `ListMarkers`
  - `UpdateMarkerPosition`
  - `DeleteMarker`
- `lxmf.peers.v1.PeerService`
  - `ListPeers`
  - `SearchPeers`
  - `SyncPeer`
  - `Unpeer`
  - `ClearPeers`

## Start `reticulumd`

Plaintext local development:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 127.0.0.1:4243 \
  --grpc 127.0.0.1:50051 \
  --db reticulum.db
```

TLS or mTLS:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 0.0.0.0:4243 \
  --grpc 0.0.0.0:50051 \
  --rpc-tls-cert /path/server.pem \
  --rpc-tls-key /path/server.key \
  --rpc-tls-client-ca /path/ca.pem \
  --grpc-tls-cert /path/server.pem \
  --grpc-tls-key /path/server.key \
  --grpc-tls-client-ca /path/ca.pem \
  --db reticulum.db
```

The HTTP RPC and gRPC listeners can now be configured independently. If you omit
the `--grpc-tls-*` flags, the gRPC listener remains plaintext even when HTTP RPC
TLS is enabled.

## Reflection

Server reflection is enabled.

List services:

```bash
grpcurl -plaintext 127.0.0.1:50051 list
```

Describe a service:

```bash
grpcurl -plaintext 127.0.0.1:50051 describe lxmf.runtime.v1.RuntimeService
```

## Official Rust Client

The workspace now includes an official Rust client crate at
[`crates/libs/lxmf-grpc-client`](../../crates/libs/lxmf-grpc-client).

Smoke test it against a local daemon:

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

## Small Operator Wrapper

For quick operator tasks, use the `rngrpc` helper in `rns-tools`:

```bash
cargo run -p rns-tools --bin rngrpc -- snapshot
cargo run -p rns-tools --bin rngrpc -- topics list --limit 10
cargo run -p rns-tools --bin rngrpc -- interfaces list
cargo run -p rns-tools --bin rngrpc -- events poll --max 8
cargo run -p rns-tools --bin rngrpc -- markers list --limit 10
```

It honors `LXMF_GRPC_ENDPOINT` and `LXMF_GRPC_BEARER_TOKEN`, or you can pass
`--endpoint` and `--bearer-token` explicitly.

## Example Calls

Snapshot:

```bash
grpcurl \
  -plaintext \
  -d '{"includeCounts":true}' \
  127.0.0.1:50051 \
  lxmf.runtime.v1.RuntimeService/GetSnapshot
```

Invoke a command:

```bash
grpcurl \
  -plaintext \
  -d '{
    "command":"status",
    "target":"peer-a",
    "payload":{"mode":"quick"},
    "timeoutMs":"5000"
  }' \
  127.0.0.1:50051 \
  lxmf.command.v1.CommandService/InvokeCommand
```

List command sessions:

```bash
grpcurl \
  -plaintext \
  -d '{"page":{"pageSize":25}}' \
  127.0.0.1:50051 \
  lxmf.command.v1.CommandService/ListCommandSessions
```

Send a message:

```bash
grpcurl \
  -plaintext \
  -d '{
    "id":"msg-1",
    "source":"node-a",
    "destination":"node-b",
    "title":"Test",
    "content":"hello"
  }' \
  127.0.0.1:50051 \
  lxmf.delivery.v1.DeliveryService/Send
```

Get delivery status:

```bash
grpcurl \
  -plaintext \
  -d '{"messageId":"msg-1"}' \
  127.0.0.1:50051 \
  lxmf.delivery.v1.DeliveryService/GetStatus
```

Cancel a message:

```bash
grpcurl \
  -plaintext \
  -d '{"messageId":"msg-1"}' \
  127.0.0.1:50051 \
  lxmf.delivery.v1.DeliveryService/Cancel
```

Negotiate:

```bash
grpcurl \
  -plaintext \
  -d '{
    "supportedContractVersions":[2],
    "requestedCapabilities":[],
    "config":{
      "profile":"desktop-local-runtime",
      "bindMode":"local_only",
      "authMode":"local_trusted",
      "overflowPolicy":"reject"
    }
  }' \
  127.0.0.1:50051 \
  lxmf.runtime.v1.RuntimeService/Negotiate
```

List interfaces:

```bash
grpcurl \
  -plaintext \
  -d '{}' \
  127.0.0.1:50051 \
  lxmf.admin.v1.InterfaceAdminService/ListInterfaces
```

Set interfaces:

```bash
grpcurl \
  -plaintext \
  -d '{
    "interfaces":[
      {
        "type":"tcp_client",
        "enabled":true,
        "host":"127.0.0.1",
        "port":4242,
        "name":"primary"
      }
    ]
  }' \
  127.0.0.1:50051 \
  lxmf.admin.v1.InterfaceAdminService/SetInterfaces
```

Reload config with explicit desired interfaces:

```bash
grpcurl \
  -plaintext \
  -d '{
    "desiredInterfaces":{
      "interfaces":[
        {
          "type":"tcp_client",
          "enabled":true,
          "host":"127.0.0.1",
          "port":4242,
          "name":"primary"
        }
      ]
    }
  }' \
  127.0.0.1:50051 \
  lxmf.admin.v1.InterfaceAdminService/ReloadConfig
```

Reload config with legacy no-params semantics:

```bash
grpcurl \
  -plaintext \
  -d '{}' \
  127.0.0.1:50051 \
  lxmf.admin.v1.InterfaceAdminService/ReloadConfig
```

Create topic:

```bash
grpcurl \
  -plaintext \
  -d '{"topicPath":"tak/alpha"}' \
  127.0.0.1:50051 \
  lxmf.topics.v1.TopicService/CreateTopic
```

Get topic:

```bash
grpcurl \
  -plaintext \
  -d '{"topicId":"topic-1"}' \
  127.0.0.1:50051 \
  lxmf.topics.v1.TopicService/GetTopic
```

List topics:

```bash
grpcurl \
  -plaintext \
  -d '{"page":{"pageSize":25}}' \
  127.0.0.1:50051 \
  lxmf.topics.v1.TopicService/ListTopics
```

List topics using a continuation token:

```bash
grpcurl \
  -plaintext \
  -d '{"page":{"pageSize":25,"pageToken":"topic:25"}}' \
  127.0.0.1:50051 \
  lxmf.topics.v1.TopicService/ListTopics
```

Subscribe to a topic:

```bash
grpcurl \
  -plaintext \
  -d '{"topicId":"topic-1"}' \
  127.0.0.1:50051 \
  lxmf.topics.v1.TopicService/SubscribeTopic
```

Publish to a topic:

```bash
grpcurl \
  -plaintext \
  -d '{
    "topicId":"topic-1",
    "payload":{"kind":"note","value":"hello"},
    "correlationId":"corr-1"
  }' \
  127.0.0.1:50051 \
  lxmf.topics.v1.TopicService/PublishTopic
```

List markers:

```bash
grpcurl \
  -plaintext \
  -d '{"page":{"pageSize":25}}' \
  127.0.0.1:50051 \
  lxmf.markers.v1.MarkerService/ListMarkers
```

List peers:

```bash
grpcurl \
  -plaintext \
  -d '{}' \
  127.0.0.1:50051 \
  lxmf.peers.v1.PeerService/ListPeers
```

Sync a peer:

```bash
grpcurl \
  -plaintext \
  -d '{"peerId":"peer-a"}' \
  127.0.0.1:50051 \
  lxmf.peers.v1.PeerService/SyncPeer
```

Create marker:

```bash
grpcurl \
  -plaintext \
  -d '{
    "label":"checkpoint-alpha",
    "position":{"lat":45.4215,"lon":-75.6972,"altM":70}
  }' \
  127.0.0.1:50051 \
  lxmf.markers.v1.MarkerService/CreateMarker
```

Store attachment:

```bash
grpcurl \
  -plaintext \
  -d '{
    "name":"brief.txt",
    "contentType":"text/plain",
    "bytesBase64":"aGVsbG8="
  }' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/StoreAttachment
```

Get attachment:

```bash
grpcurl \
  -plaintext \
  -d '{"attachmentId":"attachment-1"}' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/GetAttachment
```

Delete attachment:

```bash
grpcurl \
  -plaintext \
  -d '{"attachmentId":"attachment-1"}' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/DeleteAttachment
```

Download attachment:

```bash
grpcurl \
  -plaintext \
  -d '{"attachmentId":"attachment-1"}' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/DownloadAttachment
```

Start chunked upload:

```bash
grpcurl \
  -plaintext \
  -d '{
    "name":"stream.bin",
    "contentType":"application/octet-stream",
    "totalSize":11,
    "checksumSha256":"b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
  }' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/UploadStart
```

Send upload chunk:

```bash
grpcurl \
  -plaintext \
  -d '{
    "uploadId":"upload-1",
    "offset":0,
    "bytesBase64":"aGVsbG8gd28="
  }' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/UploadChunk
```

Commit upload:

```bash
grpcurl \
  -plaintext \
  -d '{"uploadId":"upload-1"}' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/UploadCommit
```

Download chunk:

```bash
grpcurl \
  -plaintext \
  -d '{
    "attachmentId":"attachment-1",
    "offset":0,
    "maxBytes":5
  }' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/DownloadChunk
```

List attachments:

```bash
grpcurl \
  -plaintext \
  -d '{"page":{"pageSize":25}}' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/ListAttachments
```

List attachments for a topic using a continuation token:

```bash
grpcurl \
  -plaintext \
  -d '{
    "topicId":"topic-1",
    "page":{"pageSize":25,"pageToken":"attachment:25"}
  }' \
  127.0.0.1:50051 \
  lxmf.attachments.v1.AttachmentService/ListAttachments
```

Poll events:

```bash
grpcurl \
  -plaintext \
  -d '{"max":16}' \
  127.0.0.1:50051 \
  lxmf.events.v1.EventService/PollEvents
```

Poll events with a cursor:

```bash
grpcurl \
  -plaintext \
  -d '{"cursor":"<next-cursor>","max":16}' \
  127.0.0.1:50051 \
  lxmf.events.v1.EventService/PollEvents
```

Subscribe to live events:

```bash
grpcurl \
  -plaintext \
  127.0.0.1:50051 \
  lxmf.events.v1.EventService/SubscribeEvents
```

List identities:

```bash
grpcurl \
  -plaintext \
  -d '{}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/ListIdentities
```

Activate identity:

```bash
grpcurl \
  -plaintext \
  -d '{"identity":"node-b"}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/ActivateIdentity
```

Import identity:

```bash
grpcurl \
  -plaintext \
  -d '{"bundleBase64":"<base64 bundle>"}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/ImportIdentity
```

Export identity:

```bash
grpcurl \
  -plaintext \
  -d '{"identity":"node-b"}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/ExportIdentity
```

Resolve identity:

```bash
grpcurl \
  -plaintext \
  -d '{"hash":"node-b-pub"}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/ResolveIdentity
```

Announce now:

```bash
grpcurl \
  -plaintext \
  -d '{}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/AnnounceNow
```

Update contact:

```bash
grpcurl \
  -plaintext \
  -d '{
    "identity":"node-b",
    "displayName":"Node Bravo",
    "trustLevel":"trusted",
    "bootstrap":true,
    "metadata":{"source":"manual"}
  }' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/UpdateContact
```

List contacts:

```bash
grpcurl \
  -plaintext \
  -d '{"page":{"pageSize":25}}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/ListContacts
```

List presence:

```bash
grpcurl \
  -plaintext \
  -d '{"page":{"pageSize":25}}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/ListPresence
```

Bootstrap identity:

```bash
grpcurl \
  -plaintext \
  -d '{"identity":"node-b","autoSync":true}' \
  127.0.0.1:50051 \
  lxmf.identity.v1.IdentityService/BootstrapIdentity
```

## Auth Modes

gRPC uses the same runtime auth policy as HTTP RPC.

### `local_trusted`

- loopback access only
- no authorization metadata required

### `token`

- remote access allowed when the runtime has negotiated `auth_mode=token`
- pass the bearer token as gRPC metadata:

```bash
grpcurl \
  -plaintext \
  -H 'authorization: Bearer <token>' \
  -d '{"includeCounts":true}' \
  127.0.0.1:50051 \
  lxmf.runtime.v1.RuntimeService/GetSnapshot
```

### `mtls`

- remote access allowed when the runtime has negotiated `auth_mode=mtls`
- gRPC transport must use TLS and present a client certificate matching the
  negotiated SAN/client-cert policy

```bash
grpcurl \
  -cacert /path/ca.pem \
  -cert /path/client.pem \
  -key /path/client.key \
  -d '{"includeCounts":true}' \
  127.0.0.1:50051 \
  lxmf.runtime.v1.RuntimeService/GetSnapshot
```

## Notes

- `SetInterfaces` and `ReloadConfig` mirror the current interface management
  behavior in the daemon.
- restart-required conditions are surfaced in the gRPC response body instead of
  as transport-level failures.
- interface `settings` round-trip through protobuf `Struct`, which normalizes
  numeric values to protobuf numbers.
- `SubscribeEvents` is live-stream only right now. For replay or cursor resume,
  use `PollEvents`.
