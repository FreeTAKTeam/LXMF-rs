use super::*;

impl RequestBook {
    pub(in crate::destination::link) fn next_deadline(&self) -> Option<Instant> {
        if !self.cleanup.is_empty() {
            return Some(Instant::now());
        }
        self.entries
            .values()
            .filter_map(|record| {
                let Record::Live(live) = record else {
                    return None;
                };
                let snapshot = live.sender.borrow();
                if snapshot.status == RequestStatus::Failed {
                    return Some(Instant::now());
                }
                let started = match (live.source, snapshot.status) {
                    (RequestSource::Packet(_), RequestStatus::Sent)
                        if !snapshot.packet_acknowledged =>
                    {
                        snapshot.sent_at
                    }
                    (RequestSource::Resource(_), RequestStatus::Delivered) => snapshot.delivered_at,
                    _ => None,
                }?;
                started.checked_add(live.timeout?)
            })
            .min()
    }

    pub(in crate::destination::link) fn packet_acknowledged(
        &mut self,
        link_id: AddressHash,
        hash: Hash,
    ) {
        let bytes = hash.to_bytes();
        let id = &bytes[..crate::hash::ADDRESS_HASH_SIZE];
        if let Some(Record::Live(live)) = self.entries.get(id) {
            if live.identity.link_id == link_id
                && matches!(live.source, RequestSource::Packet(expected) if expected == hash)
            {
                live.sender.send_if_modified(|snapshot| {
                    if snapshot.status.is_terminal() || snapshot.packet_acknowledged {
                        return false;
                    }
                    // Packet ACK proves delivery, but RequestReceipt stays SENT.
                    snapshot.packet_acknowledged = true;
                    true
                });
            }
        }
    }

    pub(in crate::destination::link) fn request_resource_delivered(
        &mut self,
        link_id: AddressHash,
        hash: Hash,
        now: Instant,
    ) {
        for record in self.entries.values() {
            if let Record::Live(live) = record {
                if live.identity.link_id == link_id
                    && matches!(live.source, RequestSource::Resource(expected) if expected == hash)
                {
                    live.sender.send_if_modified(|snapshot| {
                        if snapshot.status != RequestStatus::Sent {
                            return false;
                        }
                        snapshot.status = RequestStatus::Delivered;
                        snapshot.delivered_at = Some(now);
                        true
                    });
                }
            }
        }
    }

    pub(in crate::destination::link) fn pending_failed(
        &mut self,
        request_id: &[u8],
        reason: RequestFailure,
    ) -> bool {
        // These two callbacks implement the precise 1.5.5 SENT/DELIVERED guard.
        if !matches!(self.entries.get(request_id), Some(Record::Live(live))
            if matches!(live.sender.borrow().status, RequestStatus::Sent | RequestStatus::Delivered))
        {
            return false;
        }
        self.finish(request_id, Err(reason))
    }

    pub(in crate::destination::link) fn resource_failed(
        &mut self,
        link_id: AddressHash,
        hash: Hash,
        reason: RequestFailure,
    ) {
        let ids = self.entries.iter().filter_map(|(id, record)| match record {
            Record::Live(live) if live.identity.link_id == link_id
                && (live.response_hashes.contains(&hash)
                    || matches!(live.source, RequestSource::Resource(expected) if expected == hash)) => Some(id.clone()),
            _ => None,
        }).collect::<Vec<_>>();
        for id in ids {
            self.finish(&id, Err(reason));
        }
    }

    pub(in crate::destination::link) fn poll(&mut self, now: Instant) {
        let failures = self
            .entries
            .iter()
            .filter_map(|(id, record)| {
                let Record::Live(live) = record else {
                    return None;
                };
                let snapshot = live.sender.borrow();
                if snapshot.status == RequestStatus::Failed {
                    return Some((
                        id.clone(),
                        snapshot.failure.unwrap_or(RequestFailure::OwnerUnavailable),
                    ));
                }
                let timeout = live.timeout?;
                let started = match (live.source, snapshot.status) {
                    (RequestSource::Packet(_), RequestStatus::Sent)
                        if !snapshot.packet_acknowledged =>
                    {
                        snapshot.sent_at
                    }
                    (RequestSource::Resource(_), RequestStatus::Delivered) => snapshot.delivered_at,
                    _ => None,
                }?;
                (now.saturating_duration_since(started) > timeout)
                    .then_some((id.clone(), RequestFailure::Timeout))
            })
            .collect::<Vec<_>>();
        for (id, reason) in failures {
            if reason == RequestFailure::Timeout {
                self.pending_failed(&id, reason);
            } else {
                self.finish(&id, Err(reason));
            }
        }
    }
}
