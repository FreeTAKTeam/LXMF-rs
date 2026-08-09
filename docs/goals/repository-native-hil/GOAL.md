# Repository-Native HIL Controller

## Outcome

LXMF-rs has one repository-owned hardware-in-the-loop controller with three
execution levels:

- pr: deterministic virtual and pinned Python-reference checks;
- nightly: the complete configured profile matrix, including physical
  adapters and prepared-host suites;
- release: the same matrix with release evidence retained for support and
  compatibility claims.

Every run emits machine-readable JSON, JUnit, raw command logs, the commit
under test, the random seed, and an explicit result class. Missing lab
provisioning is reported as BLOCKED; it is never converted into a passing
result.

## Acceptance evidence

The first live hardware acceptance run must cover two provisioned RNodes and
prove discovery, link establishment, payload/resource transfer, forwarding,
and reset/recovery. Other profiles remain visible in the support matrix and
are honestly blocked until their runner identity, endpoint, firmware, and
reset configuration are provisioned.
