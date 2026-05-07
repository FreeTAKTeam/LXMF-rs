use super::identity_resolver;
use super::*;

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

impl DeliveryTask {
    fn cached_identity_for_destination(&self, destination_hash: AddressHash) -> Option<Identity> {
        identity_resolver::cached_identity_for_destination(
            destination_hash,
            self.peer_identity,
            self.propagation_node_identity,
            &self.peer_crypto,
            &self.outbound_propagation_identities,
        )
    }

    pub(super) async fn run(self) {
        log_delivery_trace(&self.message_id, &self.destination_hex, "start", "delivery requested");
        if self.abort_if_cancelled("start") {
            return;
        }
        let payload = match self.build_payload().await {
            Ok(payload) => payload,
            Err(err) => {
                if self.abort_if_cancelled("payload") {
                    return;
                }
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
                return;
            }
        };
        if self.abort_if_cancelled("payload") {
            return;
        }
        match self.requested_method {
            RequestedDeliveryMethod::Direct => self.run_direct(payload).await,
            RequestedDeliveryMethod::Opportunistic => self.run_opportunistic(payload).await,
            RequestedDeliveryMethod::Propagated => self.run_propagated(payload).await,
            RequestedDeliveryMethod::Paper => {}
        }
    }

    async fn run_direct(self, payload: Vec<u8>) {
        if self.abort_if_cancelled("link") {
            return;
        }
        let Some(identity) = self.resolve_destination_identity().await else {
            return;
        };
        if self.abort_if_cancelled("link") {
            return;
        }
        let destination_desc = DestinationDesc {
            identity,
            address_hash: self.destination_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };

        match self
            .send_via_link_mode(
                "link",
                self.destination_hex.as_str(),
                destination_desc,
                &payload,
                LinkModeStatuses {
                    packet: "sent: link",
                    resource: "sending: link resource",
                    resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
                },
            )
            .await
        {
            Ok(()) => {}
            Err(err) if self.try_propagation_on_fail && self.propagation_node_hex.is_some() => {
                let detail = format!("direct failed err={err}; trying propagated");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: format!("link failed: {err}; trying propagated"),
                });
                self.run_propagated(payload).await;
            }
            Err(err) => {
                let detail = format!("direct failed err={err}");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
            }
        }
    }

    async fn run_propagated(self, payload: Vec<u8>) {
        if self.abort_if_cancelled("propagation") {
            return;
        }
        let Some(destination_identity) = self.resolve_destination_identity().await else {
            return;
        };
        if self.abort_if_cancelled("propagation") {
            return;
        }
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "recipient identity ready",
        );
        let Some(propagation_node_hex) = self.propagation_node_hex.clone() else {
            let _ = self.receipt_tx.try_send(ReceiptEvent {
                message_id: self.message_id,
                status: "failed: no outbound propagation node selected".to_string(),
            });
            return;
        };

        let propagation_hash = match parse_destination_hash_required(&propagation_node_hex) {
            Ok(hash) => AddressHash::new(hash),
            Err(err) => {
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
                return;
            }
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "selected propagation node parsed",
        );
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "looking up propagation stamp cost",
        );
        let target_cost = self
            .propagation_target_cost(propagation_node_hex.as_str())
            .unwrap_or(propagation::DEFAULT_PROPAGATION_STAMP_COST);
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            format!("using propagation stamp cost={target_cost}").as_str(),
        );
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "building propagation payload",
        );
        self.record_propagation_stamp_work_metadata("generating", target_cost, None);
        if self.abort_if_cancelled("propagation") {
            self.record_propagation_stamp_work_metadata("cancelled", target_cost, None);
            return;
        }
        let propagation_payload = match propagation::build_propagation_payload_until_cancelled(
            &payload,
            &destination_identity,
            target_cost,
            || {
                let status = self.daemon.message_receipt_status(&self.message_id).ok().flatten();
                Self::is_cancelled_status(status.as_deref())
            },
        ) {
            Ok(payload) => payload,
            Err(err) => {
                if self.abort_if_cancelled("propagation") {
                    self.record_propagation_stamp_work_metadata("cancelled", target_cost, None);
                    return;
                }
                self.record_propagation_stamp_work_metadata(
                    "failed",
                    target_cost,
                    Some(err.to_string()),
                );
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
                return;
            }
        };
        self.record_propagation_stamp_work_metadata(
            "ready",
            target_cost,
            Some(propagation_payload.stamp_value.to_string()),
        );
        self.record_propagation_payload_metadata(&propagation_payload, target_cost);
        let payload = propagation_payload.bytes;
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            format!("propagation payload ready bytes={}", payload.len()).as_str(),
        );
        if self.abort_if_cancelled("propagation") {
            return;
        }

        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "resolving propagation link",
        );
        let propagation_link = match self
            .resolve_or_create_propagation_link(&propagation_node_hex, propagation_hash)
            .await
        {
            Ok(link) => link,
            Err(err) => {
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
                return;
            }
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "propagation link ready",
        );
        if self.abort_if_cancelled("propagation") {
            return;
        }

        if let Err(err) = self
            .send_via_existing_link_mode(
                "propagation",
                propagation_node_hex.as_str(),
                propagation_link,
                &payload,
                LinkModeStatuses {
                    packet: "sent: propagated",
                    resource: "sending: propagated resource",
                    resource_sent: "sent: propagated resource",
                },
            )
            .await
        {
            let detail = format!("propagated failed err={err}");
            log_delivery_trace(&self.message_id, &self.destination_hex, "propagation", &detail);
            let _ = self.receipt_tx.try_send(ReceiptEvent {
                message_id: self.message_id,
                status: format!("failed: {err}"),
            });
        }
    }

    async fn run_opportunistic(self, payload: Vec<u8>) {
        if self.abort_if_cancelled("opportunistic") {
            return;
        }
        // Opportunistic SINGLE packets must carry LXMF wire bytes
        // without the destination prefix. Receivers prepend the
        // packet destination hash before unpacking.
        let opportunistic_payload = opportunistic_payload(&payload, &self.destination);
        let mut data = PacketDataBuffer::new();
        if data.write(opportunistic_payload).is_err() {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic",
                "payload too large",
            );
            let _ = self.receipt_tx.try_send(ReceiptEvent {
                message_id: self.message_id,
                status: "failed: opportunistic payload too large".to_string(),
            });
            return;
        }

        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: self.destination_hash,
            transport: None,
            context: PacketContext::None,
            data,
        };
        let packet_hash = hex::encode(packet.hash().to_bytes());
        track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
        if diagnostics_enabled() {
            let detail = format!(
                "sending packet_hash={} payload_len={} payload_prefix={}",
                packet_hash,
                opportunistic_payload.len(),
                payload_preview(opportunistic_payload, 16)
            );
            log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", &detail);
        } else {
            log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", "sending");
        }
        let trace = self.transport.send_packet_with_trace(packet).await;
        let trace_detail = send_trace_detail(trace);
        log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", &trace_detail);
        let outcome = trace.outcome;
        if !send_outcome_is_sent(outcome) {
            if let Ok(mut map) = self.receipt_map.lock() {
                map.remove(&packet_hash);
            }
        }
        let _ = self.receipt_tx.try_send(ReceiptEvent {
            message_id: self.message_id,
            status: send_outcome_status("opportunistic", outcome),
        });
    }

    async fn resolve_destination_identity(&self) -> Option<Identity> {
        let identity = self
            .resolve_identity(
                Some(self.destination_hex.as_str()),
                self.destination_hash,
                self.peer_identity,
                "identity",
                "failed: peer not announced",
            )
            .await?;

        if let Ok(mut peers) = self.peer_crypto.lock() {
            peers.insert(self.destination_hex.clone(), PeerCrypto { identity });
        }
        Some(identity)
    }

    async fn resolve_or_create_propagation_link(
        &self,
        propagation_node_hex: &str,
        propagation_hash: AddressHash,
    ) -> Result<Arc<tokio::sync::Mutex<Link>>, std::io::Error> {
        if let Some(link) = propagation::cached_propagation_link(
            &self.outbound_propagation_link,
            propagation_node_hex,
        )
        .await
        {
            return Ok(link);
        }

        let cached_identity = self
            .propagation_node_identity
            .or_else(|| {
                self.outbound_propagation_identities
                    .lock()
                    .ok()
                    .and_then(|guard| guard.get(propagation_node_hex).cloned())
            })
            .or_else(|| {
                resolve_destination_identity_blocking(
                    self.transport.clone(),
                    propagation_hash,
                    Duration::from_secs(12),
                )
            });
        let Some(propagation_identity) = self
            .resolve_identity(
                Some(propagation_node_hex),
                propagation_hash,
                cached_identity,
                "propagation-node",
                "failed: propagation node not announced",
            )
            .await
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "propagation node not announced",
            ));
        };
        if let Ok(mut guard) = self.outbound_propagation_identities.lock() {
            guard.insert(propagation_node_hex.to_string(), propagation_identity);
        }

        let propagation_destination = SingleOutputDestination::new(
            propagation_identity,
            DestinationName::new("lxmf", "propagation"),
        );

        Ok(propagation::propagation_link_for_node(
            self.transport.as_ref(),
            &self.outbound_propagation_link,
            propagation_node_hex,
            propagation_destination.desc,
        )
        .await)
    }

    async fn resolve_identity(
        &self,
        destination_hex: Option<&str>,
        destination_hash: AddressHash,
        cached: Option<Identity>,
        stage: &str,
        failure_status: &str,
    ) -> Option<Identity> {
        let mut identity =
            cached.or_else(|| self.cached_identity_for_destination(destination_hash));
        if identity.is_some() {
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(
                &self.message_id,
                detail,
                stage,
                "resolved from cached peer identity",
            );
        }

        if identity.is_none() {
            self.transport.request_path(&destination_hash, None, None).await;
            log_delivery_trace(&self.message_id, &self.destination_hex, stage, "path-requested");
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(&self.message_id, detail, stage, "waiting for announce");
            let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
            while tokio::time::Instant::now() < deadline {
                if self.abort_if_cancelled(stage) {
                    return None;
                }
                if let Some(found) = self.transport.destination_identity(&destination_hash).await {
                    identity = Some(found);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        if identity.is_none() {
            identity = self.cached_identity_for_destination(destination_hash);
            if identity.is_some() {
                let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
                log_delivery_trace(
                    &self.message_id,
                    detail,
                    stage,
                    "resolved from cached peer identity",
                );
            }
        }

        let Some(identity) = identity else {
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(&self.message_id, detail, stage, "not found");
            let _ = self.receipt_tx.try_send(ReceiptEvent {
                message_id: self.message_id.clone(),
                status: failure_status.to_string(),
            });
            return None;
        };

        let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
        log_delivery_trace(&self.message_id, detail, stage, "resolved");
        Some(identity)
    }
}
