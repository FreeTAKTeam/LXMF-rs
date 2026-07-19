#![allow(clippy::result_large_err)]

use lxmf_sdk::{
    Client, LxmfSdk, SdkConfig, SendRequest, StartRequest, ZmqPipelineBackendClient,
    ZmqPipelineBackendConfig, ZmqPipelineTokenAuth,
};
use serde_json::json;
use std::time::Duration;

fn main() -> Result<(), lxmf_sdk::SdkError> {
    let endpoint =
        std::env::var("LXMF_ZMQ_ENDPOINT").unwrap_or_else(|_| "tcp://127.0.0.1:9100".to_owned());
    let source = std::env::var("LXMF_SOURCE").unwrap_or_else(|_| "example.zmq".to_owned());
    let destination =
        std::env::var("LXMF_DESTINATION").unwrap_or_else(|_| "example.peer".to_owned());

    let mut config = match token_auth_from_env() {
        Some(token_auth) => ZmqPipelineBackendConfig::remote(endpoint, token_auth),
        None => ZmqPipelineBackendConfig::local(endpoint),
    };
    config.request_timeout = Duration::from_millis(
        std::env::var("LXMF_ZMQ_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5_000),
    );
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
        .with_correlation_id("example-zmq-pipeline-send")
        .with_delivery_method("direct")
        .with_stamp_cost(8)
        .with_include_ticket(true)
        .with_try_propagation_on_fail(true),
    )?;
    println!("queued message_id={message_id}");

    let snapshot = client.snapshot()?;
    println!(
        "snapshot runtime_id={} state={:?} queued={} in_flight={}",
        snapshot.runtime_id, snapshot.state, snapshot.queued_messages, snapshot.in_flight_messages
    );

    Ok(())
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
