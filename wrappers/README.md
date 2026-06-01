# SDK Wrapper Conformance

First-party wrappers must stay aligned with the SDK app v1 contract fixtures in
`docs/fixtures/sdk-app-v1`.

`wrappers/wrapper-conformance.json` is the reusable registry for wrapper parity
gates. Add a new wrapper entry there before adding a second binding, and include
the same scenario IDs used by the Rust app-mode conformance suite.

Current wrapper entries:

- `kotlin-mobile`: first-party mobile wrapper source and conformance anchors.

The release gate is:

```bash
cargo test -p test-support sdk_wrapper_parity_release_gate
```

PR CI and release bundle packaging both run that gate. Release packaging must
not proceed if wrapper manifests, docs, examples, or shared fixture references
drift.
