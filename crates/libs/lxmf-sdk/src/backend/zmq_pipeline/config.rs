use crate::error::{code, ErrorCategory, SdkError};
use rns_rpc::rpc::zmq;
use std::net::IpAddr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZmqEndpointRole {
    Bind,
    Connect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZmqPipelineTokenAuth {
    pub issuer: String,
    pub audience: String,
    pub shared_secret: String,
    pub ttl_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZmqPipelineBackendConfig {
    pub command_endpoint: String,
    pub command_role: ZmqEndpointRole,
    pub response_endpoint: String,
    pub response_role: ZmqEndpointRole,
    pub request_timeout: Duration,
    pub max_envelope_bytes: usize,
    pub token_auth: Option<ZmqPipelineTokenAuth>,
}

impl ZmqPipelineBackendConfig {
    pub fn local_tcp(
        command_endpoint: impl Into<String>,
        response_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            command_endpoint: normalize_loopback_endpoint(command_endpoint.into()),
            command_role: ZmqEndpointRole::Connect,
            response_endpoint: normalize_loopback_endpoint(response_endpoint.into()),
            response_role: ZmqEndpointRole::Bind,
            request_timeout: Duration::from_secs(5),
            max_envelope_bytes: zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES,
            token_auth: None,
        }
    }

    pub fn validate(&self) -> Result<(), SdkError> {
        validate_endpoint_security(&self.command_endpoint, self.token_auth.is_some())?;
        validate_endpoint_security(&self.response_endpoint, self.token_auth.is_some())?;
        if self.max_envelope_bytes > zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "zmq max_envelope_bytes exceeds protocol limit",
            ));
        }
        Ok(())
    }
}

fn normalize_loopback_endpoint(endpoint: String) -> String {
    endpoint
        .strip_prefix("tcp://127.0.0.1:")
        .map(|port| format!("tcp://localhost:{port}"))
        .unwrap_or(endpoint)
}

fn validate_endpoint_security(endpoint: &str, has_auth: bool) -> Result<(), SdkError> {
    if is_local_endpoint(endpoint) || has_auth {
        return Ok(());
    }
    Err(SdkError::new(
        code::SECURITY_AUTH_REQUIRED,
        ErrorCategory::Security,
        "remote zmq endpoints require explicit token authentication",
    ))
}

fn is_local_endpoint(endpoint: &str) -> bool {
    if endpoint.starts_with("inproc://") {
        return true;
    }
    let Some(authority) = endpoint.strip_prefix("tcp://") else {
        return false;
    };
    let host = authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
        .trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}
