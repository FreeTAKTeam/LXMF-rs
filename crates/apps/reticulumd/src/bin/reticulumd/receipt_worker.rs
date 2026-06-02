use super::outbound_resources::OutboundResourceMap;
use super::receipt_events::handle_receipt_update;
use reticulum_daemon::receipt_bridge::ReceiptEvent;
use rns_rpc::RpcDaemon;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Receiver;

const RECEIPT_WORKER_DRAIN_LIMIT: usize = 256;

pub(super) fn spawn_receipt_worker(
    daemon: Arc<RpcDaemon>,
    mut receipt_rx: Receiver<ReceiptEvent>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: OutboundResourceMap,
) {
    let daemon_receipts = daemon;
    tokio::spawn(async move {
        while let Some(event) = receipt_rx.recv().await {
            handle_receipt_update(&daemon_receipts, event, &receipt_map, &outbound_resource_map);
            for _ in 0..RECEIPT_WORKER_DRAIN_LIMIT {
                match receipt_rx.try_recv() {
                    Ok(event) => {
                        handle_receipt_update(
                            &daemon_receipts,
                            event,
                            &receipt_map,
                            &outbound_resource_map,
                        );
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => return,
                }
            }
            tokio::task::yield_now().await;
        }
    });
}
