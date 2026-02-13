# Contract V2 Shared Fixtures

`payload-domains.json` is the semantic fixture shared with Weft for parser parity.

Coverage includes:

- Message list envelope and canonical field domains (`0x02`, `0x05`, `0x09`, `0x0C`, `0x0E`, `0x10`)
- Attachment and paper metadata domains
- Announce, peer, interface, and propagation node snapshots
- Outbound propagation selection response
- Delivery trace transitions
- Runtime event payload sample

This file must stay byte-identical in:

- `LXMF-rs/docs/fixtures/contract-v2/payload-domains.json`
- `Weft-Web/docs/fixtures/contract-v2/payload-domains.json`
