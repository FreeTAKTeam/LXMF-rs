#[derive(Debug)]
pub struct ResourceManager {
    pending_outgoing: HashMap<Hash, ResourceSender>,
    outgoing: HashMap<Hash, ResourceSender>,
    outgoing_segment_chains: HashMap<Hash, PendingSegments>,
    incoming: HashMap<Hash, ResourceReceiver>,
    incoming_segments: HashMap<Hash, InboundSegmentAssembly>,
    events: Vec<ResourceEvent>,
    retry_interval: Duration,
    retry_limit: u8,
    link_stats: HashMap<AddressHash, LinkStats>,
}

impl ResourceManager {
    pub fn cancel_outgoing(
        &mut self,
        resource_hash: Hash,
        link: &Link,
    ) -> Result<Option<Packet>, RnsError> {
        let active_hash = self
            .outgoing
            .iter()
            .find_map(|(hash, sender)| (sender.original_hash == resource_hash).then_some(*hash));
        if let Some(active_hash) = active_hash {
            let packet = build_link_packet(
                link,
                PacketType::Data,
                PacketContext::ResourceInitiatorCancel,
                active_hash.as_slice(),
            )?;
            let sender = self
                .outgoing
                .remove(&active_hash)
                .expect("outgoing sender existed before cancel packet");
            self.outgoing_segment_chains.remove(&sender.original_hash);
            self.events.push(ResourceEvent {
                hash: sender.original_hash,
                link_id: sender.link_id,
                kind: ResourceEventKind::OutboundCancelled,
            });
            return Ok(Some(packet));
        }

        if let Some(sender) = self.pending_outgoing.remove(&resource_hash) {
            self.outgoing_segment_chains.remove(&sender.original_hash);
            self.events.push(ResourceEvent {
                hash: resource_hash,
                link_id: sender.link_id,
                kind: ResourceEventKind::OutboundCancelled,
            });
        }
        Ok(None)
    }

    pub fn remove_link_state(&mut self, link_id: AddressHash) {
        self.pending_outgoing.retain(|_, sender| sender.link_id != link_id);
        self.outgoing.retain(|_, sender| sender.link_id != link_id);
        // Was `senders.front().is_none_or(..)`, which also leaked: a chain
        // drained to empty matched no link and so was retained forever.
        self.outgoing_segment_chains.retain(|_, pending| pending.link_id != link_id);
        self.incoming.retain(|_, receiver| receiver.link_id != link_id);
        self.incoming_segments.retain(|_, assembly| assembly.link_id != link_id);
        self.link_stats.remove(&link_id);
    }

    pub fn drain_events(&mut self) -> Vec<ResourceEvent> {
        std::mem::take(&mut self.events)
    }

    #[cfg(test)]
    pub(crate) fn has_no_outbound_state(&self) -> bool {
        self.pending_outgoing.is_empty() && self.outgoing.is_empty()
    }

    pub fn handle_packet(&mut self, packet: &Packet, link: &mut Link) -> Vec<Packet> {
        let mut responses = Vec::new();
        self.handle_packet_into(packet, link, &mut responses);
        responses
    }

    pub fn handle_packet_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        self.handle_packet_into_with_mtu(packet, link, responses, DEFAULT_RESOURCE_INTERFACE_MTU);
    }

    pub fn handle_packet_into_with_mtu(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
        interface_mtu: usize,
    ) {
        responses.clear();
        match packet.context {
            PacketContext::ResourceAdvrtisement => {
                self.handle_advertisement_into(packet, link, responses, interface_mtu)
            }
            PacketContext::ResourceRequest => self.handle_request_into(packet, link, responses),
            PacketContext::ResourceHashUpdate => {
                self.handle_hash_update_into(packet, link, responses)
            }
            PacketContext::Resource => self.handle_resource_part_into(packet, link, responses),
            PacketContext::ResourceProof => self.handle_proof_into(packet, link, responses),
            PacketContext::ResourceInitiatorCancel | PacketContext::ResourceReceiverCancel => {
                self.cancel_into(packet, responses)
            }
            _ => {}
        }
    }

    fn handle_advertisement_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
        interface_mtu: usize,
    ) {
        let Ok(advertisement) = ResourceAdvertisement::unpack(packet.data.as_slice()) else {
            log::debug!("[resource-diag] reject_advertisement unpack_failed");
            return;
        };
        log::debug!(
            "[resource-diag] advertisement link={} hash={} transfer_size={} data_size={} parts={} flags=0x{:02x} request={} response={} metadata={} compressed={} encrypted={}",
            link.id(),
            advertisement.hash,
            advertisement.transfer_size,
            advertisement.data_size,
            advertisement.parts,
            advertisement.flags,
            advertisement.is_request(),
            advertisement.is_response(),
            (advertisement.flags & FLAG_METADATA) == FLAG_METADATA,
            advertisement.compressed(),
            advertisement.encrypted()
        );
        // Enforce the inbound limits before any receiver state is created
        // (issue #514) — see advertisement_limits.rs.
        if advertisement_exceeds_inbound_limits(&advertisement, link.id()) {
            return;
        }
        if advertisement.total_segments > 1 {
            let expected_segment = self
                .incoming_segments
                .get(&advertisement.original_hash)
                .map(|assembly| assembly.next_segment)
                .unwrap_or(1);
            if advertisement.segment_index != expected_segment {
                // Out-of-order split-resource segment (issue #520): we
                // deliberately log-and-drop rather than accept or reject.
                // Reference Reticulum has NO segment-ordering check and
                // blindly accepts segments in any order, so silently
                // accepting would mask a real receiver-side assembly
                // divergence here (our receiver assembles strictly in
                // order). Sending a ResourceReceiverCancel (RCL, context
                // 0x07) is the interoperable reject signal, but it would
                // tear down the sender's entire resource — much harsher
                // than the reference behavior for what is usually a
                // reordered/in-flight segment that the sender will
                // retransmit on the next request window. Dropping keeps
                // the transfer alive: the sender re-offers the missing
                // segment and normal flow resumes.
                log::warn!(
                    "rejecting out-of-order resource segment original_hash={} expected={} received={}",
                    advertisement.original_hash,
                    expected_segment,
                    advertisement.segment_index
                );
                return;
            }
        }
        let resource_hash = advertisement.hash;
        if self.incoming.get(&resource_hash).is_some_and(|receiver| receiver.is_active()) {
            log::debug!("[resource-diag] advertisement_duplicate hash={resource_hash}");
            log::debug!("resource inbound: duplicate advertisement for active receiver hash={}", resource_hash);
            return;
        }
        let receiver = if interface_mtu == DEFAULT_RESOURCE_INTERFACE_MTU {
            ResourceReceiver::new(&advertisement, *link.id())
        } else {
            ResourceReceiver::new_with_mtu(&advertisement, *link.id(), interface_mtu)
        };
        let Ok(mut receiver) = receiver else {
            log::warn!("rejecting unreasonable advertisement");
            log::debug!("[resource-diag] reject_advertisement unreasonable");
            return;
        };
        let adv_now = Instant::now();
        let stats = *self.link_stats.entry(*link.id()).or_insert_with(LinkStats::new);
        // Start where the last resource on this link finished rather than at
        // WINDOW, so a split transfer does not re-learn the same link once per
        // segment.
        //
        // Deliberately *not* clamped to `window_max`. The reference restores
        // only `window` and leaves the ceiling at its default
        // (`RNS/Resource.py`), which means a carried window may legitimately
        // start above it: the ceiling gates *growth*, and a link that has
        // already demonstrated a window of 24 should not be made to re-climb
        // to 10 before it can use it. Growth simply stays disabled until a
        // loss brings the window back under the ceiling — which is the
        // intended reading of "this link has proven this much".
        if let Some(previous) = stats.last_window {
            receiver.window = previous.max(receiver.window_min);
        }
        let request = receiver.build_request(adv_now, stats.rtt, stats.arrival_interval, RequestTrigger::Immediate);
        log::debug!(
            "[resource-diag] request_parts hash={} requested={} exhausted={}",
            resource_hash,
            request.requested_hashes.len(),
            request.hashmap_exhausted
        );
        receiver.mark_request();
        self.incoming.insert(resource_hash, receiver);
        match build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceRequest,
            &request.encode(),
        ) {
            Ok(packet) => responses.push(packet),
            Err(_) => {
                log::warn!("failed to build request packet");
            }
        };
    }

    fn handle_request_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        let Ok(request) = ResourceRequest::decode(packet.data.as_slice()) else {
            log::debug!("[resource-diag] request_decode_failed link={}", link.id());
            return;
        };
        log::debug!(
            "[resource-diag] request_received link={} hash={} requested={} exhausted={} sender_present={}",
            link.id(),
            request.resource_hash,
            request.requested_hashes.len(),
            request.hashmap_exhausted,
            self.outgoing.contains_key(&request.resource_hash)
        );
        if let Some(sender) = self.outgoing.get_mut(&request.resource_hash) {
            sender.handle_request_into(&request, link, responses);
            log::debug!(
                "[resource-diag] request_responses link={} hash={} responses={}",
                link.id(),
                request.resource_hash,
                responses.len()
            );
        }
    }

    fn handle_hash_update_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        let Ok(update) = ResourceHashUpdate::decode(packet.data.as_slice()) else {
            return;
        };
        if let Some(receiver) = self.incoming.get_mut(&update.resource_hash) {
            receiver.handle_hash_update(&update);
            let update_now = Instant::now();
            let stats =
                self.link_stats.get(&receiver.link_id).copied().unwrap_or_else(LinkStats::new);
            let request = receiver.build_request(update_now, stats.rtt, stats.arrival_interval, RequestTrigger::Immediate);
            match build_link_packet(
                link,
                PacketType::Data,
                PacketContext::ResourceRequest,
                &request.encode(),
            ) {
                Ok(packet) => {
                    receiver.mark_active_request();
                    // This request is a send like any other, so it has to
                    // refresh the timestamp the exhaustion gate measures
                    // against. A hashmap update can arrive and still leave the
                    // next fragments unmapped — small hashmap segments, or a
                    // window that has grown past one segment — in which case
                    // this dispatches *another* hashmap request. Leaving
                    // `last_request` pointing at the previous one makes the
                    // wait look already expired, so the very next part to
                    // arrive emits a duplicate, which walks a reference
                    // sender's serving window forward: exactly the stall this
                    // gate exists to prevent.
                    responses.push(packet)
                }
                Err(_) => {
                    log::warn!("failed to build request packet");
                }
            };
        }
    }

    fn handle_resource_part_into(
        &mut self,
        packet: &Packet,
        link: &mut Link,
        responses: &mut Vec<Packet>,
    ) {
        let mut completed: Option<Hash> = None;
        let mut proof_packet: Option<Packet> = None;
        let mut request_packet: Option<Packet> = None;
        let mut payload: Option<ResourcePayload> = None;
        let mut failed: Option<(Hash, AddressHash, ResourceProgress, &'static str)> = None;
        for (hash, receiver) in self.incoming.iter_mut() {
            let before_received = receiver.received;
            match receiver.handle_part(packet.data.as_slice(), link) {
                PartOutcome::NoMatch => continue,
                PartOutcome::Failed(reason) => {
                    failed = Some((*hash, receiver.link_id, receiver.progress(), reason));
                    break;
                }
                PartOutcome::Complete(packet, data_payload) => {
                    log::debug!(
                        "[resource-diag] complete hash={} len={} metadata={}",
                        hash,
                        data_payload.data.len(),
                        data_payload.metadata.as_ref().map(|data| data.len()).unwrap_or(0)
                    );
                    completed = Some(*hash);
                    proof_packet = Some(packet);
                    payload = Some(data_payload);
                    break;
                }
                PartOutcome::Incomplete => {
                    let now = Instant::now();
                    let stats = self.link_stats
                        .entry(receiver.link_id)
                        .or_insert_with(LinkStats::new);

                    // Collect RTT sample measured during handle_part (if any).
                    if let Some(rtt) = receiver.last_rtt_sample.take() {
                        stats.update_rtt(rtt);
                    }

                    if receiver.received > before_received {
                        stats.record_arrival(now);
                        log::debug!(
                            "[resource-diag] progress hash={} received={}/{} bytes={}/{}",
                            hash,
                            receiver.received,
                            receiver.parts.len(),
                            receiver.received_bytes,
                            receiver.total_bytes
                        );
                        self.events.push(ResourceEvent {
                            hash: *hash,
                            link_id: receiver.link_id,
                            kind: ResourceEventKind::Progress(receiver.progress()),
                        });
                    }

                    // Only ask again once the round has drained — the
                    // reference's `elif self.outstanding_parts == 0:` on the
                    // receive path (`RNS/Resource.py`). Rebuilding on every
                    // arriving part sends one request per fragment, since
                    // there is only ever the one slot just vacated to fill.
                    //
                    // `build_request` still runs, because it also owns the
                    // idle-timeout check and the hashmap-exhaustion signal,
                    // and both must keep working while a round is in flight.
                    // It simply finds no room in the window and returns
                    // nothing to send.
                    let (rtt, arrival_interval) = (stats.rtt, stats.arrival_interval);
                    let request = receiver.build_request(now, rtt, arrival_interval, RequestTrigger::PartReceived);
                    if !request.requested_hashes.is_empty() || request.hashmap_exhausted {
                            request_packet = match build_link_packet(
                            link,
                            PacketType::Data,
                            PacketContext::ResourceRequest,
                            &request.encode(),
                        ) {
                            Ok(packet) => Some(packet),
                            Err(_) => {
                                log::warn!("failed to build request packet");
                                None
                            }
                        };
                    }
                    break;
                }
            }
        }
        if let Some((hash, link_id, progress, reason)) = failed {
            log::warn!("resource transfer failed link={link_id} hash={hash} reason={reason}");
            self.incoming.remove(&hash);
            self.events.push(ResourceEvent {
                hash,
                link_id,
                kind: ResourceEventKind::InboundFailed(ResourceFailure {
                    reason: reason.to_string(),
                    progress,
                }),
            });
            // Reset so the inter-resource gap doesn't skew the arrival EWMA.
            // TODO: a better approach is to schedule a delayed reset — wait
            // arrival_interval * 2, and only reset if no new part has arrived by
            // then. This preserves the estimate when the next resource starts
            // immediately after this one finishes.
            if let Some(stats) = self.link_stats.get_mut(link.id()) {
                stats.last_arrival = None;
            }
            return;
        }
        if let Some(hash) = completed {
            let concluded_window = self.incoming.remove(&hash).map(|receiver| receiver.window);
            // Same TODO as the failed path above.
            if let Some(stats) = self.link_stats.get_mut(link.id()) {
                stats.last_arrival = None;
                // Hand the window this resource earned to the next one on the
                // same link, as `Link.resource_concluded` does in the
                // reference. Only on success: a window that ended in failure
                // is not evidence of what the link can carry.
                stats.last_window = concluded_window;
            }
            if let Some(payload) = payload {
                self.finish_inbound_payload(hash, *link.id(), payload);
            }
        }
        if let Some(packet) = proof_packet {
            responses.push(packet);
        } else if let Some(packet) = request_packet {
            responses.push(packet);
        }
    }

    /// A cancel names the *segment* that is in flight, not the split resource
    /// it belongs to — `cancel_outgoing` builds the packet from the active
    /// segment's own hash, and so does the reference. Dropping the sender is
    /// therefore not enough: the unbuilt tail is keyed by `original_hash`, so
    /// it would go on holding the caller's whole payload until the link closed.
    fn cancel_into(&mut self, packet: &Packet, _responses: &mut Vec<Packet>) {
        let Ok(hash_bytes) = copy_hash(packet.data.as_slice()) else {
            return;
        };
        let hash = Hash::new(hash_bytes);
        if let Some(receiver) = self.incoming.remove(&hash) {
            // A split receiver keeps the already-completed segments in a
            // separate assembly keyed by the original resource hash. A
            // remote cancel names the segment currently in flight, so remove
            // that assembly as well and report the abandoned payload instead
            // of leaving the caller waiting for a timeout.
            self.fail_inbound_segments(receiver.original_hash, "remote_cancelled");
        }
        // Removed from both, as before: a hash lives in exactly one of these,
        // but which one depends on whether dispatch has been confirmed yet.
        let cancelled = self.pending_outgoing.remove(&hash);
        if let Some(sender) = self.outgoing.remove(&hash).or(cancelled) {
            self.outgoing_segment_chains.remove(&sender.original_hash);
            self.events.push(ResourceEvent {
                hash: sender.original_hash,
                link_id: sender.link_id,
                kind: ResourceEventKind::OutboundCancelled,
            });
        }
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}
