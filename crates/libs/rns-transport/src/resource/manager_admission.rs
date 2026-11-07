impl ResourceManager {
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
        if let Some(assembly) = self.incoming_segments.get(&advertisement.original_hash) {
            if assembly.link_id != *link.id()
                || assembly.request_id.as_deref()
                    != advertisement.request_id.as_ref().map(|id| id.as_ref())
                || assembly.is_request != advertisement.is_request()
                || assembly.is_response != advertisement.is_response()
                || assembly.total_segments != advertisement.total_segments
                || assembly.total_data_size != advertisement.data_size
            {
                log::warn!(
                    "rejecting inconsistent resource advertisement link={} original_hash={}",
                    link.id(),
                    advertisement.original_hash
                );
                return;
            }
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
            log::debug!(
                "resource inbound: duplicate advertisement for active receiver hash={}",
                resource_hash
            );
            return;
        }
        if advertisement.is_response() {
            let Some(id) = advertisement.request_id.as_deref() else {
                log::warn!(
                    "response Resource has no request ID link={} resource={}",
                    link.id(),
                    resource_hash
                );
                return;
            };
            if !link.response_resource_allowed(id, advertisement.data_size) {
                Self::reject_response_advertisement(link, resource_hash, responses);
                return;
            }
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
        // Only a validated, newly admitted receiver can bind an owned request.
        if advertisement.is_response() {
            let id =
                advertisement.request_id.as_deref().expect("response ID checked before allocation");
            if !link.register_response_resource(
                id,
                advertisement.original_hash,
                resource_hash,
                advertisement.data_size,
            ) {
                Self::reject_response_advertisement(link, resource_hash, responses);
                return;
            }
            link.start_response_resource(resource_hash, advertisement.data_size);
        }
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
        let request = receiver.build_request(
            adv_now,
            stats.rtt,
            stats.arrival_interval,
            RequestTrigger::Immediate,
        );
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

    fn reject_response_advertisement(link: &Link, hash: Hash, responses: &mut Vec<Packet>) {
        log::warn!("rejecting response Resource link={} resource={}", link.id(), hash);
        match build_link_packet(
            link,
            PacketType::Data,
            PacketContext::ResourceReceiverCancel,
            hash.as_slice(),
        ) {
            Ok(packet) => responses.push(packet),
            Err(error) => log::warn!(
                "response Resource rejection frame failed link={} resource={} error={}",
                link.id(),
                hash,
                error
            ),
        }
    }
}
