# `lxmf_rpc_chat_app`

Minimal Flutter UI smoke app for the public `lxmf_sdk_app` RPC surface.

What it demonstrates:

- connect to a local `reticulumd` RPC endpoint
- request the identity and contact capabilities the demo uses
- inspect local identity, contact count, and message history
- stream a single-peer conversation through `RpcConversationClient`
- send a message and surface the initial receipt state

Run from this directory:

```sh
flutter pub get
flutter run -d macos
```

Before pressing `Connect`, start `reticulumd` in a separate terminal from the
repo root:

```sh
cargo run -p reticulumd --bin reticulumd -- --rpc 127.0.0.1:4543 --db /tmp/lxmf-rpc-chat-app.db --announce-interval-secs 0
```

Defaults:

- RPC endpoint: `http://127.0.0.1:4543/rpc`
- peer destination: `0123456789abcdef0123456789abcdef`

The app is intentionally narrow in scope. It is an experimental UI smoke
harness for the current RPC-backed client path, not a production chat client.
