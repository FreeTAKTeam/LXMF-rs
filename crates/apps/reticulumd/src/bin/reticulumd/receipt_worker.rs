use super::bridge_helpers::log_delivery_trace;
use reticulum_daemon::receipt_bridge::{handle_receipt_event, ReceiptEvent};
use rns_rpc::RpcDaemon;
use std::rc::Rc;
use tokio::sync::mpsc::UnboundedReceiver;

pub(super) fn spawn_receipt_worker(
    daemon: Rc<RpcDaemon>,
    mut receipt_rx: UnboundedReceiver<ReceiptEvent>,
) {
    let daemon_receipts = daemon;
    tokio::task::spawn_local(async move {
        while let Some(event) = receipt_rx.recv().await {
            let message_id = event.message_id.clone();
            let status = event.status.clone();
            let detail = format!("status={status}");
            log_delivery_trace(&message_id, "-", "receipt-update", &detail);
            let result = handle_receipt_event(&daemon_receipts, event);
            if let Err(err) = result {
                let detail = format!("persist-failed err={err}");
                log_delivery_trace(&message_id, "-", "receipt-persist", &detail);
            } else {
                log_delivery_trace(&message_id, "-", "receipt-persist", "ok");
            }
        }
    });
}
