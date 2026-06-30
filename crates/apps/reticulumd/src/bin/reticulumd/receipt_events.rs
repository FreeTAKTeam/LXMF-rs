use super::bridge_helpers::log_delivery_trace;
use super::outbound_resources::{
    prune_outbound_resource_mappings_for_message, OutboundResourceMap,
};
use reticulum_daemon::receipt_bridge::{handle_receipt_event, ReceiptEvent};
use rns_rpc::RpcDaemon;
use rns_transport::receipt::prune_receipt_mappings_for_message;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptDeliveryState {
    InProgress,
    Terminal,
}

impl ReceiptDeliveryState {
    fn from_status(status: &str) -> Self {
        let normalized = status.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), "delivered" | "cancelled" | "expired" | "rejected")
            || normalized.starts_with("failed")
        {
            Self::Terminal
        } else {
            Self::InProgress
        }
    }

    fn should_prune_correlations(self) -> bool {
        matches!(self, Self::Terminal)
    }
}

pub(super) fn handle_receipt_update(
    daemon: &RpcDaemon,
    event: ReceiptEvent,
    receipt_map: &Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: &OutboundResourceMap,
) {
    let message_id = event.message_id.clone();
    let status = event.status.clone();
    let detail = format!("status={status}");
    log_delivery_trace(&message_id, "-", "receipt-update", &detail);
    let delivery_state = ReceiptDeliveryState::from_status(&status);
    let result = handle_receipt_event(daemon, event);
    if let Err(err) = result {
        let detail = format!("persist-failed err={err}");
        log_delivery_trace(&message_id, "-", "receipt-persist", &detail);
        return;
    }

    if delivery_state.should_prune_correlations() {
        prune_receipt_mappings_for_message(receipt_map, &message_id);
        prune_outbound_resource_mappings_for_message(outbound_resource_map, &message_id);
    }
    log_delivery_trace(&message_id, "-", "receipt-persist", "ok");
}

#[cfg(test)]
mod tests {
    use super::ReceiptDeliveryState;

    #[test]
    fn terminal_receipt_states_prune_correlation_maps() {
        for status in ["delivered", "cancelled", "expired", "rejected", "failed: no route"] {
            assert_eq!(ReceiptDeliveryState::from_status(status), ReceiptDeliveryState::Terminal);
            assert!(ReceiptDeliveryState::from_status(status).should_prune_correlations());
        }
    }

    #[test]
    fn in_progress_receipt_states_keep_correlation_maps() {
        for status in [
            "sending",
            "sent: link resource",
            "sent: direct link",
            "queued",
            "queued: waiting for announce",
        ] {
            assert_eq!(ReceiptDeliveryState::from_status(status), ReceiptDeliveryState::InProgress);
            assert!(!ReceiptDeliveryState::from_status(status).should_prune_correlations());
        }
    }

    #[test]
    fn receipt_state_matching_is_case_and_space_tolerant() {
        assert_eq!(
            ReceiptDeliveryState::from_status("  Failed: timeout  "),
            ReceiptDeliveryState::Terminal
        );
        assert_eq!(
            ReceiptDeliveryState::from_status("  DELIVERED  "),
            ReceiptDeliveryState::Terminal
        );
    }
}
