use super::*;
use hmac::{Hmac, Mac};
use rns_rpc::e2e_harness::{build_rpc_frame, parse_http_response_body, parse_rpc_frame};
use rns_rpc::RpcError;
use sha2::Sha256;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

impl RpcBackendClient {
    pub(super) fn call_rpc(
        &self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, SdkError> {
        let auth = self.session_auth.read().expect("session_auth rwlock poisoned").clone();
        let headers = self.headers_for_session_auth(&auth);
        self.call_rpc_with_headers(method, params, &headers)
    }

    pub(super) fn call_rpc_with_headers(
        &self,
        method: &str,
        params: Option<JsonValue>,
        headers: &[(String, String)],
    ) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let frame = build_rpc_frame(request_id, method, params).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let request = Self::build_http_post_with_headers("/rpc", &self.endpoint, &frame, headers);
        let mut stream = TcpStream::connect(&self.endpoint).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.write_all(&request).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.shutdown(Shutdown::Write).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let body = parse_http_response_body(&response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let rpc_response = parse_rpc_frame(&body).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        if let Some(error) = rpc_response.error {
            return Err(Self::map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    pub(super) fn build_http_post_with_headers(
        path: &str,
        host: &str,
        body: &[u8],
        headers: &[(String, String)],
    ) -> Vec<u8> {
        let mut request = Vec::new();
        request.extend_from_slice(format!("POST {path} HTTP/1.1\r\n").as_bytes());
        request.extend_from_slice(format!("Host: {host}\r\n").as_bytes());
        request.extend_from_slice(b"Content-Type: application/msgpack\r\n");
        for (name, value) in headers {
            request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        request.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
        request.extend_from_slice(b"\r\n");
        request.extend_from_slice(body);
        request
    }

    pub(super) fn map_rpc_error(error: RpcError) -> SdkError {
        let category = Self::map_category(error.code.as_str());
        SdkError::new(error.code, category, error.message)
    }

    pub(super) fn map_category(code: &str) -> ErrorCategory {
        if code.contains("_VALIDATION_") {
            return ErrorCategory::Validation;
        }
        if code.contains("_CAPABILITY_") {
            return ErrorCategory::Capability;
        }
        if code.contains("_CONFIG_") {
            return ErrorCategory::Config;
        }
        if code.contains("_POLICY_") {
            return ErrorCategory::Policy;
        }
        if code.contains("_TRANSPORT_") {
            return ErrorCategory::Transport;
        }
        if code.contains("_STORAGE_") {
            return ErrorCategory::Storage;
        }
        if code.contains("_CRYPTO_") {
            return ErrorCategory::Crypto;
        }
        if code.contains("_TIMEOUT_") {
            return ErrorCategory::Timeout;
        }
        if code.contains("_RUNTIME_") {
            return ErrorCategory::Runtime;
        }
        if code.contains("_SECURITY_") {
            return ErrorCategory::Security;
        }
        ErrorCategory::Internal
    }

    pub(super) fn profile_to_wire(profile: crate::types::Profile) -> &'static str {
        match profile {
            crate::types::Profile::DesktopFull => "desktop-full",
            crate::types::Profile::DesktopLocalRuntime => "desktop-local-runtime",
            crate::types::Profile::EmbeddedAlloc => "embedded-alloc",
        }
    }

    pub(super) fn bind_mode_to_wire(bind_mode: crate::types::BindMode) -> &'static str {
        match bind_mode {
            crate::types::BindMode::LocalOnly => "local_only",
            crate::types::BindMode::Remote => "remote",
        }
    }

    pub(super) fn auth_mode_to_wire(auth_mode: crate::types::AuthMode) -> &'static str {
        match auth_mode {
            crate::types::AuthMode::LocalTrusted => "local_trusted",
            crate::types::AuthMode::Token => "token",
            crate::types::AuthMode::Mtls => "mtls",
        }
    }

    pub(super) fn overflow_policy_to_wire(
        overflow_policy: crate::types::OverflowPolicy,
    ) -> &'static str {
        match overflow_policy {
            crate::types::OverflowPolicy::Reject => "reject",
            crate::types::OverflowPolicy::DropOldest => "drop_oldest",
            crate::types::OverflowPolicy::Block => "block",
        }
    }

    pub(super) fn session_auth_from_request(
        &self,
        req: &NegotiationRequest,
    ) -> Result<SessionAuth, SdkError> {
        match req.auth_mode {
            AuthMode::LocalTrusted => Ok(SessionAuth::LocalTrusted),
            AuthMode::Mtls => {
                let mtls_auth = req
                    .rpc_backend
                    .as_ref()
                    .and_then(|config| config.mtls_auth.as_ref())
                    .ok_or_else(|| {
                        SdkError::new(
                            code::SECURITY_AUTH_REQUIRED,
                            ErrorCategory::Security,
                            "mtls auth mode requires rpc_backend.mtls_auth",
                        )
                    })?;
                if mtls_auth.ca_bundle_path.trim().is_empty() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls auth mode requires non-empty rpc_backend.mtls_auth.ca_bundle_path",
                    ));
                }
                Ok(SessionAuth::Mtls { allowed_san: mtls_auth.allowed_san.clone() })
            }
            AuthMode::Token => {
                let token_auth = req
                    .rpc_backend
                    .as_ref()
                    .and_then(|config| config.token_auth.as_ref())
                    .ok_or_else(|| {
                        SdkError::new(
                            code::SECURITY_AUTH_REQUIRED,
                            ErrorCategory::Security,
                            "token auth mode requires rpc_backend.token_auth",
                        )
                    })?;
                if token_auth.shared_secret.trim().is_empty() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "token auth shared_secret must be configured",
                    ));
                }
                Ok(SessionAuth::Token {
                    issuer: token_auth.issuer.clone(),
                    audience: token_auth.audience.clone(),
                    shared_secret: token_auth.shared_secret.clone(),
                    ttl_secs: (token_auth.jti_cache_ttl_ms / 1000).max(1),
                })
            }
        }
    }

    pub(super) fn token_signature(secret: &str, payload: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("token shared secret must be non-empty");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub(super) fn headers_for_session_auth(&self, auth: &SessionAuth) -> Vec<(String, String)> {
        match auth {
            SessionAuth::LocalTrusted => Vec::new(),
            SessionAuth::Mtls { allowed_san } => {
                let mut headers = vec![("X-Client-Cert-Present".to_owned(), "1".to_owned())];
                if let Some(allowed_san) =
                    allowed_san.as_deref().map(str::trim).filter(|value| !value.is_empty())
                {
                    headers.push(("X-Client-SAN".to_owned(), allowed_san.to_owned()));
                }
                headers.push(("X-Client-Subject".to_owned(), "sdk-client-mtls".to_owned()));
                headers
            }
            SessionAuth::Token { issuer, audience, shared_secret, ttl_secs } => {
                let jti = format!("sdk-jti-{}", self.next_request_id());
                let iat = Self::now_seconds();
                let exp = iat.saturating_add(*ttl_secs);
                let payload = format!(
                    "iss={issuer};aud={audience};jti={jti};sub=sdk-client;iat={iat};exp={exp}"
                );
                let sig = Self::token_signature(shared_secret, payload.as_str());
                let token = format!("{payload};sig={sig}");
                vec![("Authorization".to_owned(), format!("Bearer {token}"))]
            }
        }
    }
}
