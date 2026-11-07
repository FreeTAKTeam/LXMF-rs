use super::*;

pub(super) async fn assert_row(h: &mut Harness, case: &Value, row: &Value, index: usize) {
    let name = case["name"].as_str().expect("name");
    let tag = format!("{name} row {index}");
    assert_eq!(h.path_known, row["path_known"], "{tag}: declared path input");
    assert_eq!(h.measurements.0, row["medium_timeout"].as_f64().expect("medium"));
    assert_eq!(h.measurements.1, row["lowest_bitrate"]);
    assert_eq!(h.measurements.2, row["first_hop"].as_f64().expect("first"));
    let observation = h.messages[0].control.observation.lock().expect("observation").clone();
    assert_eq!(
        observation.attempts as u64,
        row["attempts"].as_u64().expect("attempts"),
        "{tag}: attempts"
    );
    assert_eq!(
        observation.schedule.max_attempts() as u64,
        row["max_attempts"].as_u64().expect("limit"),
        "{tag}: limit"
    );
    assert_eq!(
        observation.schedule.awaiting_path(),
        row["awaiting_path"].as_bool().expect("awaiting"),
        "{tag}: path wait"
    );
    assert_eq!(
        observation.never_active_requested,
        row["path_request_retried"].as_bool().expect("path request retried"),
        "{tag}: closure path request"
    );
    match (observation.schedule.deadline(), row["deadline"].as_f64()) {
        (Some(actual), Some(expected)) => {
            let origin = observation.origin.expect("owner origin").duration_since(h.origin);
            assert_time(origin + actual, expected - 1000.0, &tag, "deadline");
        }
        (None, None) => {}
        (actual, expected) => panic!("{tag}: deadline presence {actual:?} vs {expected:?}"),
    }
    assert_eq!(
        h.origin.elapsed(),
        Duration::from_secs_f64(row["time"].as_f64().expect("snapshot time") - 1000.0),
        "{tag}: declared clock"
    );
    let rows = &case["rows"].as_array().expect("rows")[..=index];
    let cancelled = rows.iter().any(|r| r["event"]["cancel"] == true);
    assert_message(h, 0, row, cancelled, &tag);
    if let Some(messages) = row["messages"].as_array() {
        assert_eq!(h.messages.len(), messages.len(), "{tag}: shared admission count");
        for (item, expected) in messages.iter().enumerate() {
            assert_message(h, item, expected, cancelled && item == 0, &tag);
        }
    }
    let dispatches = h
        .messages
        .iter()
        .map(|m| m.control.dispatches.lock().expect("dispatches").len())
        .sum::<usize>();
    assert_eq!(
        dispatches as u64,
        row["transmission_count"].as_u64().expect("attempted transmissions"),
        "{tag}: logical dispatch attempts"
    );
    let resource_attempts = h
        .messages
        .iter()
        .map(|m| {
            m.control.dispatches.lock().expect("dispatches").iter().filter(|d| d.resource).count()
        })
        .sum::<usize>();
    assert_eq!(
        resource_attempts as u64,
        row["resource_count"].as_u64().expect("resources"),
        "{tag}: Resource dispatch count"
    );
    assert_eq!(
        h.packet_tokens().len() as u64,
        row["receipt_count"].as_u64().expect("receipts"),
        "{tag}: native packet receipts"
    );
    assert_eq!(
        h.wire.resource_handoffs, resource_attempts,
        "{tag}: actual native Resource advertisements"
    );
    let native_packets =
        if case["send_effect"] == "receipt" { dispatches - resource_attempts } else { 0 };
    assert_eq!(
        h.wire.packet_handoffs, native_packets,
        "{tag}: actual native packet handoffs (controlled producer faults excluded)"
    );
    for (item, message) in h.messages.iter().enumerate() {
        for attempt in message.control.dispatches.lock().expect("dispatches").iter() {
            assert_eq!(attempt.bytes, message.expected, "{tag}: immutable prepared bytes");
            assert_eq!(
                attempt.resource,
                case["representation"] == "resource",
                "{tag}: native representation"
            );
            assert!(
                h.wire.links.iter().any(|link| Arc::ptr_eq(&link.link, &attempt.link)),
                "{tag}: dispatch uses a recorded native Arc"
            );
        }
        let observed = message.control.observation.lock().expect("observation").clone();
        if let Some(link) = observed.link.as_ref().filter(|_| observed.creator) {
            let native = h
                .wire
                .links
                .iter_mut()
                .find(|native| Arc::ptr_eq(&native.link, link))
                .expect("creator native Link");
            if let Some(owner) = native.creator {
                assert_eq!(owner, item, "{tag}: one immutable creator");
            } else {
                native.creator = Some(item);
            }
        }
        if let Some(resource) =
            observed.resource.filter(|_| !message.control.done.load(AtomicOrdering::Acquire))
        {
            assert_eq!(
                h.resources.lock().expect("Resource owner").get(&resource.to_string()),
                Some(&message.id),
                "{tag}: native transfer stays correlated"
            );
        }
        let stored = h
            .daemon
            .durable_prepared_payload(&message.id)
            .expect("durable preparation")
            .flatten()
            .expect("committed preparation");
        let prepared =
            crate::bridge::delivery_scheduler::decode_prepared(stored).expect("stored bytes");
        assert_eq!(
            prepared.propagation.as_ref().map_or(&prepared.lxmf_payload, |p| &p.payload.bytes),
            &message.expected,
            "{tag}: durable prepared bytes unchanged"
        );
        let mut journal = h.connection.prepare("SELECT payload FROM broker_events WHERE event_type='receipt' AND json_extract(payload,'$.message_id')=?1").expect("receipt journal");
        let statuses = journal
            .query_map([&message.id], |row| row.get::<_, String>(0))
            .expect("journal rows")
            .map(|row| {
                serde_json::from_str::<Value>(&row.expect("journal payload")).expect("receipt JSON")
                    ["status"]
                    .as_str()
                    .expect("status")
                    .to_string()
            })
            .collect::<Vec<_>>();
        let terminal = statuses
            .iter()
            .filter(|status| reticulum_daemon::receipt_bridge::is_terminal(status))
            .count();
        let custody = statuses.iter().filter(|status| status.starts_with("sent:")).count();
        assert!(terminal <= 1 && custody <= 1, "{tag}: one durable winner per completion kind");
        if let Some(status) =
            h.daemon.message_receipt_status(&message.id).expect("committed status")
        {
            if reticulum_daemon::receipt_bridge::is_terminal(&status) {
                assert_eq!(terminal, 1, "{tag}: terminal result is durable exactly once");
            }
            if status.starts_with("sent:") {
                assert_eq!(custody, 1, "{tag}: custody result is durable exactly once");
            }
        }
        assert!(message.admitted_at >= h.origin);
    }
    assert_eq!(
        h.wire.received.len(),
        native_packets,
        "{tag}: peer reconstructed every native packet"
    );
    for bytes in h.wire.received.iter().chain(&h.wire.resource_received) {
        assert!(
            h.messages.iter().any(|m| m.expected == *bytes),
            "{tag}: peer received exact committed bytes"
        );
    }
    let expected_resource_completions =
        rows.iter().filter(|r| r["event"]["resource"] == "complete").count();
    assert_eq!(
        h.wire.resource_received.len(),
        expected_resource_completions,
        "{tag}: real Resource recipient completion"
    );
    let actions = rows
        .iter()
        .flat_map(|r| r["actions"].as_array().expect("actions"))
        .map(|a| a.as_str().expect("action"))
        .collect::<Vec<_>>();
    let requests = h
        .messages
        .iter()
        .flat_map(|m| m.control.requests.lock().expect("requests").clone())
        .collect::<Vec<_>>();
    assert_eq!(
        requests.len(),
        actions.iter().filter(|a| **a == "request_path").count(),
        "{tag}: actual owner request decisions"
    );
    match (requests.iter().max(), row["request_time"].as_f64()) {
        (Some(actual), Some(expected)) => {
            assert_time(actual.duration_since(h.origin), expected - 1000.0, &tag, "last request")
        }
        (None, None) => {}
        (actual, expected) => panic!("{tag}: request presence {actual:?} vs {expected:?}"),
    }
    let teardowns = h
        .messages
        .iter()
        .map(|m| m.control.teardowns.lock().expect("teardowns").len())
        .sum::<usize>();
    assert_eq!(
        teardowns,
        actions.iter().filter(|a| a.starts_with("teardown:")).count(),
        "{tag}: owner teardown decisions"
    );
    assert_eq!(
        h.wire.links.len(),
        row["links"].as_array().expect("native Links").len(),
        "{tag}: native Link count"
    );
    for (native, expected) in h.wire.links.iter().zip(row["links"].as_array().expect("Links")) {
        assert_eq!(native.label, expected["label"], "{tag}: native Link creation order");
        let guard = native.link.lock().await;
        let status = match guard.status() {
            rns_transport::destination::link::LinkStatus::Pending => "pending",
            rns_transport::destination::link::LinkStatus::Handshake => "handshake",
            rns_transport::destination::link::LinkStatus::Active => "active",
            rns_transport::destination::link::LinkStatus::Closed => "closed",
            rns_transport::destination::link::LinkStatus::Stale => "stale",
        };
        assert_eq!(status, expected["status"], "{tag}: native Link status");
        assert_eq!(
            guard.was_activated(),
            expected["activated"],
            "{tag}: native activation history"
        );
        assert_eq!(
            native.creator.map(|i| i as u64),
            expected["creator"].as_u64(),
            "{tag}: creation-bound signal owner"
        );
    }
    let selected = selected_link(h).await;
    assert_eq!(selected.as_deref(), row["selected_link"].as_str(), "{tag}: selected Link");
    // The reference keeps Propagated's last delivery destination across closure;
    // Direct clears it. Rust preserves each attempt's immutable native binding.
    let delivery = {
        let attempts = h.messages[0].control.dispatches.lock().expect("dispatches");
        attempts
            .last()
            .filter(|attempt| {
                h.propagated
                    || observation
                        .link
                        .as_ref()
                        .is_some_and(|link| Arc::ptr_eq(&attempt.link, link))
            })
            .map(|attempt| {
                h.wire
                    .links
                    .iter()
                    .find(|native| Arc::ptr_eq(&native.link, &attempt.link))
                    .expect("native dispatch context")
                    .label
                    .clone()
            })
    };
    assert_eq!(
        delivery.as_deref(),
        row["delivery_link"].as_str(),
        "{tag}: exact dispatched Link context"
    );
    assert_eq!(h.global.available_permits(), 1, "{tag}: waiting owner releases execution capacity");
    assert_eq!(h.stamp.available_permits(), 1, "{tag}: waiting owner releases stamp capacity");
}
fn assert_time(actual: Duration, expected: f64, tag: &str, field: &str) {
    let expected = Duration::from_secs_f64(expected);
    assert!(
        actual.abs_diff(expected) <= Duration::from_nanos(1),
        "{tag}: {field} actual={actual:?} expected={expected:?}"
    );
}
fn assert_message(h: &Harness, item: usize, expected: &Value, cancelled: bool, tag: &str) {
    let message = &h.messages[item];
    let observation = message.control.observation.lock().expect("observation").clone();
    let state = message_state(h, item, &observation);
    let expected_state =
        if cancelled { "cancelled" } else { expected["state"].as_str().expect("reference state") };
    assert_eq!(
        state, expected_state,
        "{tag} message {item}: state (committed cancellation is monotonic)"
    );
    assert_eq!(
        observation.attempts as u64,
        expected["attempts"].as_u64().expect("message attempts"),
        "{tag} message {item}: attempts"
    );
    assert_eq!(
        h.scheduler.owns_delivery(&message.id),
        expected["pending"].as_bool().expect("pending"),
        "{tag} message {item}: retained admission"
    );
    if let Some(destination) = expected["destination"].as_str() {
        let prepared = h
            .daemon
            .durable_prepared_payload(&message.id)
            .expect("stored")
            .flatten()
            .expect("prepared");
        let wire = crate::bridge::delivery_scheduler::decode_prepared(prepared).expect("decode");
        assert_eq!(
            hex::encode(&wire.lxmf_payload[..16]),
            destination,
            "{tag} message {item}: final recipient preserved"
        );
    }
}
fn message_state(h: &Harness, index: usize, observation: &Observation) -> String {
    let status = h.daemon.message_receipt_status(&h.messages[index].id).expect("durable status");
    if let Some(status) = status {
        if status.starts_with("sent:") {
            return "sent".into();
        }
        if status.starts_with("failed") {
            return "failed".into();
        }
        if ["delivered", "cancelled", "rejected", "expired"].contains(&status.as_str()) {
            return status;
        }
    }
    if observation.packets.iter().any(|receipt| receipt.is_proved()) {
        return if h.propagated { "sent" } else { "delivered" }.into();
    }
    if observation.sending { "sending" } else { "outbound" }.into()
}
async fn selected_link(h: &Harness) -> Option<String> {
    let pending =
        h.messages.iter().filter(|m| h.scheduler.owns_delivery(&m.id)).collect::<Vec<_>>();
    let link = if !pending.is_empty() {
        pending.iter().find_map(|m| m.control.observation.lock().expect("selection").link.clone())
    } else if h.propagated {
        h.propagation_link.lock().await.as_ref().map(|cache| cache.link.clone())
    } else {
        h.wire.sender.delivery_link(&h.wire.descriptor.address_hash).await
    };
    link.and_then(|link| {
        h.wire.links.iter().find(|l| Arc::ptr_eq(&l.link, &link)).map(|l| l.label.clone())
    })
}
