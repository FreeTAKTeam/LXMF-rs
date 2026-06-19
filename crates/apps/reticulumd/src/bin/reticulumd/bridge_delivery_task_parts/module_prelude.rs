use super::identity_resolver;

use super::*;

use rand_core::OsRng;

use rns_transport::ratchets::encrypt_for_public_key;

pub(super) struct LinkModeStatuses {
    pub(super) packet: &'static str,
    pub(super) resource: &'static str,
    pub(super) resource_sent: &'static str,
}

pub(super) struct DeliveryTask {
    pub(super) daemon: Arc<RpcDaemon>,
    pub(super) transport: Arc<Transport>,
    pub(super) peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    pub(super) outbound_propagation_identities: Arc<Mutex<HashMap<String, Identity>>>,
    pub(super) receipt_map: Arc<Mutex<HashMap<String, String>>>,
    pub(super) outbound_resource_map: OutboundResourceMap,
    pub(super) outbound_propagation_link: Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    pub(super) receipt_tx: tokio::sync::mpsc::Sender<ReceiptEvent>,
    pub(super) message_id: String,
    pub(super) source_hash: [u8; 16],
    pub(super) destination: [u8; 16],
    pub(super) destination_hash: AddressHash,
    pub(super) destination_hex: String,
    pub(super) title: String,
    pub(super) content: String,
    pub(super) fields: Option<JsonValue>,
    pub(super) signer: PrivateIdentity,
    pub(super) stamp_cost: Option<u32>,
    pub(super) outbound_ticket: Option<String>,
    pub(super) include_ticket: Option<(i64, Vec<u8>)>,
    pub(super) peer_identity: Option<Identity>,
    pub(super) propagation_node_identity: Option<Identity>,
    pub(super) requested_method: RequestedDeliveryMethod,
    pub(super) try_propagation_on_fail: bool,
    pub(super) propagation_node_hex: Option<String>,
}

pub(super) struct PreparedDeliveryPayload {
    pub(super) lxmf_payload: Vec<u8>,
    pub(super) propagation: Option<PreparedPropagationPayload>,
}

pub(super) struct PreparedPropagationPayload {
    pub(super) propagation_node_hex: String,
    pub(super) propagation_hash: AddressHash,
    pub(super) target_cost: u32,
    pub(super) payload: propagation::PropagationPayload,
}

pub(super) struct PropagationPreparationContext {
    pub(super) destination_identity: Identity,
    pub(super) propagation_node_hex: String,
    pub(super) propagation_hash: AddressHash,
    pub(super) target_cost: u32,
}

pub(crate) fn emit_receipt_event(
    receipt_tx: &tokio::sync::mpsc::Sender<ReceiptEvent>,
    event: ReceiptEvent,
) {
    if let Err(err) = receipt_tx.try_send(event) {
        log::warn!("[daemon] dropped receipt event: {err}");
    }
}
