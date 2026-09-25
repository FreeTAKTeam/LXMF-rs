// The two time-driven entry points into `ResourceManager`: the inbound retry
// sweep and the outbound send pump. Split out of `manager.rs` to keep both
// files inside the repository's 500-line budget; both are `include!`d into the
// same `resource` module, so this is a file boundary rather than a privacy one.

impl ResourceManager {
    pub fn retry_requests(&mut self, now: Instant) -> Vec<(AddressHash, ResourceRequest)> {
        let mut requests = Vec::new();
        let mut failed = Vec::new();
        for (hash, receiver) in self.incoming.iter_mut() {
            if receiver.retry_due(now, self.retry_interval, self.retry_limit) {
                let stats =
                    self.link_stats.get(&receiver.link_id).copied().unwrap_or_else(LinkStats::new);
                let request = receiver.build_request(now, stats.rtt, stats.arrival_interval, RequestTrigger::Immediate);
                if !request.requested_hashes.is_empty() || request.hashmap_exhausted {
                    receiver.mark_request();
                    requests.push((receiver.link_id, request));
                }
            }
            if receiver.retry_count >= self.retry_limit {
                failed.push((*hash, receiver.original_hash, receiver.link_id, receiver.progress()));
            }
        }
        for (hash, original_hash, link_id, progress) in failed {
            log::warn!("resource transfer failed link={link_id} hash={hash} reason=retry_limit_exhausted");
            self.incoming.remove(&hash);
            if !self.fail_inbound_segments(original_hash, "retry_limit_exhausted") {
                self.events.push(ResourceEvent {
                    hash: original_hash,
                    link_id,
                    kind: ResourceEventKind::InboundFailed(ResourceFailure {
                        reason: "retry_limit_exhausted".to_string(),
                        progress,
                    }),
                });
            }
        }
        requests
    }

    pub fn poll_outgoing(&mut self, now: Instant) -> Vec<(AddressHash, Packet)> {
        let mut packets = Vec::new();
        let mut failed = Vec::new();

        for (hash, sender) in self.outgoing.iter_mut() {
            match sender.poll(now, self.retry_interval) {
                OutboundResourcePoll::Send(packet) => {
                    packets.push((sender.link_id, (*packet).clone()));
                }
                OutboundResourcePoll::Failed => {
                    self.events.push(ResourceEvent {
                        hash: sender.original_hash,
                        link_id: sender.link_id,
                        kind: ResourceEventKind::OutboundFailed,
                    });
                    failed.push((*hash, sender.original_hash));
                }
                OutboundResourcePoll::None => {}
            }
        }

        for (hash, original_hash) in failed {
            self.outgoing.remove(&hash);
            self.outgoing_segment_chains.remove(&original_hash);
        }

        packets
    }
}
