# Interop Golden Corpus v1

This directory stores stable, versioned golden artifacts used to validate
cross-client interoperability semantics.

## Files

- `golden-corpus.json`: canonical corpus for Sideband, RCH, and Columba send flow artifacts.

## Validation

The corpus is validated by:

- `cargo test -p test-support sdk_interop_corpus -- --nocapture`
- `cargo run -p xtask -- interop-corpus-check`

The corpus schema is:

- `docs/schemas/contract-v2/interop-golden-corpus.schema.json`
