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
        let peer_info =
            self.peer_crypto.lock().expect("peer map").get(&record.destination).copied();
        let peer_identity = peer_info.map(|info| info.identity);
        let daemon = self
            .daemon
            .lock()
            .expect("transport bridge daemon mutex poisoned")
            .clone()
            .ok_or_else(|| std::io::Error::other("daemon bridge unavailable"))?;

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
        let propagation_node_identity = if requested_method == RequestedDeliveryMethod::Propagated {
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
                        let identity = resolve_destination_identity_blocking(
                            self.transport.clone(),
                            hash,
                            Duration::from_secs(12),
                        )?;
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
