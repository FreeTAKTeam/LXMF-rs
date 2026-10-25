# Application custody classification

The unpublished propagation custody API uses `MessageKind::Application` for
opaque application data. R3AKT's `Mission` classification belongs to
`r3akt-mesh-delivery::CustodyKind` in the R3AKTClient shared-crate workspace.
Embedding applications translate that domain type at their LXMF boundary.

`Chat` and keyed `Telemetry` remain generic LXMF custody kinds. Existing stored
application records keep discriminator `1`, their retention and recovery state,
and their original signed wire. No database migration or data deletion is needed.
Generic propagation storage, validation, retention and `/get`/`/offer` handlers
remain in LXMF-rs; LXMF-rs does not depend on R3AKTClient.
