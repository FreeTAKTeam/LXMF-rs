//! Standard propagation control policy shared by daemon and embedded hosts.
use crate::protocol::{parse_transfer_limit_bytes, validated_binary_id_list};
use rns_core::destination::{DestinationName, SingleOutputDestination};
use rns_core::identity::Identity;

mod get;
mod offer;
pub use get::handle_get;
pub use offer::handle_offer;

pub enum HandlerResponse {
    Code(u8),
    Bool(bool),
    Rmpv(rmpv::Value),
}

pub trait GetBackend {
    type Error;
    fn delivery_identity_allowed(&self, identity: &Identity) -> bool;
    fn retain_synced_on_node(&self) -> bool;
    fn list_propagation_payloads_for_destination(
        &self,
        destination: &[u8; 16],
    ) -> Vec<(Vec<u8>, usize)>;
    fn has_peer_completed_propagation_mark(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, Self::Error>;
    fn record_propagation_offer_peer(&self, peer: &str) -> Result<(), Self::Error>;
    fn purge_propagation_payloads_for_destination(
        &self,
        destination: &[u8; 16],
        haves: &[Vec<u8>],
    ) -> usize;
    fn has_peer_propagation_mark(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, Self::Error>;
    fn record_existing_peer_received_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<bool, Self::Error>;
    fn preview_propagation_payloads_for_destination_with_ids(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<(String, Vec<u8>)>;
    fn transfer_limited_propagation_payload_ids_for_destination(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<String>;
    fn fetch_propagation_payloads_for_destination_with_ids(
        &self,
        destination: &[u8; 16],
        wanted: &[Vec<u8>],
        transfer_limit_bytes: Option<usize>,
    ) -> Vec<(String, Vec<u8>)>;
    fn record_peer_transferred_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), Self::Error>;
    fn record_peer_transfer_limited_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), Self::Error>;
}

pub trait OfferBackend {
    type Error;
    fn static_peer_allowed(&self, peer: &str) -> bool;
    fn peering_cost(&self) -> u32;
    fn mark_current_link_validated(&self) -> Result<(), Self::Error>;
    fn propagation_peer_is_throttled(&self, peer: &str) -> bool;
    fn propagation_peer_offer_is_throttled(&self, peer: &str) -> bool;
    fn throttle_propagation_peer_offer(&self, peer: &str);
    fn has_propagation_payload(&self, transient_id: &str) -> bool;
    fn record_peer_received_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> Result<(), Self::Error>;
    fn propagation_peer_admission_allowed(&self, peer: &str) -> bool;
}

pub fn named_destination_hash_for_identity(identity: &Identity, aspect: &str) -> [u8; 16] {
    let destination = SingleOutputDestination::new(*identity, DestinationName::new("lxmf", aspect));
    let mut result = [0; 16];
    result.copy_from_slice(destination.desc.address_hash.as_slice());
    result
}
fn delivery_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    named_destination_hash_for_identity(identity, "delivery")
}
fn propagation_destination_hash_for_identity(identity: &Identity) -> [u8; 16] {
    named_destination_hash_for_identity(identity, "propagation")
}
