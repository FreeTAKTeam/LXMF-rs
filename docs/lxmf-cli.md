# LXMF CLI Quick Reference

This note documents the `lxmf` operator CLI implemented by the `lxmf-cli`
crate.

The same crate also ships `lxmd`, which is a separate daemon-compatibility
entrypoint. This page is about `lxmf`, not `lxmd`.

## Invocation

```bash
cargo run -p lxmf-cli --bin lxmf -- --help
cargo run -p lxmf-cli --bin lxmd -- --help
```

## Global Flags

- `--rpc <addr>`: RPC endpoint (default `unix:/tmp/lxmf-rpc.sock`)
- `--profile <desktop-full|desktop-local-runtime|embedded-alloc>`
- `--bind-mode <local_only|remote>`
- `--auth-mode <local_trusted|token|mtls>`
- `--output <human|json|json-pretty>`: output mode
- `--json`: legacy alias for `--output json-pretty`
- `--quiet`: suppress non-error output

Auth-specific flags:

- token: `--token-issuer`, `--token-audience`, `--token-shared-secret`
- mTLS: `--mtls-ca-bundle-path`, `--mtls-require-client-cert`, `--mtls-allowed-san`

## Commands

- `start`
- `send --source --destination [--content|--payload-json]`
  `[--delivery-method <direct|propagated|paper>] [--stamp-cost <cost>]`
  `[--include-ticket] [--try-propagation-on-fail]`
- `cancel --message-id`
- `status --message-id`
- `paper-encode --message-id`
- `paper-decode --uri [--transient-id] [--destination-hint]`
- `poll [--cursor] [--max]`
- `snapshot`
- `configure --expected-revision --patch-json`
- `shutdown --mode <graceful|immediate>`
- `tick [--max-work-items] [--max-duration-ms]`
- `completions --shell <bash|zsh|fish|powershell|elvish>`

## Examples

Start runtime and send a message:

```bash
cargo run -p lxmf-cli --bin lxmf -- start
cargo run -p lxmf-cli --bin lxmf -- send \
  --source example.service \
  --destination example.peer \
  --content "hello from lxmf-cli"
```

Send with explicit delivery options:

```bash
cargo run -p lxmf-cli --bin lxmf -- send \
  --source example.service \
  --destination example.peer \
  --content "hello from lxmf-cli" \
  --delivery-method direct \
  --stamp-cost 8 \
  --include-ticket \
  --try-propagation-on-fail
```

Encode and decode paper-message URIs through the SDK-backed daemon paper
surface:

```bash
cargo run -p lxmf-cli --bin lxmf -- paper-encode --message-id queued-message-id
cargo run -p lxmf-cli --bin lxmf -- paper-decode --uri "lxm://..."
```

Poll events in human mode:

```bash
cargo run -p lxmf-cli --bin lxmf -- poll --max 32
```

Poll events in machine mode:

```bash
cargo run -p lxmf-cli --bin lxmf -- --output json poll --max 32
```

Generate shell completions:

```bash
cargo run -p lxmf-cli --bin lxmf -- completions --shell zsh > _lxmf
```

Related references:

- `README.md`
- `docs/contracts/sdk-v2.md`
- `docs/contracts/rpc-contract.md`
