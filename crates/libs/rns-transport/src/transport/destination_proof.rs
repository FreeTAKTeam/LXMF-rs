use super::*;
use crate::packet::Header;

/// An inbound event with an optional application-controlled opportunistic proof.
/// The legacy `ReceivedData` stream and its public struct remain unchanged.
#[derive(Clone)]
pub struct ReceivedDataWithProof {
    pub received: ReceivedData,
    pub proof: Option<DestinationProof>,
}

/// Opaque correlation for one decrypted packet and its ingress interface.
/// Dropping this handle sends nothing. It owns no payload, task or cache entry;
/// the bounded event stream bounds outstanding receive-side correlation.
#[derive(Clone)]
pub struct DestinationProof {
    destination: AddressHash,
    packet_hash: Hash,
    interface: AddressHash,
    pub(super) received_at: Instant,
}

impl DestinationProof {
    pub(super) fn new(packet: &Packet, interface: AddressHash) -> Self {
        Self {
            destination: packet.destination,
            packet_hash: packet.hash(),
            interface,
            received_at: Instant::now(),
        }
    }
}

impl Transport {
    /// Subscribe before admitting traffic. Only destinations explicitly registered
    /// for deferred proofs provide a proof handle; Link events have none.
    pub fn received_data_events_with_proofs(&self) -> broadcast::Receiver<ReceivedDataWithProof> {
        self.received_data_with_proof_tx.subscribe()
    }

    /// Register a destination whose opportunistic proofs require explicit
    /// application completion, atomically with respect to inbound processing.
    pub async fn add_destination_with_deferred_proofs(
        &self,
        identity: PrivateIdentity,
        name: DestinationName,
    ) -> Arc<Mutex<SingleInputDestination>> {
        let mut destination = SingleInputDestination::new(identity, name);
        destination.set_deferred_proofs(true);
        self.register_destination(destination).await
    }

    /// Prove a validated, admitted packet. The caller owns application validation
    /// and durability; transport signs its original encrypted wire hash and sends
    /// on its ingress interface for Reticulum reverse routing. Returns whether
    /// dispatch accepted the proof, not whether the remote sender received it.
    pub async fn prove_destination_packet(&self, proof: DestinationProof) -> bool {
        if proof.received_at.elapsed() > INTERVAL_KEEP_PACKET_CACHED {
            return false;
        }
        let handler = self.handler.lock().await;
        let Some(destination) = handler.single_in_destinations.get(&proof.destination) else {
            return false;
        };
        let signature = {
            let destination = destination.lock().await;
            if !destination.deferred_proofs {
                return false;
            }
            destination.identity.sign(&proof.packet_hash.to_bytes())
        };
        let mut data = Vec::with_capacity(HASH_SIZE + ed25519_dalek::SIGNATURE_LENGTH);
        data.extend_from_slice(proof.packet_hash.as_slice());
        data.extend_from_slice(&signature.to_bytes());
        let packet = Packet {
            header: Header { packet_type: PacketType::Proof, ..Default::default() },
            destination: AddressHash::new_from_hash(&proof.packet_hash),
            data: PacketDataBuffer::new_from_slice(&data),
            ..Packet::default()
        };
        let dispatch = handler
            .send(TxMessage { tx_type: TxMessageType::Direct(proof.interface), packet })
            .await;
        let dispatched = dispatch.sent_ifaces > 0 || dispatch.queued_ifaces > 0;
        log::debug!(
            "[tp-diag] deferred_destination_proof dst={} packet_hash={} iface={} dispatched={} sent={} queued={} failed={}",
            proof.destination, proof.packet_hash, proof.interface, dispatched,
            dispatch.sent_ifaces, dispatch.queued_ifaces, dispatch.failed_ifaces
        );
        dispatched
    }
}

impl TransportHandler {
    /// Only opted-in local opportunistic packets may reach application duplicate
    /// admission again. Routing guards still apply before bypassing membership.
    pub(super) async fn allows_deferred_destination_retry(&self, packet: &Packet) -> bool {
        if packet.header.packet_type != PacketType::Data
            || packet.header.destination_type != DestinationType::Single
            || packet.context != PacketContext::None
            || packet.transport.is_some_and(|id| id != *self.config.identity.address_hash())
        {
            return false;
        }
        match self.single_in_destinations.get(&packet.destination) {
            Some(destination) => destination.lock().await.deferred_proofs,
            None => false,
        }
    }
}
