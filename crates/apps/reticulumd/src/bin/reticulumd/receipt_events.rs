use super::bridge_helpers::log_delivery_trace;
use super::inbound_worker::{
    prune_outbound_resource_mappings_for_message, OutboundResourceTracking,
};
use reticulum_daemon::receipt_bridge::{handle_receipt_event, ReceiptEvent};
use rns_rpc::RpcDaemon;
use rns_transport::receipt::prune_receipt_mappings_for_message;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(super) fn handle_receipt_update(
    daemon: &RpcDaemon,
    event: ReceiptEvent,
    receipt_map: &Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: &Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
) {
    let message_id = event.message_id.clone();
    let status = event.status.clone();
    let detail = format!("status={status}");
    log_delivery_trace(&message_id, "-", "receipt-update", &detail);
    let result = handle_receipt_event(daemon, event);
    if let Err(err) = result {
        let detail = format!("persist-failed err={err}");
        log_delivery_trace(&message_id, "-", "receipt-persist", &detail);
        return;
    }

    if is_terminal_receipt_status(&status) {
        prune_receipt_mappings_for_message(receipt_map, &message_id);
        prune_outbound_resource_mappings_for_message(outbound_resource_map, &message_id);
    }
    log_delivery_trace(&message_id, "-", "receipt-persist", "ok");
}

fn is_terminal_receipt_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "delivered" | "cancelled" | "expired" | "rejected"
    ) || status.trim().to_ascii_lowercase().starts_with("failed")
}
