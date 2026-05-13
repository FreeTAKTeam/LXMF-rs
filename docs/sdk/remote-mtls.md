# SDK Remote mTLS Example

This guide shows the smallest remote RPC setup that uses transport-bound mTLS
instead of bearer token authentication.

## When to Use mTLS

Use mTLS when SDK clients connect to `reticulumd` over a non-loopback TCP
address and the deployment already has an operator-managed certificate
authority.

Use the default Unix socket instead when the SDK and daemon run on the same
host. Do not expose remote TCP with `local_trusted` auth.

## Certificate Inputs

The daemon needs:

- server certificate and private key
- client CA bundle used to verify SDK client certificates

Each SDK client needs:

- CA bundle that validates the daemon server certificate
- client certificate and private key when the daemon requires client certs

Keep private keys out of command-line arguments and logs. Store them through the
service manager, host secret store, or another local secret injector.

## Start `reticulumd`

Remote TCP binds are refused unless token auth or mTLS client authentication is
configured. For mTLS, provide the server material and client CA at startup:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 0.0.0.0:4242 \
  --rpc-tls-cert /etc/reticulumd/server.pem \
  --rpc-tls-key /etc/reticulumd/server.key \
  --rpc-tls-client-ca /etc/reticulumd/client-ca.pem
```

For production, prefer a service-manager unit and restrictive file permissions
for `/etc/reticulumd/server.key`.

## Configure the SDK Client

Configure the RPC backend with a TCP endpoint and mTLS auth. The client
certificate and key must be configured together when client authentication is
required.

```rust
use lxmf_sdk::app::{Client, Config};

#[tokio::main]
async fn main() -> Result<(), lxmf_sdk::app::Error> {
    let client = Client::rpc("reticulumd.example.net:4242");
    let mut config = Config::desktop_default();
    config.sdk_config = config
        .sdk_config
        .with_mtls_auth("/etc/lxmf/client/daemon-ca.pem")
        .with_mtls_client_credentials(
            "/etc/lxmf/client/client.pem",
            "/etc/lxmf/client/client.key",
        );

    let _handle = client.runtime().start_async(config).await?;
    Ok(())
}
```

The SDK rejects mTLS auth over Unix socket endpoints. mTLS is transport auth,
so authorization is evaluated from TLS peer-certificate state rather than
request headers.

## Event Streams

Native SDK event streams work over the same mTLS endpoint. Subscribe once after
startup and use cursor recovery only after `StreamGapDetected` or reconnect:

```rust
use lxmf_sdk::app::SubscriptionStart;

let mut events = client.events().subscribe(SubscriptionStart::Tail)?;
```

See `docs/sdk/lifecycle-and-events.md` for cursor and recovery semantics.

## Rotation and Recovery

Plan rotation before enabling remote RPC:

- overlap old and new client CA bundles during rollout
- restart or reconfigure clients after replacing client cert/key files
- treat expired or missing client certs as typed SDK security failures
- verify `/healthz`, `/readyz`, and `/metrics` after daemon restart

Operational deployment examples live in
`docs/runbooks/reticulumd-operational-deployment.md`.
