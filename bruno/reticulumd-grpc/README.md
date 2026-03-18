# Bruno gRPC Collection

This folder is a Bruno collection shell for the live `reticulumd` gRPC API.

This collection now includes saved unary gRPC requests in Bruno's native
request-file format.

## Open in Bruno

```bash
open -a Bruno "$(pwd)/bruno/reticulumd-grpc"
```

## Run the Daemon

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 127.0.0.1:4243 \
  --grpc 127.0.0.1:50051 \
  --db reticulum.db
```

## In Bruno

Use:

- host: `127.0.0.1:50051`
- reflection: enabled

If Bruno asks for proto import paths, use:

- `api/proto` from the repository root

Included requests:

- `runtime/Get Snapshot`
- `runtime/Negotiate Local Trusted`
- `command/Invoke Command`
- `command/Get Command Session`
- `command/List Command Sessions`
- `delivery/Send Message`
- `delivery/Get Message Status`
- `delivery/Cancel Message`
- `admin/List Interfaces`
- `admin/Reload Config`
- `topics/List Topics First Page`
- `topics/Get Topic`
- `topics/Subscribe Topic`
- `topics/Publish Topic`
- `topics/Create Topic`
- `attachments/List Attachments First Page`
- `attachments/Store Attachment`
- `attachments/Download Attachment`
- `events/Poll Events`
- `identity/List Identities`
- `identity/Activate Identity`
- `identity/Import Identity`
- `identity/Export Identity`
- `identity/Announce Now`
- `markers/List Markers First Page`
- `markers/Create Marker`
- `peers/List Peers`
- `peers/Sync Peer`
- `peers/Unpeer`
- `peers/Clear Peers`

Example payloads and usage notes live in:

- `docs/grpc-getting-started.md`
- `docs/runbooks/grpc.md`

If you save more request variants, I can extend the collection the same way.
