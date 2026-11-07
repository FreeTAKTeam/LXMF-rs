# Kotlin Mobile Easy-Mode Wrapper

This is the first-party Kotlin wrapper surface for the SDK app v1 easy-mode
contract. It is the reference mobile binding shape for issue #31.

The wrapper keeps the default mobile API small:

- `Config.mobile_default()` selects the mobile profile.
- `LxmfEasyClient.start(...)` starts the runtime in one call.
- `LxmfEasyClient.events(...)` exposes an async `Flow<LxmfEvent>`.
- `LxmfEasyClient.send(...)` returns a typed receipt or throws a typed
  `LxmfEasyError`.
- `LxmfEasyClient.stop(...)` and `close()` provide lifecycle cleanup.

The wrapper must match the shared SDK app v1 fixtures listed in
`conformance-manifest.json`. The Kotlin test file mirrors those scenario IDs
and checks the same semantic cases as the Rust conformance suite.

Current status: source, conformance tests, and the conditional
`kotlin-wrapper-conformance` job are present. The job in
`.github/workflows/ci-full.yml` runs Gradle tests for full-CI dispatches and
pull requests labeled `run-full-CI`.
