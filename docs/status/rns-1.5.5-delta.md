# Reticulum 1.5.5 feature update for LXMF-rs v0.13.0

The update target is the official Python Reticulum `1.5.5` tag, peeled to
`7f2b3b9b524c9386316379af1313b43a5e4f7a5d`. Compare that tag with the
official `1.5.4` tag for the changed behavior below. This is a feature-update
ledger, not a claim of complete Python Reticulum parity. The tested RNS 1.5.2
release baseline and the historical pinned 1.5.4-dev #605 feature record remain
separate until their reference policy is deliberately changed.

| Changed upstream behavior | Existing Rust owner | v0.13.0 work |
| --- | --- | --- |
| `rnstatus` named attach/detach/reload and `enable_interface_management` switch | `rns-tools` `rnstatus`; `reticulumd` configuration and hot-apply bridge; `rns-rpc` | v0.13.0 implements named operations for `tcp_client`, `tcp_server`, `udp`, and `pipe`, with local-start readiness, child-worker teardown, failure/rollback reporting, and focused CLI/RPC/loopback regressions. Other interface kinds remain explicit follow-ups. |
| Discovery auto-connect accepts only Backbone announcements from verified `RNS` 1.5.2+ by default, with explicit unverified override; `rnstatus` stale/unknown display is opt-in | `rns-transport` discovery; `reticulumd` policy; `rns-tools` `rnstatus` | Add implementation/version retention, safe qualification, override, display flags, and focused tests. |
| Empty/ambiguous `None` IFAC values are not published or adopted; I2P discovery config adds `.b32.i2p` | `rns-transport` discovery; `reticulumd` publish/config examples | Add IFAC sanitation and legacy-record regression. Rust I2P example already uses `.b32.i2p`; retain its test. |
| rngit permits Markdown download converted to `.mu`, and counts readable work documents in scope links | `rns-tools` `rngit_parts` page media and work pages | Add bounded conversion and permission-filtered counts/links with pinned-Python Link evidence. |
| Local client initializes owner and bitrate before connecting; detach stops listener/reconnect workers and spawned children inherit policy | `rns-transport` local and interface workers | Audit existing Rust lifecycle, add regressions only for uncovered equivalents. |
| Link request timeout and rejection can fail receipts still in `SENT` state | `rns-transport` Link/request path | Audit for equivalent Rust receipt states and add a focused regression or record API non-equivalence. |
| Default announce cap is a percentage, including virtual children | `rns-transport` interface manager | Existing 2% default and child inheritance are present; verify with focused tests. |

## Release evidence and remaining gaps

The focused Rust `rns_1_5_5` tests pass for discovery metadata, legacy IFAC
sanitation, Backbone auto-connect qualification, converted rngit downloads, and
readable workdoc counts. The full rngit binary suite passed (96 passed, 43
pre-existing pinned-reference tests ignored). The ignored, exact-tag Python
1.5.5 Link test was run locally against commit
`7f2b3b9b524c9386316379af1313b43a5e4f7a5d`: it received `README.mu` from
the Rust server with bytes matching Python's MarkdownToMicron on that fixture.
The same exact-tag check is now in normal Verify CI. This is one converted
download trace, not general Markdown renderer parity.

The discovery library planner applies the 1.5.5 safe default; production
`reticulumd` has no discovery auto-connect worker, so the configuration knobs
do not yet create live interfaces. `rnstatus --discovered` shows filtered human
rows while JSON retains the unfiltered discovery set, matching the Python
display boundary. The Rust publisher already requires `publish_ifac` and
configured IFAC fields; encoding now drops empty or literal `None` values. The
sample I2P discovery entry already uses `.b32.i2p`.

Local-client forced bitrate is applied before the accepted stream worker runs,
and virtual children inherit their parent's initial announce cap and policy.
Rust has no Python-style pending Link request-receipt state, so the 1.5.5
`SENT` timeout/rejection fix has no equivalent state transition to patch here;
that remains a broader API gap. Live changes to a parent's announce pacing do
not currently propagate to existing virtual children. Neither gap is hidden by
the 1.5.5 feature pin.

Named attach/detach/reload now has focused local tests for real TCP listener
operations, failed bind and pipe spawn, reload rollback, disabled policy,
duplicate/missing/unsupported names, RPC completion, and CLI error exits. The
new `manage_interface` RPC waits for completion; existing `set_interfaces`
still acknowledges queue admission before startup readiness and must not be
presented as the new feature. HTTP and both ZeroMQ ingress paths dispatch the
synchronous completion wait off Tokio's reactor; single-thread ZeroMQ
regressions cover the two paths. The built-in rngit
Markdown converter covers common syntax but not Python's full table and
syntax-highlighting behavior. The code-identical release candidate passed an
uninterrupted local `cargo xtask release-check`, including 3,021 nextest
tests. [PR #651](https://github.com/FreeTAKTeam/LXMF-rs/pull/651) merged; its
exact head passed normal CI, independent interoperability, and Verify.
The integrated commit `fbc75b86e15550722923b366398f0a4116182894`
passed [CI](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/37124456558)
and [Verify](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/37124456451).
The annotated `v0.13.0` tag peels to that commit. The
[GitHub release](https://github.com/FreeTAKTeam/LXMF-rs/releases/tag/v0.13.0)
is public, and tag-level
[independent interoperability](https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/37126480810)
passed with its bounded, published evidence.

The official [Reticulum changelog](https://github.com/markqvist/Reticulum/blob/1.5.5/Changelog.md)
highlights live interface management, rngit converted downloads/counts, corrected
I2P examples, and the local-client startup race. The source delta also includes
the discovery/IFAC, lifecycle, and Link changes above. Documentation/editorial
changes and Python-specific implementation details are not independent Rust
features. #608–#614 remain visible parity follow-ups; #616 physical, client,
and public-network acceptance stays outside this feature update.
