//! Local resource qualification driver. Uses the public SDK for all traffic;
//! it never inserts messages/events into a daemon database or control API.
use lxmf_sdk::{
    Client, EventCursor, LxmfSdk, LxmfSdkIdentity, MessageId, SdkConfig, SendRequest, StartRequest,
    ZmqPipelineBackendClient, ZmqPipelineBackendConfig,
};
use serde::Deserialize;
use serde_json::json;
use std::io::{BufRead, Read, Write};
use std::time::Duration;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Operation {
    Send { token: String, destination: String, content: String },
    Status { token: String, message_id: String },
    Announce { token: String },
    Poll { token: String, cursor: Option<String> },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let endpoint = arguments.next().ok_or("expected local command endpoint")?;
    let response = arguments.next().ok_or("expected local response endpoint")?;
    if !endpoint.starts_with("tcp://127.0.0.1:")
        || !response.starts_with("tcp://127.0.0.1:")
        || arguments.next().is_some()
    {
        return Err("local test endpoints only".into());
    }
    let mut config = ZmqPipelineBackendConfig::local_tcp(endpoint.clone(), response.clone());
    config.command_endpoint = endpoint;
    config.response_endpoint = response;
    config.request_timeout = Duration::from_secs(5);
    let client = Client::new(ZmqPipelineBackendClient::new(config)?);
    let handle = client.start(
        StartRequest::new(SdkConfig::desktop_full_default()).with_requested_capabilities([
            "sdk.capability.identity_multi",
            "sdk.capability.identity_import_export",
            "sdk.capability.identity_discovery",
            "sdk.capability.cursor_replay",
            "sdk.capability.receipt_terminality",
        ]),
    )?;
    let identity = client.identity_create(lxmf_sdk::domain::IdentityCreateRequest {
        display_name: Some("Resource qualification peer".to_owned()),
        capabilities: vec!["lxmf.delivery".to_owned()],
        ..Default::default()
    })?;
    let source =
        identity.delivery_destination.ok_or("peer identity has no delivery destination")?;
    client.identity_announce_now()?;
    let mut output = std::io::stdout().lock();
    writeln!(
        output,
        "{}",
        json!({"ready": true, "runtime_id": handle.runtime_id, "destination": source})
    )?;
    output.flush()?;
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    let mut line = Vec::new();
    const MAX_INPUT_BYTES: u64 = 4 * 1024 * 1024;
    loop {
        line.clear();
        let count = input.by_ref().take(MAX_INPUT_BYTES + 1).read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if count as u64 > MAX_INPUT_BYTES {
            return Err("qualification input exceeds 4 MiB".into());
        }
        let result = match serde_json::from_slice::<Operation>(&line)? {
            Operation::Send { token, destination, content } => {
                let request = SendRequest::new(
                    source.clone(),
                    destination,
                    json!({"title": token, "content": content}),
                )
                .with_correlation_id(token.clone())
                .with_delivery_method("direct")
                .with_stamp_cost(0)
                .with_ttl_ms(60_000)
                .with_try_propagation_on_fail(false);
                let message_id = client.send(request)?;
                json!({"token": token, "message_id": message_id})
            }
            Operation::Status { token, message_id } => {
                json!({"token": token, "status": client.status(MessageId(message_id))?})
            }
            Operation::Announce { token } => {
                json!({"token": token, "ack": client.identity_announce_now()?})
            }
            Operation::Poll { token, cursor } => {
                json!({"token": token, "batch": client.poll_events(cursor.map(EventCursor), 64)?})
            }
        };
        writeln!(output, "{result}")?;
        output.flush()?;
    }
    Ok(())
}
