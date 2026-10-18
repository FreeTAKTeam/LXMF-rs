//! Immutable Link packet meaning lives beside the existing receipt publisher.
use super::{ReceiptEvent, ReceiptPublisher};
use rns_rpc::broker::PropagationCustody;
use rns_transport::transport::LinkPacketReceipt;
use std::io;

pub(super) struct PacketCorrelation {
    event: ReceiptEvent,
    receipt: LinkPacketReceipt,
}

impl ReceiptPublisher {
    /// Register immutable attempt meaning before handoff. Returns whether this
    /// call inserted a new correlation; a duplicate cannot change its meaning.
    pub fn track_link_packet(
        &self,
        mut event: ReceiptEvent,
        receipt: LinkPacketReceipt,
    ) -> io::Result<bool> {
        if event.custody() != Some(PropagationCustody::Packet)
            && !(event.custody().is_none() && event.status == "delivered")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "packet proof requires delivery or typed packet custody",
            ));
        }
        let hash = receipt.packet_hash().to_bytes();
        event.packet_hash = Some(hex::encode(hash));
        event.link_id = Some(hex::encode(receipt.link_id().as_slice()));
        let mut packets = self.packets.lock().map_err(|e| io::Error::other(e.to_string()))?;
        if let Some(existing) = packets.get(&hash) {
            if existing.event == event {
                return Ok(false);
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "packet correlation meaning is immutable",
            ));
        }
        packets.insert(hash, PacketCorrelation { event, receipt });
        Ok(true)
    }

    /// Called after native proof validation, or by its retained delivery owner.
    /// Known correlations never fall through to ordinary delivered routing.
    /// Queue acceptance/commit deduplication stays with the existing publisher.
    pub fn publish_link_packet(&self, hash: [u8; 32]) -> io::Result<bool> {
        let correlation = self
            .packets
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?
            .get(&hash)
            .map(|tracked| (tracked.event.clone(), tracked.receipt.clone()));
        let Some((event, receipt)) = correlation else {
            return Ok(false);
        };
        if !receipt.is_proved() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packet proof has not been validated by Transport",
            ));
        }
        // Do not nest correlation and reservation locks, or retain either through
        // transport I/O. The native token still preserves proof after backpressure.
        self.try_send(event)?;
        Ok(true)
    }

    pub fn forget_link_packet(&self, hash: [u8; 32], message_id: &str) -> io::Result<()> {
        let mut packets = self.packets.lock().map_err(|e| io::Error::other(e.to_string()))?;
        if packets.get(&hash).is_some_and(|tracked| tracked.event.message_id == message_id) {
            packets.remove(&hash);
        }
        Ok(())
    }

    pub(super) fn prune_link_packets(&self, message_id: &str, custody_only: bool) {
        match self.packets.lock() {
            Ok(mut packets) => packets.retain(|_, tracked| {
                tracked.event.message_id != message_id
                    || (custody_only && tracked.event.custody().is_none())
            }),
            Err(error) => log::error!(
                "packet correlation retirement poisoned message_id={message_id}: {error}"
            ),
        }
    }
}
