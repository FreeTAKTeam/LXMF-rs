# reticulumd Operational Deployment

Status: active
Applies to: `reticulumd` with RPC-backed `lxmf-sdk` clients

## Default Local Deployment

`reticulumd` listens on a local Unix socket by default and does not bind TCP
unless `--rpc` is provided.

```bash
cargo run -p reticulumd --bin reticulumd
```

Default endpoint:

```text
unix:/tmp/lxmf-rpc.sock
```

Probe health, readiness, and metrics over the Unix socket:

```bash
curl --unix-socket /tmp/lxmf-rpc.sock http://localhost/healthz
curl --unix-socket /tmp/lxmf-rpc.sock http://localhost/readyz
curl --unix-socket /tmp/lxmf-rpc.sock http://localhost/metrics
```

The daemon removes an existing socket at the configured path only when the path
is actually a Unix socket. It refuses to remove regular files or directories.
On graceful shutdown, the listener exits and removes the socket path.

## Optional TCP Deployment

Loopback TCP is for local development:

```bash
cargo run -p reticulumd --bin reticulumd -- --rpc 127.0.0.1:4242
```

Remote TCP binds are refused unless the daemon is configured with remote token
auth or mTLS client authentication.

First-run token auth:

```bash
export LXMF_RPC_TOKEN_SECRET='replace-with-a-generated-secret'
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 0.0.0.0:4242 \
  --rpc-token-issuer production-reticulumd \
  --rpc-token-audience lxmf-sdk \
  --rpc-token-secret-env LXMF_RPC_TOKEN_SECRET
```

mTLS remote bind:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 0.0.0.0:4242 \
  --rpc-tls-cert /etc/reticulumd/server.pem \
  --rpc-tls-key /etc/reticulumd/server.key \
  --rpc-tls-client-ca /etc/reticulumd/client-ca.pem
```

Do not place token secrets in command-line arguments. Use an environment
variable, a service manager secret facility, or another local secret injector.
For SDK client configuration, see `docs/sdk/remote-mtls.md`.

## Shutdown

`SIGINT`/Ctrl+C triggers graceful listener shutdown. The daemon stops accepting
new RPC connections and removes the Unix socket path if it owns a socket there.

For service managers, prefer normal termination first and only escalate to hard
kill after the configured stop timeout.

## systemd Example

```ini
[Unit]
Description=reticulumd LXMF daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/reticulumd --rpc-unix /run/reticulumd/lxmf-rpc.sock
RuntimeDirectory=reticulumd
Restart=on-failure
RestartSec=2
KillSignal=SIGINT
TimeoutStopSec=15
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

## launchd Example

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>org.lxmf.reticulumd</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/reticulumd</string>
    <string>--rpc-unix</string>
    <string>/tmp/lxmf-rpc.sock</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/tmp/reticulumd.out.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/reticulumd.err.log</string>
</dict>
</plist>
```

## Operational Signals

Use `/healthz` for process liveness and `/readyz` for readiness. Use `/metrics`
for request counts, RPC errors, auth failures, event drops, stream and send
latency histograms, queue pressure, and transport-specific counters.

Minimum deployment checks:

```bash
curl --unix-socket /tmp/lxmf-rpc.sock http://localhost/healthz
curl --unix-socket /tmp/lxmf-rpc.sock http://localhost/readyz
curl --unix-socket /tmp/lxmf-rpc.sock http://localhost/metrics
cargo run -p lxmf-cli -- --rpc unix:/tmp/lxmf-rpc.sock status
```
