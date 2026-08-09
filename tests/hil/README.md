# HIL lab contract

lab.toml and the case files are the repository-owned description of the HIL
matrix. They contain profile capabilities and environment variable names only.
Physical identity material, serial paths, endpoints, firmware revisions, reset
hooks, and adapter commands stay on the runner host.
Use stable host-side identifiers there: Linux /dev/serial/by-id paths, an
explicit Windows COM mapping, fixed Bluetooth addresses/service UUIDs, and
fixed Android ADB serials.

Run the controller from the repository root:

    cargo xtask hil doctor --all
    cargo xtask hil list
    cargo xtask hil run --level pr --all
    cargo xtask hil run --level nightly --profile heltec
    cargo xtask hil report

For a physical profile, the runner environment must provide the profile's
identity, endpoint, firmware version/hash, executor, and either a reset command or a
uhubctl hub/port mapping. RF profiles additionally require
HIL_RF_ENCLOSURE_CONFIRMED=true. Missing values produce BLOCKED evidence.
Nightly RF runs require a shielded enclosure or cabled path with attenuation
and dummy loads; do not use unattended open-air transmissions.
Set HIL_LOCK_PATH to the rack's shared lock location when more than one
runner can reach the same hardware; the controller also expires locks after
the configured lock_ttl_secs interval. Reset hooks and uhubctl power cycles
are bounded by the configured reset_timeout_secs value.

The profile executor command receives HIL_PROFILE_ID, HIL_CASE_ID,
HIL_SUITE, HIL_EXECUTION_LEVEL, HIL_RANDOM_SEED, and HIL_ATTEMPT.
It must return zero only after the protocol assertions and device checks have
completed; a non-zero exit is classified using the case's failure_class.
Only FAIL_LAB receives one retry. A protocol, assertion, or device failure
is never retried into a pass.

The first live acceptance run is the two-RNode path: discovery, link setup,
payload/resource transfer, forwarding, and reset/recovery. Keep the complete
matrix in the nightly and release reports even when a profile is BLOCKED while
its host is being provisioned.
