use rns_transport::delivery::strip_destination_prefix as shared_strip_destination_prefix;
use rns_transport::transport::SendPacketTrace;

pub(crate) fn opportunistic_payload<'a>(payload: &'a [u8], destination: &[u8; 16]) -> &'a [u8] {
    shared_strip_destination_prefix(payload, destination)
}

pub(crate) fn delivery_trace_line(
    message_id: &str,
    destination: &str,
    stage: &str,
    detail: &str,
) -> String {
    format!("[delivery-trace] msg_id={message_id} dst={destination} stage={stage} {detail}")
}

pub(crate) fn log_delivery_trace(message_id: &str, destination: &str, stage: &str, detail: &str) {
    let line = delivery_trace_line(message_id, destination, stage, detail);
    log::trace!("{line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_trace_line_preserves_resource_marker() {
        let line = delivery_trace_line("msg-1", "dst-1", "direct", "resource_hash=abc123");

        assert!(line.contains("resource_hash=abc123"));
    }
}

pub(crate) fn payload_preview(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    hex::encode(&bytes[..end])
}

pub(crate) fn send_trace_detail(trace: SendPacketTrace) -> String {
    let direct_iface =
        trace.direct_iface.map(|iface| iface.to_string()).unwrap_or_else(|| "-".to_string());
    format!(
        "outcome={:?} direct_iface={} broadcast={} dispatch(matched={},sent={},failed={})",
        trace.outcome,
        direct_iface,
        trace.broadcast,
        trace.dispatch.matched_ifaces,
        trace.dispatch.sent_ifaces,
        trace.dispatch.failed_ifaces
    )
}
