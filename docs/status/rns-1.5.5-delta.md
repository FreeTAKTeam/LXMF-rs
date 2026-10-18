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
| Link request timeout and rejection can fail receipts still in `SENT` state | `rns-transport` Link/request path | No equivalent retained state at v0.13.0. The later item-4 follow-up adds a Link-owned receipt book, native phases/cleanup and exact-target guard evidence; see REQUESTS.md below. |
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

The discovery library planner applies the 1.5.5 safe default. The later item-4
native checkpoint now consumes it in the existing interface mutation worker and
creates prepared live clients; the three exact-target Python process traces
remain pending. `rnstatus --discovered` shows filtered human
rows while JSON retains the unfiltered discovery set, matching the Python
display boundary. The Rust publisher already requires `publish_ifac` and
configured IFAC fields; encoding now drops empty or literal `None` values. The
sample I2P discovery entry already uses `.b32.i2p`.

The item-4 preparation now reads startup configuration once for all typed views,
with a private, independently falling-back unverified override. An additive
policy-aware initial planner reserves names and applies endpoint exclusions.
A controlled exact-target comparison passes 78 qualification cases, nine
endpoints and sequential name collisions; large numeric version components
retain Python's ordering against the fixed 1.5.2 minimum. The override now feeds
native admission, initial/persisted/periodic paths and monitoring. Bounded native
tests cover exact packet provenance, blackholes, operator suppression, contention
and teardown; trusted bootstrap composition covers four managed kinds plus
private Backbone clients. Broader family recovery and target-process evidence
remain open in [DISCOVERY.md](../goals/open-issues-2026-10/DISCOVERY.md). The Rust
publisher retains its honest LXMF-rs implementation metadata.

Local-client forced bitrate is applied before the accepted stream worker runs,
and virtual children inherit their parent's initial announce cap and policy.
At v0.13.0 Rust had no equivalent retained pending Link request state. The later
item-4 follow-up adds the native receipt book and packet/Resource owner described
in [REQUESTS.md](../goals/open-issues-2026-10/REQUESTS.md), with exact 1.5.5
controlled guard comparisons, native signed ACK/Resource phase tests and real
rncp/rngit baseline/target traces. These bounded software checks do not complete
the broader transport/discovery acceptance. The subsequent common child-policy
slice also propagates live mode, outgoing, gravity and announce pacing through
registered descendants, with native dispatch/queue/routing controls. Existing
queue timestamps/deadlines and PathTable entries remain committed. This qualifies
the shared owner, not all carrier-specific lifecycle or Python process transitions;
those boundaries are not hidden by the 1.5.5 feature pin.

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
