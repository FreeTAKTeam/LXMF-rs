# Run `lxmd` with systemd

Status: active

This guide installs `lxmd` and `reticulumd` as a single long-running Linux
service. `lxmd` launches `reticulumd`, so most deployments need only one unit.
For a direct `reticulumd` service, use the
[`reticulumd` operational deployment guide](reticulumd-operational-deployment.md).

## Install the binaries

Build from source:

```bash
cargo build --release -p lxmf-cli --bin lxmd -p reticulumd --bin reticulumd
sudo install -m 0755 target/release/lxmd /usr/local/bin/lxmd
sudo install -m 0755 target/release/reticulumd /usr/local/bin/reticulumd
```

You can instead use the matching Linux archive or package from the
[latest GitHub release](https://github.com/FreeTAKTeam/LXMF-rs/releases/latest).
Verify the downloaded asset against the published checksums before installing
it.

## Create the service account and configuration

```bash
sudo useradd --system --create-home --shell /usr/sbin/nologin lxmd
sudo mkdir -p /etc/lxmf/lxmd /etc/lxmf/reticulumd /var/log/lxmf
sudo chown -R lxmd:lxmd /etc/lxmf /var/log/lxmf
sudo -u lxmd /usr/local/bin/lxmd --exampleconfig > /etc/lxmf/lxmd/config
sudo chmod 600 /etc/lxmf/lxmd/config
```

To use an explicit Reticulum configuration instead of generated defaults:

```bash
sudo cp crates/apps/reticulumd/examples/service-reference.toml \
  /etc/lxmf/reticulumd/config.toml
sudo chown lxmd:lxmd /etc/lxmf/reticulumd/config.toml
```

## Install the unit

Create `/etc/systemd/system/lxmd.service` with:

```ini
[Unit]
Description=LXMF daemon (lxmd + reticulumd)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=lxmd
Group=lxmd
WorkingDirectory=/etc/lxmf/lxmd
ExecStart=/usr/local/bin/lxmd --config /etc/lxmf/lxmd/config --rnsconfig /etc/lxmf/reticulumd/config.toml
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

If you are relying on generated Reticulum defaults, remove
`--rnsconfig /etc/lxmf/reticulumd/config.toml` from `ExecStart`.

Enable and start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now lxmd.service
sudo systemctl status lxmd.service --no-pager
```

## Separate daemon and RCH services

For RCH deployments with a separate `reticulumd` process, use this unit as
`lxmd.service` in place of the combined launcher above. Add the existing daemon
arguments for your database, identity, configuration and ZeroMQ endpoints to
`ExecStart`:

```ini
[Unit]
Description=Reticulum daemon for RCH
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
NotifyAccess=main
TimeoutStartSec=1800
User=lxmd
Group=lxmd
WorkingDirectory=/etc/lxmf/lxmd
ExecStart=/usr/local/bin/reticulumd --config /etc/lxmf/reticulumd/config.toml --rpc-unix /run/lxmd/rpc.sock
RuntimeDirectory=lxmd
RuntimeDirectoryMode=0700
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

The daemon sends `READY=1` only after all configured RPC sockets and ZeroMQ
listeners bind. The startup timeout allows large database and route restoration;
size it for the deployment. It sends `STOPPING=1` on SIGINT or SIGTERM.
Add these dependencies to the separate RCH unit:

```ini
[Unit]
After=lxmd.service
Requires=lxmd.service
```

The combined `lxmd` launcher above stays `Type=simple`: `NotifyAccess=main`
would reject notifications from its `reticulumd` child. Use the direct daemon
unit when RCH startup must wait for daemon readiness.

## Inspect and troubleshoot

Follow the service log with:

```bash
sudo journalctl -u lxmd.service -f
```

Configuration and runtime failures should be visible in the journal. Use the
[logging and diagnostics guide](logging-and-diagnostics.md) for log filters
and failure context, and the [CLI reference](../lxmf-cli.md) for operator
commands.
