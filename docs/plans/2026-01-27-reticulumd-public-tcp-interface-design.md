# Reticulumd Public TCP Interface (rmap.world:4242) Design

## Goal
Add an opt-in Weft setting that connects to the public Reticulum TCP interface at `rmap.world:4242` so Weft can see/communicate with public peers.

## Architecture
- Weft setting toggles a daemon config describing extra TCP client interfaces.
- reticulumd reads this config at startup to spawn TCP client interfaces.
- Weft restarts reticulumd when the toggle changes.

## Data Flow
1) User enables “Public Network (rmap.world:4242)” in Weft.
2) Weft writes/updates daemon config file.
3) Weft restarts reticulumd with `--config <path>`.
4) reticulumd starts TCPClient to `rmap.world:4242` and processes announces/traffic.
5) Weft UI receives announce/peer events and can message public peers.

## Config Format
Proposed minimal config file (TOML or JSON):
```
interfaces = [
  { type = "tcp_client", enabled = true, host = "rmap.world", port = 4242, name = "Public RMap" }
]
```

## Error Handling
- TCP connect failure: log warning, Weft shows “Disconnected”, toggle remains enabled.
- Invalid config: reticulumd falls back to local transport and logs error.
- Restart failure: Weft shows error and reverts toggle.

## Testing
- Unit test reticulumd config parser (valid/invalid/disabled).
- Integration test: enable toggle -> reticulumd attempts connect.
- Manual test: verify announces and messaging via public interface.
