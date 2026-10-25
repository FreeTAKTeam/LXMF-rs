enum CleanupScope<'a> {
    Link(AddressHash),
    Request {
        link_id: AddressHash,
        request_id: &'a [u8],
        outbound: Option<Hash>,
        inbound: &'a std::collections::HashSet<Hash>,
    },
}

impl CleanupScope<'_> {
    fn link_id(&self) -> AddressHash {
        match self {
            Self::Link(id) | Self::Request { link_id: id, .. } => *id,
        }
    }

    fn outgoing(&self, link_id: AddressHash, original: Hash) -> bool {
        link_id == self.link_id()
            && match self {
                Self::Link(_) => true,
                Self::Request { outbound, .. } => *outbound == Some(original),
            }
    }

    fn incoming(
        &self,
        link_id: AddressHash,
        original: Hash,
        active: Hash,
        request: Option<&[u8]>,
        is_response: bool,
    ) -> bool {
        link_id == self.link_id()
            && match self {
                Self::Link(_) => true,
                Self::Request { request_id, inbound, .. } => {
                    is_response
                        && request == Some(*request_id)
                        && (inbound.contains(&original) || inbound.contains(&active))
                }
            }
    }

    fn outbound_outcome(&self) -> ResourceEventKind {
        match self {
            Self::Link(_) => ResourceEventKind::OutboundFailed,
            Self::Request { .. } => ResourceEventKind::OutboundCancelled,
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            Self::Link(_) => "link_closed",
            Self::Request { .. } => "request_cancelled",
        }
    }
}

impl ResourceManager {
    /// Abort only the retained offline owner's transfer, including segmented sends.
    pub fn abort_owned_outgoing(&mut self, resource_hash: Hash, link_id: AddressHash) {
        let mut removed = false;
        self.outgoing.retain(|_, sender| {
            let owned = sender.original_hash == resource_hash && sender.link_id == link_id;
            removed |= owned;
            !owned
        });
        self.pending_outgoing.retain(|_, sender| {
            let owned = sender.original_hash == resource_hash && sender.link_id == link_id;
            removed |= owned;
            !owned
        });
        if removed {
            self.outgoing_segment_chains.remove(&resource_hash);
            self.events.push(ResourceEvent { hash: resource_hash, link_id, kind: ResourceEventKind::OutboundCancelled });
        }
    }

    #[cfg(test)]
    pub(crate) fn has_incoming_resource(&self, link_id: AddressHash, hash: Hash) -> bool {
        self.incoming
            .get(&hash)
            .is_some_and(|receiver| receiver.link_id == link_id && receiver.is_active())
    }

    /// Remove only one request's logical transfers even when cancellation-frame
    /// encryption fails or its original Link generation no longer exists.
    pub(crate) fn cancel_request_state(
        &mut self,
        link_id: AddressHash,
        request_id: &[u8],
        outbound: Option<Hash>,
        inbound: &std::collections::HashSet<Hash>,
        link: Option<&Link>,
    ) -> Vec<Packet> {
        let scope = CleanupScope::Request { link_id, request_id, outbound, inbound };
        self.cancel_scoped_state(scope, link, |link, context, hash| {
            build_link_packet(link, PacketType::Data, context, hash.as_slice())
        })
    }

    fn cancel_scoped_state(
        &mut self,
        scope: CleanupScope<'_>,
        link: Option<&Link>,
        mut cancel_frame: impl FnMut(&Link, PacketContext, Hash) -> Result<Packet, RnsError>,
    ) -> Vec<Packet> {
        let link_id = scope.link_id();
        let mut packets = Vec::new();
        if let Some(link) = link.filter(|link| {
            *link.id() == link_id && link.status() == crate::destination::link::LinkStatus::Active
        }) {
            let outgoing = self
                .outgoing
                .iter()
                .filter(|(_, sender)| scope.outgoing(sender.link_id, sender.original_hash))
                .map(|(hash, _)| (*hash, PacketContext::ResourceInitiatorCancel));
            let incoming = self
                .incoming
                .iter()
                .filter(|(hash, receiver)| {
                    scope.incoming(
                        receiver.link_id,
                        receiver.original_hash,
                        **hash,
                        receiver.request_id.as_deref(),
                        receiver.is_response,
                    )
                })
                .map(|(hash, _)| (*hash, PacketContext::ResourceReceiverCancel));
            for (hash, context) in outgoing.chain(incoming) {
                match cancel_frame(link, context, hash) {
                    Ok(packet) => packets.push(packet),
                    Err(error) => log::warn!(
                        "request Resource cancellation frame failed link={} resource={} error={}",
                        link_id,
                        hash,
                        error
                    ),
                }
            }
        }
        self.remove_transfer_state(scope);
        packets
    }

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
        self.remove_transfer_state(CleanupScope::Link(link_id));
    }

    fn remove_transfer_state(&mut self, scope: CleanupScope<'_>) {
        let link_id = scope.link_id();
        let mut failed_outbound = std::collections::HashSet::new();
        let pending_outgoing = std::mem::take(&mut self.pending_outgoing);
        for (hash, sender) in pending_outgoing {
            if scope.outgoing(sender.link_id, sender.original_hash) {
                if failed_outbound.insert(sender.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: sender.original_hash,
                        link_id,
                        kind: scope.outbound_outcome(),
                    });
                }
            } else {
                self.pending_outgoing.insert(hash, sender);
            }
        }
        let outgoing = std::mem::take(&mut self.outgoing);
        for (hash, sender) in outgoing {
            if scope.outgoing(sender.link_id, sender.original_hash) {
                if failed_outbound.insert(sender.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: sender.original_hash,
                        link_id,
                        kind: scope.outbound_outcome(),
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
            if scope.outgoing(pending.link_id, pending.original_hash) {
                if failed_outbound.insert(pending.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: pending.original_hash,
                        link_id,
                        kind: scope.outbound_outcome(),
                    });
                }
            } else {
                self.outgoing_segment_chains.insert(hash, pending);
            }
        }

        let mut failed_inbound_segments = std::collections::HashSet::new();
        let incoming_segments = std::mem::take(&mut self.incoming_segments);
        for (hash, assembly) in incoming_segments {
            if scope.incoming(
                assembly.link_id,
                hash,
                hash,
                assembly.request_id.as_deref(),
                assembly.is_response,
            ) {
                failed_inbound_segments.insert(hash);
                self.events.push(ResourceEvent {
                    hash,
                    link_id,
                    kind: ResourceEventKind::InboundFailed(ResourceFailure {
                        reason: scope.reason().to_string(),
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
            if scope.incoming(
                receiver.link_id,
                receiver.original_hash,
                hash,
                receiver.request_id.as_deref(),
                receiver.is_response,
            ) {
                // Segment n has a live receiver while the completed segments
                // remain in `incoming_segments` under the same original hash.
                // The assembly event above is the logical transfer failure;
                // avoid emitting a second terminal callback for that transfer.
                if !failed_inbound_segments.contains(&receiver.original_hash) {
                    self.events.push(ResourceEvent {
                        hash: receiver.original_hash,
                        link_id,
                        kind: ResourceEventKind::InboundFailed(ResourceFailure {
                            reason: scope.reason().to_string(),
                            progress: receiver.progress(),
                        }),
                    });
                }
            } else {
                self.incoming.insert(hash, receiver);
            }
        }
        if matches!(scope, CleanupScope::Link(_)) {
            self.link_stats.remove(&link_id);
        }
    }
}
