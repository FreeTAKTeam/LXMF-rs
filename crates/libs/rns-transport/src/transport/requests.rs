use super::*;
use crate::destination::link::{LinkRequest, RequestReceipt, RequestSource, RequestStatus};
use crate::resource::PreparedSend;

enum RequestBody {
    Packet(Packet),
    Resource(Box<PreparedSend>),
}

impl RequestBody {
    fn source(&self) -> RequestSource {
        match self {
            Self::Packet(packet) => RequestSource::Packet(packet.hash()),
            Self::Resource(resource) => RequestSource::Resource(resource.original_hash()),
        }
    }
}

struct PreparedRequest {
    link_id: AddressHash,
    request_id: Vec<u8>,
    body: RequestBody,
}

impl Transport {
    /// Send a request on this exact retained Link and observe its native outcome.
    /// Packet/Resource selection follows the negotiated MDU. Dropping this send
    /// before it returns cancels its registration; dropping a returned receipt
    /// or its wait does not cancel the request.
    pub async fn send_request_on_link(
        &self,
        link: &Arc<Mutex<Link>>,
        request: LinkRequest,
        timeout: Duration,
        max_response_size: Option<usize>,
    ) -> Result<RequestReceipt, RnsError> {
        self.send_request_on_link_observed(link, request, timeout, max_response_size, |_| {}).await
    }

    /// The observer receives a retained token before first handoff, with no
    /// native lock held. It can retain the outcome even when dispatch fails.
    /// A public Link restart can close this generation after dispatch commits.
    pub async fn send_request_on_link_observed(
        &self,
        link: &Arc<Mutex<Link>>,
        request: LinkRequest,
        timeout: Duration,
        max_response_size: Option<usize>,
        observe: impl FnOnce(RequestReceipt),
    ) -> Result<RequestReceipt, RnsError> {
        let (link_id, iface) = {
            let link = link.lock().await;
            if link.status() != LinkStatus::Active {
                return Err(RnsError::ConnectionError);
            }
            (*link.id(), link.ingress_iface())
        };
        let mtu = self.resource_mtu_for_iface(iface).await;
        let (request_id, prepared) = {
            let link = link.lock().await;
            if *link.id() != link_id || link.status() != LinkStatus::Active {
                return Err(RnsError::ConnectionError);
            }
            if request.packed.len() <= link.link_mdu() {
                let packet = link.request_packet(&request.packed)?;
                let id = packet.hash().to_bytes()[..crate::hash::ADDRESS_HASH_SIZE].to_vec();
                (id, RequestBody::Packet(packet))
            } else {
                // Recompute from the actual packed body; LinkRequest is public.
                let id = crate::hash::address_hash(&request.packed).to_vec();
                let prepared = ResourceManager::prepare_send(
                    &link,
                    request.packed,
                    None,
                    Some(id.clone()),
                    false,
                    mtu.min(link.link_mtu()),
                )?;
                (id, RequestBody::Resource(Box::new(prepared)))
            }
        };
        self.send_prepared_request(
            link,
            PreparedRequest { link_id, request_id, body: prepared },
            Some(timeout),
            max_response_size,
            observe,
        )
        .await
    }

    async fn send_prepared_request(
        &self,
        link: &Arc<Mutex<Link>>,
        prepared: PreparedRequest,
        timeout: Option<Duration>,
        limit: Option<usize>,
        observe: impl FnOnce(RequestReceipt),
    ) -> Result<RequestReceipt, RnsError> {
        let PreparedRequest { link_id, request_id, body } = prepared;
        let source = body.source();
        let (receipt, mut registration, packet, resource_hash) = loop {
            let mut handler = self.handler.lock().await;
            if handler.cancel.is_cancelled() || !owns_link(&handler, link) {
                return Err(RnsError::ConnectionError);
            }
            let mut owner = match link.try_lock() {
                Ok(owner) => owner,
                Err(_) => {
                    drop(handler);
                    drop(link.lock().await);
                    continue;
                }
            };
            if *owner.id() != link_id {
                return Err(RnsError::ConnectionError);
            }
            let (receipt, registration) =
                owner.register_request(request_id, source, timeout, limit)?;
            let (packet, hash) = match body {
                RequestBody::Packet(packet) => (packet, None),
                RequestBody::Resource(prepared) => {
                    let (hash, packet) = handler.resource_manager.track_prepared(*prepared);
                    (packet, Some(hash))
                }
            };
            break (receipt, registration, packet, hash);
        };
        observe(receipt.clone());
        let sent = loop {
            let mut handler = self.handler.lock().await;
            let mut owner = match link.try_lock() {
                Ok(owner) => owner,
                Err(_) => {
                    drop(handler);
                    drop(link.lock().await);
                    continue;
                }
            };
            let owned = owns_link(&handler, link);
            let iface = (owned
                && !handler.cancel.is_cancelled()
                && owner.request_dispatch_matches(&registration))
            .then(|| owner.ingress_iface())
            .flatten();
            if !owned {
                registration.confirm_handoff(false);
                let omitted =
                    poll_request_cleanup(&mut handler, &mut owner, std::time::Instant::now());
                if !omitted.is_empty() {
                    log::debug!(
                        "request owner replaced; native cleanup omitted {} optional frames link={}",
                        omitted.len(),
                        link_id
                    );
                }
            }
            drop(owner);
            let sent = if let Some(iface) = iface {
                handler
                    .send_observed(
                        TxMessage { tx_type: TxMessageType::Direct(iface), packet },
                        |trace| registration.confirm_handoff(trace.sent_ifaces > 0),
                    )
                    .await
                    .sent_ifaces
                    > 0
            } else {
                false
            };
            registration.confirm_handoff(sent);
            if let Some(hash) = resource_hash {
                // Publish sender readiness before ingress can process the peer's
                // first request for parts under this same handler lock.
                handler.resource_manager.confirm_outbound_dispatch(hash, sent);
                super::resource_wire::publish_resource_events(&mut handler).await;
            }
            break sent;
        };
        registration.returned();
        if sent || receipt.status() == RequestStatus::Ready {
            Ok(receipt)
        } else {
            Err(RnsError::ConnectionError)
        }
    }

    pub async fn send_request_resource(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        self.send_request_resource_with_max_response_size(link_id, request_id, data, metadata, None)
            .await
    }

    pub async fn send_request_resource_with_max_response_size(
        &self,
        link_id: &AddressHash,
        request_id: Vec<u8>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        max_response_size: Option<usize>,
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let iface = link.lock().await.ingress_iface();
        let mtu = self.resource_mtu_for_iface(iface).await;
        let prepared = {
            let link = link.lock().await;
            if link.id() != link_id {
                return Err(RnsError::ConnectionError);
            }
            ResourceManager::prepare_send(
                &link,
                data,
                metadata,
                Some(request_id.clone()),
                false,
                mtu.min(link.link_mtu()),
            )?
        };
        let hash = prepared.original_hash();
        self.send_prepared_request(
            &link,
            PreparedRequest {
                link_id: *link_id,
                request_id,
                body: RequestBody::Resource(Box::new(prepared)),
            },
            None,
            max_response_size,
            |_| {},
        )
        .await?;
        Ok(hash)
    }

    /// Cancel only this registration on its exact Link owner/generation. The
    /// result reports native cancellation; optional frame failure cannot undo it.
    pub async fn cancel_request_on_link(
        &self,
        link: &Arc<Mutex<Link>>,
        receipt: &RequestReceipt,
    ) -> bool {
        self.cancel_request_on_link_observed(link, receipt, |_| {}).await
    }

    /// Observe native cancellation after releasing native locks and before
    /// optional frame I/O. After observation the future may be dropped without
    /// undoing cleanup. A foreign Link owner returns false without observation.
    pub async fn cancel_request_on_link_observed(
        &self,
        link: &Arc<Mutex<Link>>,
        receipt: &RequestReceipt,
        locally_cancelled: impl FnOnce(bool),
    ) -> bool {
        let (cancelled, packets) = loop {
            let mut handler = self.handler.lock().await;
            if !owns_link(&handler, link) {
                return false;
            }
            let mut owner = match link.try_lock() {
                Ok(owner) => owner,
                Err(_) => {
                    drop(handler);
                    drop(link.lock().await);
                    continue;
                }
            };
            let cancelled = owner.cancel_request(receipt);
            let packets = if cancelled {
                poll_request_cleanup(&mut handler, &mut owner, std::time::Instant::now())
            } else {
                Vec::new()
            };
            break (cancelled, packets);
        };
        locally_cancelled(cancelled);
        for packet in packets {
            let dispatch = self.handler.lock().await.send(packet).await;
            if dispatch.sent_ifaces == 0 {
                log::warn!("request cancellation frame handoff failed link={}", receipt.link_id());
            }
        }
        cancelled
    }
}

pub(super) fn poll_request_cleanup(
    handler: &mut TransportHandler,
    link: &mut Link,
    now: std::time::Instant,
) -> Vec<TxMessage> {
    super::resource_wire::publish_resource_events_for_link(handler, *link.id(), Some(link));
    let iface = link.ingress_iface();
    let mut messages = Vec::new();
    for cleanup in link.poll_requests(now) {
        let packets = handler.resource_manager.cancel_request_state(
            cleanup.link_id,
            &cleanup.request_id,
            cleanup.outbound,
            &cleanup.inbound,
            Some(link),
        );
        if let Some(iface) = iface {
            messages.extend(
                packets
                    .into_iter()
                    .map(|packet| TxMessage { tx_type: TxMessageType::Direct(iface), packet }),
            );
        }
    }
    super::resource_wire::publish_resource_events_for_link(handler, *link.id(), Some(link));
    messages
}

fn owns_link(handler: &TransportHandler, link: &Arc<Mutex<Link>>) -> bool {
    handler
        .in_links
        .values()
        .chain(handler.out_links.values())
        .any(|current| Arc::ptr_eq(current, link))
}

/// A closed Arc is about to lose its native maintenance owner. Drain both its
/// old-generation request cleanup and ordinary Resource state before detaching.
pub(super) fn retire_closed_link(handler: &mut TransportHandler, link: &mut Link) {
    debug_assert_eq!(link.status(), LinkStatus::Closed);
    // Preserve ordinary Resource Link-closure failure semantics for this
    // generation, then remove any scoped cleanup left by an older generation.
    handler.resource_manager.remove_link_state(*link.id());
    let packets = poll_request_cleanup(handler, link, std::time::Instant::now());
    debug_assert!(packets.is_empty(), "closed Links cannot encrypt cancellation frames");
    super::resource_wire::publish_resource_events_for_link(handler, *link.id(), Some(link));
}

pub(super) async fn close_transport_requests(handler: &Arc<Mutex<TransportHandler>>) {
    let mut handler = handler.lock().await;
    let mut links = Vec::new();
    for link in handler.in_links.values().chain(handler.out_links.values()) {
        if !links.iter().any(|current| Arc::ptr_eq(current, link)) {
            links.push(link.clone());
        }
    }
    for link in links {
        let mut link = link.lock().await;
        super::resource_wire::publish_resource_events_for_link(
            &mut handler,
            *link.id(),
            Some(&mut link),
        );
        link.close();
        retire_closed_link(&mut handler, &mut link);
    }
    handler.in_links.clear();
    handler.out_links.clear();
    super::resource_wire::publish_resource_events(&mut handler).await;
}
