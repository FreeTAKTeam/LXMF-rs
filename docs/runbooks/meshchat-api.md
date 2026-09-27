# MeshChat API on reticulumd

`reticulumd` can expose a local MeshChat HTTP and WebSocket API from the dedicated
`meshchat-api` crate. The source contract is in `docs/contracts/meshchat/`.

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --db /path/to/reticulum.db \
  --config /path/to/reticulumd/config.toml \
  --meshchat-bind 127.0.0.1:8000 \
  --meshchat-assets /path/to/reticulum-meshchat/src/frontend/public
```

The bind is opt-in and must be loopback. The frontend assets must be a built
`public/` directory; the server uses that directory for `/` and static files.
Configure at least one network interface for live peers; see the
[UDP interface runbook](reticulumd-udp-interface.md) for a local example.
With no assets path, the API still runs but there is no browser entry page.
Local UI settings, favourites, name overrides, and read timestamps persist to
`/path/to/reticulum.meshchat.json` beside the selected daemon database. A
corrupt state file stops the API at startup instead of silently discarding it.

## Current compatibility boundary

The following operations use the active daemon state: status, local announce,
text and file/image send, inbound file/image/audio decoding,
conversation/history reads, send cancellation, durable message and
conversation deletion, route lookup/drop, stamp cost, stored signal readings,
read-only interface status, discovered propagation nodes, propagation status,
remote propagation fetch, and control WebSocket config/ping plus
inbound/outbound message events. The same
origin serves both REST and the control WebSocket. Local UI settings,
favourites, name overrides, and read state are persisted by the API crate.
The application shell's calls poll returns an empty list because this daemon
does not run an audio-call manager.

This is a **partial implementation** of the 2.4.0 contract. Every declared
route is registered, but operations without a matching daemon capability
return HTTP 501 with a JSON `message`. These include audio call actions and
binary audio WebSockets, NomadNet downloads/identify, serial port and interface
editing, transport toggles, path-table/interface statistics, delivery ping,
propagation sync cancellation, and audio attachment sends. Config PATCH rejects
keys that would need a daemon side effect. Other unsupported attachment kinds
are rejected explicitly.

Some available reads also remain partial: announce records lack the upstream
identity public key, aspect, and hop count; history uses the daemon's SQLite
row IDs, which are local to its message store; app info lacks
Python-specific paths/version data; and the daemon lacks MeshChat's call and
propagation-transfer state. When no transport is configured, path reads return
`null`; a configured transport is needed for path discovery and delivery.
Do not use this build as evidence of full API conformance or network
interoperability.

`cargo test -p meshchat-api` checks HTTP send/history/deletion, conversation
reads, and explicit unsupported behavior. A local daemon smoke check covered
HTTP status, static assets, WebSocket config/pong, and persistence across
restart. The pinned MeshChat 2.4.0 frontend built and loaded in Chromium; its
messages page opened a conversation, sent a text message, and displayed that
message after reload without browser console errors. The Rust proof of concept
was also exercised with TCP and a serial RNode against external chat peers;
text and small image exchange worked. One post-idle message was reported
delivered after about four minutes. These observations do not establish a
sustained delivery rate or full attachment interoperability.
