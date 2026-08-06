# LXMF-rs v0.9.8

v0.9.8 is a patch release over the published v0.9.7 baseline. It promotes the
reviewed post-v0.9.7 candidate beginning at `9f12fb4e` and makes no new broad
Python, hardware, public-network, or third-party-client compatibility claim.

## Highlights

- Resource transfers now apply the Python-shaped request-window hashmap gate,
  adaptive fragment scheduling, split-resource metadata handling, and
  outbound opportunistic bz2 compression while retaining cancellation,
  timeout, cleanup, and whole-resource completion behavior.
- Link payload and resource fragment sizing now follows the negotiated MTU;
  request/response packet helpers complete the small-payload control path.
- Propagation storage, fan-out, peer maintenance, ticket expiry, and remote
  fetch/download failure handling are bounded and observable through the typed
  daemon/SDK surfaces.
- ZeroMQ SDK requests are bounded by the configured SDK deadline, resource
  preparation happens before the handler lock, and runtime lock scopes avoid
  holding handler state across expensive transfer work.

## Compatibility boundary

The release preserves the maintained parity limits. The seven LXMF module rows
remain complete for their named software scenarios. The pinned RNS 1.4.2
inventory remains 1,695 complete, 115 partial, and 1 not applicable;
`RNS/Resource.py` remains partial because the Rust sender does not implement
the Python receiver serving-window collision guard. Hardware, public-network,
and external-client compatibility remain separate evidence tracks.

## Validation and publication

The exact candidate SHA, local command results, hosted CI URLs, pinned-Python
interop results, RC/final tags, package versions, bundle checksums, and known
exceptions are recorded in
[`docs/status/v0.9.8-release-candidate.md`](status/v0.9.8-release-candidate.md).
The immutable release SHA is the tag target recorded there after promotion.
