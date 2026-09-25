# Issue #614 AX.25 KISS production serial interoperability

This software-only regression supplements the separate raw KISS/IFAC PTY
trace. It runs pinned Reticulum 1.5.4's `AX25KISSInterface` in a Python
process against Rust's production serial `KissInterface` with the configured
AX.25 UI payload adapter, using a two-PTY byte bridge. It does not use or claim
physical modem, radio, or on-air interoperability.

The Python endpoint is configured as `AX25KISSInterface` with `N0CALL-1` and
uses the repository's normal Channel endpoint. Rust wraps/unwraps the same
16-byte AX.25 UI header through `Ax25KissPayloadConfig` and the production
KISS serial worker. The regression requires an announce, active Link, Rust to
Python Channel request, Python reply, and Rust delivery proof. It also asserts
non-zero Rust interface receive/transmit byte counters, bounds the end-to-end
exchange to 60 seconds, and requires the Rust serial worker to stop within two
seconds. The endpoint and PTY bridge are child processes owned by guards that
kill and reap them on test exit.

Pinned reference: Reticulum commit
`99de23c040d507e3fefca19e87b182302902725d` (`1.5.4-dev`).

Focused command and result:

```text
RETICULUM_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/Reticulum-target-99de23c0 \
LXMF_PY_REPO=/tmp/lxmf-606-parity-refs.hv0vPX/LXMF \
LXMF_PYTHON_BIN=python3 cargo test -p reticulumd --test python_channel_interop \
  python_rust_ax25_kiss_serial_channel_roundtrip_over_pty -- \
  --ignored --exact --nocapture --test-threads=1
PASS (1 test)
```

Remaining gaps include physical carrier behavior, modem/firmware-specific KISS
commands and flow control, AX.25 path/address validation beyond the UI payload
wrapper used by Reticulum, and other serial backends/platforms. This test does
not complete the broad #614 native-interface acceptance matrix.
