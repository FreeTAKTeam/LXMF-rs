//! Local resource qualification driver. Uses the public SDK for all traffic;
//! it never inserts messages/events into a daemon database or control API.
//! Append `--event-overflow-policy drop-oldest` for an explicit rolling observer;
//! omission preserves the SDK default. Batch boundaries do not measure log occupancy.
use lxmf_sdk::domain::{IdentityAnnounceRequest, IdentityAnnounceResult, IdentityRef};
use lxmf_sdk::{
    Client, EventBatch, EventCursor, LxmfSdk, LxmfSdkIdentity, MessageId, OverflowPolicy,
    SdkConfig, SendRequest, StartRequest, ZmqPipelineBackendClient, ZmqPipelineBackendConfig,
};
use serde::{Deserialize, Serialize};
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

fn parse_arguments(
    mut arguments: impl Iterator<Item = String>,
) -> Result<(String, String, Option<OverflowPolicy>), Box<dyn std::error::Error>> {
    let endpoint = arguments.next().ok_or("expected local command endpoint")?;
    let response = arguments.next().ok_or("expected local response endpoint")?;
    if !endpoint.starts_with("tcp://127.0.0.1:") || !response.starts_with("tcp://127.0.0.1:") {
        return Err("local test endpoints only".into());
    }
    let policy = match arguments.next().as_deref() {
        None => None,
        Some("--event-overflow-policy") => match arguments.next().as_deref() {
            Some("reject") => Some(OverflowPolicy::Reject),
            Some("drop-oldest") => Some(OverflowPolicy::DropOldest),
            _ => return Err("--event-overflow-policy requires reject or drop-oldest".into()),
        },
        Some(_) => return Err("unexpected qualification argument".into()),
    };
    if arguments.next().is_some() {
        return Err("unexpected qualification argument".into());
    }
    Ok((endpoint, response, policy))
}

#[derive(Default, Serialize)]
struct PollCounters {
    polls_total: u64,
    poll_errors_total: u64,
    returned_events_total: u64,
    stream_gap_events_total: u64,
    recovery_required_events_total: u64,
    max_reported_dropped_count: u64,
}

impl PollCounters {
    fn observe(&mut self, batch: &EventBatch) -> serde_json::Value {
        self.returned_events_total += batch.events.len() as u64;
        // dropped_count can be cumulative for cursor=None and zero for cursored
        // polls. Keep its maximum; summing batches would double-count losses.
        self.max_reported_dropped_count = self.max_reported_dropped_count.max(batch.dropped_count);
        let gaps = batch
            .events
            .iter()
            .filter(|event| matches!(event.event_type.as_str(), "StreamGap" | "stream_gap"))
            .map(|event| {
                json!({"seq_no": event.seq_no,
                    "expected_seq_no": event.payload.get("expected_seq_no"),
                    "observed_seq_no": event.payload.get("observed_seq_no"),
                    "dropped_count": event.payload.get("dropped_count"),
                    "recovery_required": event.payload.get("recovery_required")})
            })
            .collect::<Vec<_>>();
        self.stream_gap_events_total += gaps.len() as u64;
        self.recovery_required_events_total +=
            gaps.iter().filter(|gap| gap["recovery_required"].as_bool() == Some(true)).count()
                as u64;
        // These are returned-batch boundaries, not retained-log head/tail or
        // occupancy. The public EventBatch does not expose those measurements.
        json!({"batch_event_count": batch.events.len(),
            "batch_first_seq_no": batch.events.first().map(|event| event.seq_no),
            "batch_last_seq_no": batch.events.last().map(|event| event.seq_no),
            "next_cursor": batch.next_cursor,
            "dropped_count": batch.dropped_count,
            "snapshot_high_watermark_seq_no": batch.snapshot_high_watermark_seq_no,
            "stream_gaps": gaps,
            "counters": self})
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (endpoint, response, event_overflow_policy) = parse_arguments(std::env::args().skip(1))?;
    let mut config = ZmqPipelineBackendConfig::local_tcp(endpoint.clone(), response.clone());
    config.request_timeout = Duration::from_secs(5);
    let client = Client::new(ZmqPipelineBackendClient::new(config)?);
    let mut sdk_config = SdkConfig::desktop_full_default();
    if let Some(policy) = event_overflow_policy {
        sdk_config.overflow_policy = policy;
    }
    let effective_event_overflow_policy = sdk_config.overflow_policy.clone();
    let handle = client.start(StartRequest::new(sdk_config).with_requested_capabilities([
        "sdk.capability.identity_multi",
        "sdk.capability.identity_import_export",
        "sdk.capability.identity_discovery",
        "sdk.capability.cursor_replay",
        "sdk.capability.receipt_terminality",
    ]))?;
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
            "identity": identity_ref, "announce": announced,
            "effective_event_overflow_policy": effective_event_overflow_policy,
            "event_overflow_policy_basis": "accepted_sdk_start_request"})
    )?;
    output.flush()?;
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    let mut line = Vec::new();
    const MAX_INPUT_BYTES: u64 = 4 * 1024 * 1024;
    let mut poll_counters = PollCounters::default();
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
                poll_counters.polls_total += 1;
                match client.poll_events(cursor.clone().map(EventCursor), 64) {
                    Ok(batch) => {
                        let mut observation = poll_counters.observe(&batch);
                        observation["request_cursor"] = json!(cursor);
                        observation["effective_event_overflow_policy"] =
                            json!(effective_event_overflow_policy);
                        json!({"token": token, "batch": batch, "poll_observation": observation})
                    }
                    Err(error) => {
                        poll_counters.poll_errors_total += 1;
                        writeln!(
                            output,
                            "{}",
                            json!({"token": token,
                            "poll_observation": {
                                "effective_event_overflow_policy": effective_event_overflow_policy,
                                "request_cursor": cursor, "counters": poll_counters,
                                "error_code": error.machine_code,
                                "error_details": error.details,
                                "recovery_required": matches!(error.machine_code.as_str(),
                                    "SDK_RUNTIME_CURSOR_EXPIRED" | "SDK_RUNTIME_STREAM_DEGRADED"
                                        | "SDK_RUNTIME_INVALID_CURSOR")}})
                        )?;
                        output.flush()?;
                        return Err(error.into());
                    }
                }
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

    fn arguments(extra: &[&str]) -> impl Iterator<Item = String> {
        ["tcp://127.0.0.1:9000", "tcp://127.0.0.1:9001"]
            .into_iter()
            .chain(extra.iter().copied())
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn event_overflow_flag_is_explicit_and_rejects_invalid_arguments() {
        assert!(parse_arguments(arguments(&[])).expect("default arguments").2.is_none());
        assert_eq!(
            parse_arguments(arguments(&["--event-overflow-policy", "drop-oldest"]))
                .expect("explicit rolling observer")
                .2,
            Some(OverflowPolicy::DropOldest)
        );
        assert_eq!(
            parse_arguments(arguments(&["--event-overflow-policy", "reject"]))
                .expect("explicit reject observer")
                .2,
            Some(OverflowPolicy::Reject)
        );
        for extra in [
            vec!["--event-overflow-policy"],
            vec!["--event-overflow-policy", "block"],
            vec!["--unknown"],
            vec!["--event-overflow-policy", "drop-oldest", "unexpected"],
        ] {
            assert!(parse_arguments(arguments(&extra)).is_err());
        }
    }

    #[test]
    fn poll_audit_does_not_sum_cumulative_drop_counts_or_infer_window_occupancy() {
        let mut counters = PollCounters::default();
        let mut batch = EventBatch::empty(EventCursor("opaque-cursor".into()));
        batch.dropped_count = 7;
        let first = counters.observe(&batch);
        assert!(first["batch_first_seq_no"].is_null());
        assert!(first["batch_last_seq_no"].is_null());
        assert_eq!(first["next_cursor"], "opaque-cursor");
        counters.observe(&batch);
        batch.dropped_count = 0;
        counters.observe(&batch);
        assert_eq!(counters.max_reported_dropped_count, 7);
    }

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
