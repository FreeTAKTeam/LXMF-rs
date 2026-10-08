//! Local resource qualification driver. Uses the public SDK for all traffic;
//! it never inserts messages/events into a daemon database or control API.
use lxmf_sdk::domain::{IdentityAnnounceRequest, IdentityAnnounceResult, IdentityRef};
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

fn announce_created_identity(
    client: &impl LxmfSdkIdentity,
    identity: &IdentityRef,
    destination: &str,
) -> Result<IdentityAnnounceResult, Box<dyn std::error::Error>> {
    let result = client.identity_announce(IdentityAnnounceRequest {
        identity: Some(identity.clone()),
        ..Default::default()
    })?;
    if !result.accepted
        || result.identity.as_ref() != Some(identity)
        || result.delivery_destination.as_deref() != Some(destination)
    {
        return Err("SDK announce did not bind the created peer identity and destination".into());
    }
    Ok(result)
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
    let identity_ref = identity.identity;
    if !client.identity_activate(identity_ref.clone())?.accepted {
        return Err("SDK rejected activation of the created peer identity".into());
    }
    let announced = announce_created_identity(&client, &identity_ref, &source)?;
    let mut output = std::io::stdout().lock();
    writeln!(
        output,
        "{}",
        json!({"ready": true, "runtime_id": handle.runtime_id, "destination": source,
            "identity": identity_ref, "announce": announced})
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
                json!({"token": token, "ack": announce_created_identity(&client, &identity_ref, &source)?})
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Announcer {
        accepted: bool,
        destination: &'static str,
        identity: Option<&'static str>,
    }

    impl LxmfSdkIdentity for Announcer {
        fn identity_announce(
            &self,
            request: IdentityAnnounceRequest,
        ) -> Result<IdentityAnnounceResult, lxmf_sdk::SdkError> {
            assert_eq!(request.identity, Some(IdentityRef("created-peer".into())));
            Ok(serde_json::from_value(json!({
                "accepted": self.accepted, "identity": self.identity,
                "delivery_destination": self.destination,
            }))
            .expect("typed test announce"))
        }
    }

    #[test]
    fn discovery_uses_created_identity_and_rejects_wrong_destination_or_rejection() {
        let identity = IdentityRef("created-peer".into());
        assert!(announce_created_identity(
            &Announcer {
                accepted: true,
                destination: "peer-destination",
                identity: Some("created-peer")
            },
            &identity,
            "peer-destination"
        )
        .is_ok());
        for response in [
            Announcer {
                accepted: true,
                destination: "daemon-default",
                identity: Some("created-peer"),
            },
            Announcer {
                accepted: false,
                destination: "peer-destination",
                identity: Some("created-peer"),
            },
            Announcer {
                accepted: true,
                destination: "peer-destination",
                identity: Some("daemon-default"),
            },
            Announcer { accepted: true, destination: "peer-destination", identity: None },
        ] {
            assert!(announce_created_identity(&response, &identity, "peer-destination").is_err());
        }
    }
}
