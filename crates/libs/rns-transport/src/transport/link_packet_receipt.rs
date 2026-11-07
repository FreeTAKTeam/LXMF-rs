//! Optional per-packet proof context, independent of a mutable Link generation.
use super::*;
use std::sync::{OnceLock, Weak};
use std::time::Instant as ReceiptInstant;
use tokio::sync::watch;

struct LinkReceiptContext {
    hash: Hash,
    link_id: AddressHash,
    peer: Identity,
    timeout: Duration,
    sent_at: OnceLock<ReceiptInstant>,
}

/// An owned packet's native proof observation. Keep it alive while a late proof
/// remains meaningful. Transport keeps only a weak validation context, pruned by
/// packet-cache maintenance; ordinary cache expiry does not expire a live token.
/// Application status and persistence remain the caller's responsibility.
#[derive(Clone)]
pub struct LinkPacketReceipt {
    context: Arc<LinkReceiptContext>,
    proved: watch::Receiver<bool>,
}

impl LinkPacketReceipt {
    pub fn packet_hash(&self) -> Hash {
        self.context.hash
    }

    pub fn link_id(&self) -> AddressHash {
        self.context.link_id
    }

    pub fn is_proved(&self) -> bool {
        *self.proved.borrow()
    }

    pub fn sent_at(&self) -> Option<ReceiptInstant> {
        self.context.sent_at.get().copied()
    }

    pub fn timeout(&self) -> Duration {
        self.context.timeout
    }

    /// Expiry is strict, as in the reference PacketReceipt. A dropped handoff
    /// has no sent timestamp. Expiry does not invalidate the retained proof key;
    /// accepting a later proof is an explicit caller policy, not a resend command.
    pub fn expired(&self, now: ReceiptInstant) -> bool {
        !self.is_proved()
            && self
                .sent_at()
                .is_some_and(|sent| now.saturating_duration_since(sent) > self.timeout())
    }

    pub async fn wait_proved(&mut self) -> Result<(), std::io::Error> {
        loop {
            if self.is_proved() {
                return Ok(());
            }
            self.proved
                .changed()
                .await
                .map_err(|_| std::io::Error::other("Link packet proof owner unavailable"))?;
        }
    }

    fn mark_sent(&self) {
        self.context.sent_at.get_or_init(ReceiptInstant::now);
    }
}

struct LinkReceiptEntry {
    context: Weak<LinkReceiptContext>,
    proved: watch::Sender<bool>,
}

#[derive(Default)]
pub(super) struct LinkReceiptRegistry(HashMap<Hash, LinkReceiptEntry>);

impl LinkReceiptRegistry {
    pub(super) fn release(&mut self) {
        self.0.retain(|_, entry| entry.context.strong_count() > 0);
    }

    fn register(
        &mut self,
        hash: Hash,
        link_id: AddressHash,
        peer: Identity,
        timeout: Duration,
    ) -> Result<(LinkPacketReceipt, bool), RnsError> {
        self.release();
        if let Some(entry) = self.0.get(&hash) {
            if let Some(context) = entry.context.upgrade() {
                if context.link_id != link_id
                    || context.peer.public_key != peer.public_key
                    || context.peer.verifying_key != peer.verifying_key
                {
                    return Err(RnsError::IncorrectHash);
                }
                return Ok((
                    LinkPacketReceipt { context, proved: entry.proved.subscribe() },
                    false,
                ));
            }
        }
        let context =
            Arc::new(LinkReceiptContext { hash, link_id, peer, timeout, sent_at: OnceLock::new() });
        let (proved, receiver) = watch::channel(false);
        self.0.insert(hash, LinkReceiptEntry { context: Arc::downgrade(&context), proved });
        Ok((LinkPacketReceipt { context, proved: receiver }, true))
    }

    pub(super) fn validate(
        &mut self,
        packet: &Packet,
    ) -> Result<Option<[u8; HASH_SIZE]>, RnsError> {
        if packet.data.len() < HASH_SIZE {
            return Ok(None);
        }
        let mut hash = [0u8; HASH_SIZE];
        hash.copy_from_slice(&packet.data.as_slice()[..HASH_SIZE]);
        let hash = Hash::new(hash);
        let Some(entry) = self.0.get(&hash) else {
            return Ok(None);
        };
        let Some(context) = entry.context.upgrade() else {
            self.0.remove(&hash);
            return Ok(None);
        };
        // A known correlation must never fall through to a different live Link's
        // peer key, even if that peer signed this exact packet hash correctly.
        let validated = crate::destination::link::validate_link_packet_proof(
            &context.peer,
            &context.link_id,
            packet,
        )?;
        if validated != context.hash {
            return Err(RnsError::IncorrectHash);
        }
        entry.proved.send_replace(true);
        Ok(Some(validated.to_bytes()))
    }
}

impl Transport {
    /// Observe an active Link packet before interface I/O. The synchronous
    /// callback registers application correlation and may decline handoff with
    /// false; it must not re-enter Transport. None means no handoff was accepted.
    /// Capture the packet on this same Link generation before calling this API.
    ///
    /// The token records successful interface handoff before asynchronous Link
    /// bookkeeping. Keep prior tokens through retry/fallback if late proofs are
    /// allowed; dropping them retires native verification during cache cleanup.
    pub async fn send_link_packet_on_bound_iface_observed(
        &self,
        link: &Arc<Mutex<Link>>,
        packet: Packet,
        accept: impl FnOnce(LinkPacketReceipt) -> bool,
    ) -> Result<Option<SendPacketOutcome>, RnsError> {
        let handler = loop {
            let handler = self.handler.lock().await;
            let guard = match link.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    drop(handler);
                    drop(link.lock().await);
                    continue;
                }
            };
            if guard.status() != LinkStatus::Active
                || packet.destination != *guard.id()
                || packet.header.destination_type != DestinationType::Link
                || packet.header.packet_type != PacketType::Data
                || packet.context != PacketContext::None
            {
                return Err(RnsError::InvalidArgument);
            }
            let iface = guard.ingress_iface().ok_or(RnsError::ConnectionError)?;
            let identity = *guard.peer_identity();
            let link_id = *guard.id();
            let timeout = guard.packet_receipt_timeout();
            drop(guard);
            // Link state was captured before taking cache; no Link guard is held
            // while waiting for another transport/cache owner.
            let (receipt, newly_registered) = handler
                .packet_cache
                .lock()
                .await
                .link_receipts
                .register(packet.hash(), link_id, identity, timeout)?;
            if !accept(receipt.clone()) {
                if newly_registered {
                    handler.packet_cache.lock().await.link_receipts.0.remove(&packet.hash());
                }
                return Ok(None);
            }
            break (handler, iface, receipt);
        };
        let (handler, iface, receipt) = handler;
        let dispatch = handler
            .send_observed(TxMessage { tx_type: TxMessageType::Direct(iface), packet }, |trace| {
                if trace.sent_ifaces > 0 {
                    receipt.mark_sent();
                }
            })
            .await;
        Ok(Some(if dispatch.sent_ifaces > 0 {
            SendPacketOutcome::SentDirect
        } else {
            SendPacketOutcome::DroppedNoRoute
        }))
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;

    #[tokio::test]
    async fn native_producer_loss_is_unavailable_and_last_token_releases_weak_context() {
        let peer = crate::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let mut registry = LinkReceiptRegistry::default();
        let hash = Hash::new([7; HASH_SIZE]);
        let (receipt, _) = registry
            .register(hash, AddressHash::new([8; 16]), *peer.as_identity(), Duration::from_secs(1))
            .expect("registration");
        let mut observer = receipt.clone();
        let weak = Arc::downgrade(&receipt.context);
        drop(registry);
        assert!(observer.wait_proved().await.is_err(), "producer loss is not delivery");
        assert!(!observer.is_proved());
        drop(receipt);
        assert!(weak.upgrade().is_some(), "remaining owner retains native binding");
        drop(observer);
        assert!(weak.upgrade().is_none(), "final retirement releases the native context");

        let mut registry = LinkReceiptRegistry::default();
        let (receipt, _) = registry
            .register(hash, AddressHash::new([8; 16]), *peer.as_identity(), Duration::from_secs(1))
            .expect("new lifetime");
        drop(receipt);
        registry.release();
        assert!(registry.0.is_empty(), "maintenance removes retired weak entries");
    }
}
