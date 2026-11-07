# Explicit loopback endpoints and the #663 poll client

The full resource fixture exposed a callback failure before sustained polling:
`resource_soak_663_client` supplied numeric IPv4 command/response endpoints to
`ZmqPipelineBackendConfig::local_tcp`, but the SDK changed both `127.0.0.1`
hosts to `localhost`. On this Linux host, the callback PULL listener resolved
`localhost` to `::1`. The daemon's legacy reply connector normalizes advertised
`localhost` callbacks back to `127.0.0.1`, so replies targeted a different address
family. Sharing the same network namespace did not fix the mismatch.

The normalization originated in #199 alongside a regression explicitly named
`config_normalizes_ipv4_loopback_for_windows_tcp_bind`. The narrow correction
keeps that workaround on Windows and preserves supplied strings elsewhere for
`local_tcp`, `local` and `remote`. It changes no authentication requirements,
request deadlines, polling intervals or daemon callback policy. In particular,
the soak client still uses the SDK's default five-second request timeout.

The resource harness passes explicit numeric IPv4 endpoints, for example:

```sh
resource_soak_663_client --endpoint tcp://127.0.0.1:9100 \
  --response-endpoint tcp://127.0.0.1:9101 --duration-secs 60
```

The client example needs no field overrides. `resource_sdk_peer` also removes
its previous command/response overrides so paired SDK testing exercises the
public constructor. This repair guarantees preservation of the requested
numeric address family on non-Windows hosts; it does not change how explicit
`localhost` names resolve or claim to repair every possible hostname callback
configuration.

Platform configuration regressions cover IPv4, IPv6 and hostname preservation,
all three public constructors and unchanged timeout/authentication validation.
Windows retains a dedicated regression requiring its existing normalization.
A real daemon command-loop regression uses the public `local_tcp` constructor,
negotiates SDK v2 and receives three event-poll replies over separate callback
sockets. It contains no endpoint overrides and uses the default request timeout.
The non-Windows configuration regression was observed failing before the fix:
`tcp://localhost:9000` was returned for requested `tcp://127.0.0.1:9000`.
