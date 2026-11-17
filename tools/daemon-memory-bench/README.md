# Daemon memory comparison harness

This is an experimental smoke harness for the RCH Ubuntu comparison plan. It
runs one Python `lxmd` or Rust `reticulumd` at a time with a local-only network
namespace, a dedicated cgroup, fixed-rate propagated messages and either an
offline or periodically polling recipient. It retains samples and message
ledgers outside this checkout.

Run validation tests with `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/daemon-memory-bench -p 'test_*.py'`.
Run a frozen, externally prepared manifest with:

```sh
tools/daemon-memory-bench/run.sh /absolute/path/experiment.json python smoke-python-1
tools/daemon-memory-bench/run.sh /absolute/path/experiment.json rust smoke-rust-1
```

The runs must be sequential. The prepared-packet online profile generates fresh
signed, encrypted, stamped messages before daemon startup, then records actual
packet submissions, protocol proofs, receiver deliveries and peer queue drops.
It excludes the cost of generating stamps during the measured interval. Use
fresh isolated guests and the frozen host launcher for runtime-file cache
control; raw cgroup memory alone is not a complete operational budget.
The high-load profile uses a fresh link for each receiver poll, records failed
or timed-out polls, and retries at later scheduled slots. Its sender waits for
proofs up to the frozen confirmation target without retransmitting packets.

Populated fixtures, peer churn, multi-pair reporting, soak and production
workloads are not implemented. Idle validation checks loopback counters; it does
not observe traffic over Unix sockets. Use each frozen run's own report and
validator, and do not interpret a single pair as completion of the comparison
plan.
