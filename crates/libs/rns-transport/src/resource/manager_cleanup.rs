impl ResourceManager {
    pub fn cancel_outgoing(
        &mut self,
        resource_hash: Hash,
        link: &Link,
    ) -> Result<Option<Packet>, RnsError> {
        let active_hash = self.outgoing.iter().find_map(|(hash, sender)| {
            (sender.original_hash == resource_hash).then_some((*hash, sender.link_id))
        });
        if let Some((active_hash, link_id)) = active_hash {
            if link_id != *link.id() {
                return Err(RnsError::InvalidArgument);
            }
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

        if self
            .pending_outgoing
            .get(&resource_hash)
            .is_some_and(|sender| sender.link_id != *link.id())
        {
            return Err(RnsError::InvalidArgument);
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
        let mut failed_outbound = std::collections::HashSet::new();
        let pending_outgoing = std::mem::take(&mut self.pending_outgoing);
        for (hash, sender) in pending_outgoing {
            if sender.link_id == link_id {
                if failed_outbound.insert(sender.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: sender.original_hash,
                        link_id,
                        kind: ResourceEventKind::OutboundFailed,
                    });
                }
            } else {
                self.pending_outgoing.insert(hash, sender);
            }
        }
        let outgoing = std::mem::take(&mut self.outgoing);
        for (hash, sender) in outgoing {
            if sender.link_id == link_id {
                if failed_outbound.insert(sender.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: sender.original_hash,
                        link_id,
                        kind: ResourceEventKind::OutboundFailed,
                    });
                }
            } else {
                self.outgoing.insert(hash, sender);
            }
        }
        // A split transfer has one chain keyed by its original hash alongside
        // the currently active segment. Keep the terminal signal singular even
        // when link cleanup observes both structures, and still report a chain
        // whose active segment has already been removed by an earlier failure.
        let outgoing_segment_chains = std::mem::take(&mut self.outgoing_segment_chains);
        for (hash, pending) in outgoing_segment_chains {
            if pending.link_id == link_id {
                if failed_outbound.insert(pending.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: pending.original_hash,
                        link_id,
                        kind: ResourceEventKind::OutboundFailed,
                    });
                }
            } else {
                self.outgoing_segment_chains.insert(hash, pending);
            }
        }

        let mut failed_inbound_segments = std::collections::HashSet::new();
        let incoming_segments = std::mem::take(&mut self.incoming_segments);
        for (hash, assembly) in incoming_segments {
            if assembly.link_id == link_id {
                failed_inbound_segments.insert(hash);
                self.events.push(ResourceEvent {
                    hash,
                    link_id,
                    kind: ResourceEventKind::InboundFailed(ResourceFailure {
                        reason: "link_closed".to_string(),
                        progress: ResourceProgress {
                            received_bytes: assembly.data.len() as u64,
                            total_bytes: assembly.total_data_size,
                            received_parts: assembly.next_segment.saturating_sub(1) as usize,
                            total_parts: assembly.total_segments as usize,
                        },
                    }),
                });
            } else {
                self.incoming_segments.insert(hash, assembly);
            }
        }
        let incoming = std::mem::take(&mut self.incoming);
        for (hash, receiver) in incoming {
            if receiver.link_id == link_id {
                // Segment n has a live receiver while the completed segments
                // remain in `incoming_segments` under the same original hash.
                // The assembly event above is the logical transfer failure;
                // avoid emitting a second terminal callback for that transfer.
                if !failed_inbound_segments.contains(&receiver.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: receiver.original_hash,
                        link_id,
                        kind: ResourceEventKind::InboundFailed(ResourceFailure {
                            reason: "link_closed".to_string(),
                            progress: receiver.progress(),
                        }),
                    });
                }
            } else {
                self.incoming.insert(hash, receiver);
            }
        }
        self.link_stats.remove(&link_id);
    }
}
