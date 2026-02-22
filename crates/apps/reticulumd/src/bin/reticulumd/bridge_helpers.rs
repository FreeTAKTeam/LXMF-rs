use rns_transport::delivery::strip_destination_prefix as shared_strip_destination_prefix;
use rns_transport::transport::SendPacketTrace;
use std::sync::OnceLock;

pub(crate) fn opportunistic_payload<'a>(payload: &'a [u8], destination: &[u8; 16]) -> &'a [u8] {
    shared_strip_destination_prefix(payload, destination)
}

pub(crate) fn log_delivery_trace(message_id: &str, destination: &str, stage: &str, detail: &str) {
    eprintln!(
        "[delivery-trace] msg_id={} dst={} stage={} {}",
        message_id, destination, stage, detail
    );
}

pub(crate) fn diagnostics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RETICULUMD_DIAGNOSTICS")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on" | "debug"
                )
            })
            .unwrap_or(false)
    })
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
