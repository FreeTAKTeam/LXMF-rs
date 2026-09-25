# Reticulum 1.5.4-dev feature update plan

The existing RNS 1.5.2-compatible system is the tested, working baseline. This
plan updates it for features and changed behavior in the pinned Python RNS
1.5.4 development snapshot, not for blanket parity with every inherited
Python behavior. The active release baseline remains RNS 1.5.2.

## Reference and scope

- Keep the active RNS 1.5.2 revision
  `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6` and the forward 1.5.4-dev
  revision `99de23c040d507e3fefca19e87b182302902725d` pinned. Changing
  either requires a separately reviewed decision.
- Use [`rns-1.5.4-delta.md`](../../status/rns-1.5.4-delta.md) to identify actual
  additions and behavior changes between those revisions, including the
  HDLC, BLE, and rngit changes already tracked there. Record which update
  items are already integrated and which still need work. The ledger's broader
  parity-gap inventory is not, by itself, the #605 completion checklist.
- Preserve working LXMF, SDK, ZeroMQ, daemon, and utility paths. Additive
  public-interface changes are allowed only where a specific feature needs
  them; avoid replacement protocols, daemons, or broad rewrites.

## Delivery

1. Confirm the remaining feature delta against the pinned reference and the
   existing ledger; keep a short checklist in #605.
2. Implement remaining items in small changes on existing owner paths. Update
   the relevant parity/status documentation with each behavior change.
3. Run focused regressions for affected paths and pinned-Python
   interoperability checks where wire or runtime behavior changes. Run normal
   repository CI on the combined candidate and resolve any regressions.
4. Close #605 when the checklist is integrated, those checks pass on the
   combined head, and remaining gaps are accurately documented. A new release
   or a fresh full-parity certification is not required solely for this issue.

Open #608, #609, and #611–#614 remain linked parity follow-ups, not automatic
blockers unless a gap directly breaks the selected feature update or regresses
the tested baseline. Their status is not changed by closing #605. Closed
#606, #607, #610, and #615 retain their historical evidence; their old
full-parity wording is not a new completion gate. Physical, client, and
public-network evidence under #616 remains separately tracked and outside
this goal.
