# Native Embedded Node Config v1

## Scope

Defines the persisted node configuration schema for the standalone embedded node.

## Schema Version

- version: `1`
- unknown future version behavior: fall back into BLE recovery mode

## Stored Fields

### Identity

- `store_identity[32]`
- `lxmf_address[16]`

### Node mode

- `ble_only`
- `tcp_client`
- `tcp_server`

### Wi-Fi

- `ssid`
- `password`

### TCP client

- `host`
- `port`
- reconnect backoff uses the interop profile defaults

### TCP server

- `listen_port`

### Runtime

- `announce_interval_ms`
- `capture_default_max_bytes`
- `ble_recovery_enabled`

## Lifecycle coupling

- missing Wi-Fi credentials with `tcp_client` or `tcp_server` mode => `Unprovisioned`
- valid Wi-Fi config but no active link => `ProvisionedOffline`
- active TCP link => `TcpOnline`
- explicit local recovery session => `BleRecovery`

## Safety rules

- credentials are persisted on device only
- unknown schema or invalid mode must not start TCP automatically
- invalid persisted config falls back to BLE recovery mode
