#![allow(clippy::result_large_err)]

use lxmf_sdk::{
    Client, LxmfSdk, SdkConfig, SendRequest, StartRequest, ZmqEndpointRole,
    ZmqPipelineBackendClient, ZmqPipelineBackendConfig, ZmqPipelineTokenAuth,
};
use serde_json::json;
use std::time::Duration;

fn main() -> Result<(), lxmf_sdk::SdkError> {
    let command_endpoint =
        std::env::var("LXMF_ZMQ_COMMAND").unwrap_or_else(|_| "tcp://127.0.0.1:9100".to_owned());
    let response_endpoint =
        std::env::var("LXMF_ZMQ_RESPONSE").unwrap_or_else(|_| "tcp://127.0.0.1:9101".to_owned());
    let source = std::env::var("LXMF_SOURCE").unwrap_or_else(|_| "example.zmq".to_owned());
    let destination =
        std::env::var("LXMF_DESTINATION").unwrap_or_else(|_| "example.peer".to_owned());

    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.command_role = endpoint_role_from_env("LXMF_ZMQ_COMMAND_ROLE", config.command_role);
    config.response_role = endpoint_role_from_env("LXMF_ZMQ_RESPONSE_ROLE", config.response_role);
    config.request_timeout = Duration::from_millis(
        std::env::var("LXMF_ZMQ_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5_000),
    );
    config.token_auth = token_auth_from_env();

    let backend = ZmqPipelineBackendClient::new(config)?;
    let client = Client::new(backend);
    let handle = client.start(StartRequest::new(SdkConfig::desktop_local_default()))?;
    println!("started runtime_id={}", handle.runtime_id);

    let message_id = client.send(
        SendRequest::new(
            source,
            destination,
            json!({
                "title": "ZeroMQ SDK Example",
                "content": "hello from lxmf-sdk over ZeroMQ"
            }),
        )
        .with_ttl_ms(30_000)
        .with_correlation_id("example-zmq-pipeline-send"),
    )?;
    println!("queued message_id={message_id}");

    let snapshot = client.snapshot()?;
    println!(
        "snapshot runtime_id={} state={:?} queued={} in_flight={}",
        snapshot.runtime_id, snapshot.state, snapshot.queued_messages, snapshot.in_flight_messages
    );

    Ok(())
}

fn endpoint_role_from_env(name: &str, default_role: ZmqEndpointRole) -> ZmqEndpointRole {
    match std::env::var(name).ok().as_deref() {
        Some("bind") | Some("BIND") => ZmqEndpointRole::Bind,
        Some("connect") | Some("CONNECT") => ZmqEndpointRole::Connect,
        _ => default_role,
    }
}

fn token_auth_from_env() -> Option<ZmqPipelineTokenAuth> {
    let shared_secret = std::env::var("LXMF_ZMQ_TOKEN_SECRET").ok()?;
    Some(ZmqPipelineTokenAuth {
        issuer: std::env::var("LXMF_ZMQ_TOKEN_ISSUER")
            .unwrap_or_else(|_| "lxmf-sdk-example".to_owned()),
        audience: std::env::var("LXMF_ZMQ_TOKEN_AUDIENCE")
            .unwrap_or_else(|_| "reticulumd-zmq".to_owned()),
        shared_secret,
        ttl_secs: std::env::var("LXMF_ZMQ_TOKEN_TTL_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(60),
    })
}
