use super::ZmqPipelineBackendClient;
use crate::capability::NegotiationRequest;
use crate::types::{AuthMode, BindMode};
use serde_json::{json, Value as JsonValue};

impl ZmqPipelineBackendClient {
    pub(super) fn negotiation_security_config(
        &self,
        req: &NegotiationRequest,
    ) -> (&'static str, &'static str, Option<JsonValue>) {
        if let Some(auth) = self.config.token_auth.as_ref() {
            return (
                "remote",
                "token",
                Some(json!({
                    "token_auth": {
                        "issuer": auth.issuer,
                        "audience": auth.audience,
                        "jti_cache_ttl_ms": auth.ttl_secs.saturating_mul(1000).max(1),
                        "clock_skew_ms": 5_000,
                        "shared_secret": auth.shared_secret,
                    }
                })),
            );
        }
        (
            bind_mode_to_wire(req.bind_mode.clone()),
            auth_mode_to_wire(req.auth_mode.clone()),
            req.rpc_backend.as_ref().map(|config| {
                json!({
                    "listen_addr": config.listen_addr,
                    "read_timeout_ms": config.read_timeout_ms,
                    "write_timeout_ms": config.write_timeout_ms,
                    "max_header_bytes": config.max_header_bytes,
                    "max_body_bytes": config.max_body_bytes,
                    "token_auth": config.token_auth.as_ref().map(|token| json!({
                        "issuer": token.issuer,
                        "audience": token.audience,
                        "jti_cache_ttl_ms": token.jti_cache_ttl_ms,
                        "clock_skew_ms": token.clock_skew_ms,
                        "shared_secret": token.shared_secret,
                    })),
                    "mtls_auth": config.mtls_auth.as_ref().map(|mtls| json!({
                        "ca_bundle_path": mtls.ca_bundle_path,
                        "require_client_cert": mtls.require_client_cert,
                        "allowed_san": mtls.allowed_san,
                        "client_cert_path": mtls.client_cert_path,
                        "client_key_path": mtls.client_key_path,
                    })),
                })
            }),
        )
    }
}

fn bind_mode_to_wire(bind_mode: BindMode) -> &'static str {
    match bind_mode {
        BindMode::LocalOnly => "local_only",
        BindMode::Remote => "remote",
    }
}

fn auth_mode_to_wire(auth_mode: AuthMode) -> &'static str {
    match auth_mode {
        AuthMode::LocalTrusted => "local_trusted",
        AuthMode::Token => "token",
        AuthMode::Mtls => "mtls",
    }
}
