use rns_rpc::ProbeReceiptRegistry;
use rns_rpc::RpcDaemon;
use rns_transport::receipt::{
    lookup_receipt_message_id, record_receipt_status,
    track_receipt_mapping as shared_track_receipt_mapping,
};
use rns_transport::transport::{DeliveryReceipt, ReceiptHandler};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
mod custody;
pub use custody::{is_terminal, ReceiptPublisher};

#[derive(Debug, Clone)]
pub struct ReceiptEvent {
    pub message_id: String,
    pub status: String,
    pub packet_hash: Option<String>,
    pub resource_hash: Option<String>,
    pub peer: Option<String>,
    pub method: Option<String>,
    pub delivery_kind: Option<String>,
    pub bytes: Option<usize>,
    pub link_id: Option<String>,
    pub stage: Option<String>,
}

impl ReceiptEvent {
    pub fn new(message_id: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            message_id: message_id.into(),
            status: status.into(),
            packet_hash: None,
            resource_hash: None,
            peer: None,
            method: None,
            delivery_kind: None,
            bytes: None,
            link_id: None,
            stage: None,
        }
    }

    pub fn with_packet_hash(mut self, packet_hash: impl Into<String>) -> Self {
        self.packet_hash = Some(packet_hash.into());
        self
    }

    pub fn with_resource_hash(mut self, resource_hash: impl Into<String>) -> Self {
        self.resource_hash = Some(resource_hash.into());
        self
    }

    pub fn with_peer(mut self, peer: impl Into<String>) -> Self {
        self.peer = Some(peer.into());
        self
    }

    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    pub fn with_delivery_kind(mut self, delivery_kind: impl Into<String>) -> Self {
        self.delivery_kind = Some(delivery_kind.into());
        self
    }

    pub fn with_bytes(mut self, bytes: usize) -> Self {
        self.bytes = Some(bytes);
        self
    }

    pub fn with_link_id(mut self, link_id: impl Into<String>) -> Self {
        self.link_id = Some(link_id.into());
        self
    }

    pub fn with_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    fn rpc_params(&self) -> serde_json::Value {
        let mut params = serde_json::json!({
            "message_id": self.message_id,
            "status": self.status,
        });
        if let Some(map) = params.as_object_mut() {
            if let Some(packet_hash) = &self.packet_hash {
                map.insert("packet_hash".to_string(), serde_json::json!(packet_hash));
            }
            if let Some(resource_hash) = &self.resource_hash {
                map.insert("resource_hash".to_string(), serde_json::json!(resource_hash));
            }
            if let Some(peer) = &self.peer {
                map.insert("peer".to_string(), serde_json::json!(peer));
            }
            if let Some(method) = &self.method {
                map.insert("method".to_string(), serde_json::json!(method));
            }
            if let Some(delivery_kind) = &self.delivery_kind {
                map.insert("delivery_kind".to_string(), serde_json::json!(delivery_kind));
            }
            if let Some(bytes) = self.bytes {
                map.insert("bytes".to_string(), serde_json::json!(bytes));
            }
            if let Some(link_id) = &self.link_id {
                map.insert("link_id".to_string(), serde_json::json!(link_id));
            }
            if let Some(stage) = &self.stage {
                map.insert("stage".to_string(), serde_json::json!(stage));
            }
        }
        params
    }
}

#[derive(Clone)]
pub struct ReceiptBridge {
    map: Arc<Mutex<HashMap<String, String>>>,
    tx: ReceiptPublisher,
    probe_receipts: Arc<ProbeReceiptRegistry>,
}

impl ReceiptBridge {
    pub fn new(map: Arc<Mutex<HashMap<String, String>>>, tx: impl Into<ReceiptPublisher>) -> Self {
        Self::with_probe_registry(map, tx, Arc::new(ProbeReceiptRegistry::default()))
    }

    pub fn with_probe_registry(
        map: Arc<Mutex<HashMap<String, String>>>,
        tx: impl Into<ReceiptPublisher>,
        probe_receipts: Arc<ProbeReceiptRegistry>,
    ) -> Self {
        Self { map, tx: tx.into(), probe_receipts }
    }
}

impl ReceiptHandler for ReceiptBridge {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        self.probe_receipts.notify(receipt.message_id);
        let message_id = match lookup_receipt_message_id(&self.map, receipt) {
            Ok(id) => id,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                log::warn!("[daemon] receipt map error: {err}");
                return;
            }
        };
        let event = ReceiptEvent::new(message_id, "delivered")
            .with_packet_hash(hex::encode(receipt.message_id))
            .with_delivery_kind("transport-receipt")
            .with_stage("transport_receipt");
        if let Err(err) = self.tx.try_send(event) {
            log::warn!("[daemon] dropped delivery receipt event: {err}");
        }
    }
}

pub fn handle_receipt_event(daemon: &RpcDaemon, event: ReceiptEvent) -> Result<(), std::io::Error> {
    record_receipt_status(
        &|_message_id: &str, _status: &str| {
            daemon.record_network_receipt(
                &event.message_id,
                &event.status,
                event
                    .rpc_params()
                    .as_object()
                    .cloned()
                    .ok_or_else(|| std::io::Error::other("receipt metadata is not an object"))?,
            )?;
            Ok(())
        },
        &event.message_id,
        &event.status,
    )?;
    if event.status.eq_ignore_ascii_case("delivered") {
        daemon.record_message_delivery_receipt(event.message_id.as_str())?;
    }
    Ok(())
}

pub fn track_receipt_mapping(
    map: &Arc<Mutex<HashMap<String, String>>>,
    packet_hash: &str,
    message_id: &str,
) {
    shared_track_receipt_mapping(map, packet_hash, message_id);
}

#[cfg(test)]
mod durable_tests {
    use super::*;
    use rns_rpc::broker::{AdmitRequest, OperationReceipt};
    use rns_rpc::{BrokerCommand, MessagesStore};
    #[test]
    fn network_receipt_commits_broker_status_and_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.db");
        let store = MessagesStore::open(&path).unwrap();
        store.enable_durable_broker(32 * 1024 * 1024).unwrap();
        let receipt: OperationReceipt = serde_json::from_value(
            store
                .broker_command(BrokerCommand::Admit {
                    owner: "authenticated-client".into(),
                    source: "rch".into(),
                    request: AdmitRequest {
                        identity: "rch".into(),
                        operation_id: "operation".into(),
                        destination: "remote".into(),
                        title: String::new(),
                        content: "Test1234".into(),
                        fields: None,
                        options: Default::default(),
                    },
                })
                .unwrap(),
        )
        .unwrap();
        let daemon = RpcDaemon::with_store(store, "receipt-node".into());
        let rejected = daemon
            .handle_rpc(rns_rpc::rpc::RpcRequest {
                id: 0,
                method: "record_receipt".into(),
                params: Some(
                    serde_json::json!({"message_id":receipt.message_id,"status":"delivered"}),
                ),
            })
            .unwrap();
        assert!(rejected.error.is_some());
        handle_receipt_event(&daemon, ReceiptEvent::new(receipt.message_id.clone(), "delivered"))
            .unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT receipt_status FROM messages WHERE id=?1",
                [&receipt.message_id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "delivered"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}
