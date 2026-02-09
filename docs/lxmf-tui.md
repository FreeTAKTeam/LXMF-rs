# `lxmf tui`

The operator CLI includes an interactive terminal UI built with `ratatui` + `crossterm`.

Start it with:

```bash
cargo run --bin lxmf -- --profile <name> tui
```

## Panes

- Dashboard
- Messages
- Peers
- Interfaces
- Events
- Logs

## Keybindings

- `q`: quit
- `Tab`: next pane
- `j` / `Down`: move selection down (messages/peers/interfaces)
- `k` / `Up`: move selection up (messages/peers/interfaces)
- `s`: send message (interactive prompt)
- `y`: sync selected peer
- `u`: unpeer selected peer
- `a`: apply interfaces (`set_interfaces` + `reload_config`)
- `r`: restart daemon (managed profile)
- `n`: announce now
- `e`: force refresh

## Data Sources

The TUI refreshes from:

- `list_messages`
- `list_peers`
- `list_interfaces`
- `daemon_status_ex`
- `/events` polling
- profile `daemon.log`

## Notes

- The TUI is intended for operator workflows and daemon introspection.
- For scripted automation, prefer `lxmf --json ...` CLI subcommands.
