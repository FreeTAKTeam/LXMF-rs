use super::*;
use rns_rpc::{OutboundBridge, OutboundDeliveryOptions, PaperDecodeOutcome, PaperEncodeEnvelope};

impl OutboundBridge for TransportBridge {
    fn validate_delivery(
        &self,
        record: &rns_rpc::MessageRecord,
        options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let _destination = parse_destination_hash_required(&record.destination)?;
        let daemon = self
            .daemon
            .lock()
            .expect("transport bridge daemon mutex poisoned")
            .clone()
            .ok_or_else(|| std::io::Error::other("daemon bridge unavailable"))?;
        let requested_method = RequestedDeliveryMethod::parse(options.method.as_deref())?;
        let propagation_node_hex = daemon.outbound_propagation_node();
        validate_delivery_request(requested_method, propagation_node_hex.as_deref())
    }

    fn encode_paper(
        &self,
        record: &rns_rpc::MessageRecord,
    ) -> Result<Option<PaperEncodeEnvelope>, std::io::Error> {
        paper::encode_paper(self, record)
    }

    fn decode_paper_uri(&self, uri: &str) -> Result<Option<PaperDecodeOutcome>, std::io::Error> {
        paper::decode_paper_uri(self, uri)
    }

    fn deliver(
        &self,
        record: &rns_rpc::MessageRecord,
        options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let destination = parse_destination_hash_required(&record.destination)?;
        let daemon = self
            .daemon
            .lock()
            .expect("transport bridge daemon mutex poisoned")
            .clone()
            .ok_or_else(|| std::io::Error::other("daemon bridge unavailable"))?;
        let local_delivery_identity = (destination == self.delivery_source_hash).then(|| {
            let signer_identity = self.signer.as_identity();
            Identity::new_from_slices(
                signer_identity.public_key_bytes(),
                signer_identity.verifying_key_bytes(),
            )
        });
        let peer_identity = outbound_peer_identity(
            daemon.as_ref(),
            &self.peer_crypto,
            record.destination.as_str(),
            destination,
            local_delivery_identity,
        );

        let include_ticket = if options.include_ticket {
            daemon
                .generate_ticket(record.destination.as_str(), None)
                .map_err(std::io::Error::other)?
        } else {
            None
        };
        let include_ticket_bytes = include_ticket
            .as_ref()
            .map(|ticket| {
                hex::decode(ticket.ticket.as_str())
                    .map(|bytes| (ticket.expires_at, bytes))
                    .map_err(std::io::Error::other)
            })
            .transpose()?;
        let stamp_cost = match options.stamp_cost {
            Some(cost) => Some(cost),
            None => daemon.outbound_stamp_cost_for(record.destination.as_str())?,
        };
        let outbound_ticket = match options.ticket.clone() {
            Some(ticket) => Some(ticket),
            None => {
                daemon.outbound_ticket_for(record.destination.as_str())?.map(|ticket| ticket.ticket)
            }
        };

        let requested_method = RequestedDeliveryMethod::parse(options.method.as_deref())?;
        let propagation_node_hex = daemon.outbound_propagation_node();
        let propagation_node_identity = if requested_method == RequestedDeliveryMethod::Propagated
            || (requested_method == RequestedDeliveryMethod::Direct
                && options.try_propagation_on_fail)
        {
            propagation_node_hex.as_deref().and_then(|node_hex| {
                let cached = match self.outbound_propagation_identities.lock() {
                    Ok(guard) => guard.get(node_hex).cloned(),
                    Err(err) => {
                        log::warn!(
                            "[daemon] failed to read propagation identity cache for {node_hex}: {err}"
                        );
                        None
                    }
                };
                cached.or_else(|| {
                    let hash = parse_destination_hash_required(node_hex).ok()?;
                    let hash = AddressHash::new(hash);
                    if let Some(identity) =
                        identity_resolver::persisted_identity_for_destination(daemon.as_ref(), hash)
                    {
                        if let Ok(mut guard) = self.outbound_propagation_identities.lock() {
                            guard.insert(node_hex.to_string(), identity);
                        }
                        return Some(identity);
                    }
                    let identity = match resolve_destination_identity_blocking(
                        self.transport.clone(),
                        hash,
                        Duration::from_secs(12),
                    ) {
                        Ok(Some(id)) => id,
                        Ok(None) => return None,
                        Err(err) => {
                            log::warn!(
                                "[daemon] identity resolver for propagation node {node_hex}: {err}"
                            );
                            return None;
                        }
                    };
                    if let Ok(mut guard) = self.outbound_propagation_identities.lock() {
                        guard.insert(node_hex.to_string(), identity);
                    }
                    Some(identity)
                })
            })
        } else {
            None
        };
        if requested_method == RequestedDeliveryMethod::Paper {
            log_delivery_trace(
                &record.id,
                &record.destination,
                "paper",
                "deferred to sdk_paper_encode_v2",
            );
            return Ok(());
        }

        let task = DeliveryTask {
            daemon,
            transport: self.transport.clone(),
            peer_crypto: self.peer_crypto.clone(),
            outbound_propagation_identities: self.outbound_propagation_identities.clone(),
            receipt_map: self.receipt_map.clone(),
            outbound_resource_map: self.outbound_resource_map.clone(),
            outbound_propagation_link: self.outbound_propagation_link.clone(),
            receipt_tx: self.receipt_tx.clone(),
            message_id: record.id.clone(),
            source_hash: self.delivery_source_hash,
            destination,
            destination_hash: AddressHash::new(destination),
            destination_hex: record.destination.clone(),
            title: record.title.clone(),
            content: record.content.clone(),
            fields: record.fields.clone(),
            signer: self.signer.clone(),
            stamp_cost,
            outbound_ticket,
            include_ticket: include_ticket_bytes,
            peer_identity,
            propagation_node_identity,
            requested_method,
            try_propagation_on_fail: options.try_propagation_on_fail,
            propagation_node_hex,
        };
        self.delivery_scheduler.enqueue(task)
    }

    fn delivery_pipeline_status(&self) -> Option<serde_json::Value> {
        Some(self.delivery_scheduler.status_json())
    }
}

fn outbound_peer_identity(
    daemon: &RpcDaemon,
    peer_crypto: &Mutex<HashMap<String, PeerCrypto>>,
    destination_hex: &str,
    destination: [u8; 16],
    local_delivery_identity: Option<Identity>,
) -> Option<Identity> {
    if let Some(local_delivery_identity) = local_delivery_identity {
        return Some(local_delivery_identity);
    }
    peer_crypto
        .lock()
        .expect("peer map")
        .get(destination_hex)
        .copied()
        .map(|info| info.identity)
        .or_else(|| {
            identity_resolver::persisted_identity_for_destination(
                daemon,
                AddressHash::new(destination),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_rpc::MessagesStore;

    #[test]
    fn outbound_peer_identity_uses_persisted_announce_identity_when_live_cache_is_empty() {
        let store = MessagesStore::in_memory().expect("store");
        let daemon = RpcDaemon::with_store(store, "test-node".to_string());
        let remote = rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let delivery_hash = SingleOutputDestination::new(
            *remote.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc
        .address_hash;
        let mut destination = [0u8; 16];
        destination.copy_from_slice(delivery_hash.as_slice());
        let destination_hex = hex::encode(destination);
        daemon
            .record_announce_identity(
                destination_hex.as_str(),
                hex::encode(remote.as_identity().public_key_bytes()).as_str(),
                hex::encode(remote.as_identity().verifying_key_bytes()).as_str(),
                1_781_964_554,
            )
            .expect("record announce identity");
        let peer_crypto = Mutex::new(HashMap::new());

        let identity = outbound_peer_identity(
            &daemon,
            &peer_crypto,
            destination_hex.as_str(),
            destination,
            None,
        )
        .expect("persisted identity");

        assert_eq!(identity.public_key_bytes(), remote.as_identity().public_key_bytes());
        assert_eq!(identity.verifying_key_bytes(), remote.as_identity().verifying_key_bytes());
    }

    #[test]
    fn outbound_peer_identity_uses_local_delivery_identity_without_announce() {
        let store = MessagesStore::in_memory().expect("store");
        let daemon = RpcDaemon::with_store(store, "test-node".to_string());
        let local = rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let delivery_hash = SingleOutputDestination::new(
            *local.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        )
        .desc
        .address_hash;
        let mut destination = [0u8; 16];
        destination.copy_from_slice(delivery_hash.as_slice());
        let peer_crypto = Mutex::new(HashMap::new());

        let identity = outbound_peer_identity(
            &daemon,
            &peer_crypto,
            hex::encode(destination).as_str(),
            destination,
            Some(*local.as_identity()),
        )
        .expect("local identity");

        assert_eq!(identity.public_key_bytes(), local.as_identity().public_key_bytes());
        assert_eq!(identity.verifying_key_bytes(), local.as_identity().verifying_key_bytes());
    }
}
