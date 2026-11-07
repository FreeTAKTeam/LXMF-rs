use super::*;

mod lifecycle;
mod responses;

#[derive(Default)]
pub(in crate::destination::link) struct RequestBook {
    entries: HashMap<Vec<u8>, Record>,
    cleanup: Vec<RequestCleanup>,
}

impl RequestBook {
    pub(in crate::destination::link) fn register(
        &mut self,
        link_id: AddressHash,
        request_id: Vec<u8>,
        source: RequestSource,
        timeout: Option<Duration>,
        limit: Option<usize>,
    ) -> Result<(RequestReceipt, RequestDispatchGuard), RnsError> {
        if request_id.len() != crate::hash::ADDRESS_HASH_SIZE
            || matches!(source, RequestSource::Legacy)
            || timeout.is_some_and(|timeout| Instant::now().checked_add(timeout).is_none())
        {
            return Err(RnsError::InvalidArgument);
        }
        // An existing policy-only entry can become owned, preserving an explicit
        // legacy limit when the new call supplies none. A known owned/terminal
        // registration is never replaced, even for byte-identical request IDs.
        let old_limit = match self.entries.get(&request_id) {
            None => None,
            Some(Record::Live(live))
                if matches!(live.source, RequestSource::Legacy)
                    && live.identity.link_id == link_id
                    && live.sender.borrow().status == RequestStatus::Sent =>
            {
                live.limit
            }
            _ => return Err(RnsError::InvalidArgument),
        };
        let identity = Arc::new(RequestIdentity { link_id, request_id: request_id.clone() });
        let (sender, receiver) = watch::channel(RequestSnapshot::sent());
        let receipt = RequestReceipt { identity: identity.clone(), receiver };
        let guard = RequestDispatchGuard {
            identity: identity.clone(),
            sender: sender.clone(),
            returned: false,
        };
        self.entries.insert(
            request_id,
            Record::Live(LiveRequest {
                identity,
                source,
                timeout,
                limit: limit.or(old_limit),
                response_hashes: HashSet::new(),
                sender,
            }),
        );
        Ok((receipt, guard))
    }

    pub(in crate::destination::link) fn matches_dispatch(
        &self,
        link_id: AddressHash,
        dispatch: &RequestDispatchGuard,
    ) -> bool {
        matches!(self.entries.get(&dispatch.identity.request_id), Some(Record::Live(live))
            if live.identity.link_id == link_id
                && Arc::ptr_eq(&live.identity, &dispatch.identity)
                && !live.sender.borrow().status.is_terminal())
    }

    pub(in crate::destination::link) fn cancel(
        &mut self,
        link_id: AddressHash,
        receipt: &RequestReceipt,
    ) -> bool {
        if !matches!(self.entries.get(receipt.request_id()), Some(Record::Live(live))
            if live.identity.link_id == link_id && Arc::ptr_eq(&live.identity, &receipt.identity))
        {
            return false;
        }
        // A dropped send can already have sealed its watch while this native
        // registration still owns Resource cleanup. Retire it exactly once.
        self.finish(receipt.request_id(), Err(RequestFailure::Cancelled));
        true
    }

    pub(in crate::destination::link) fn set_legacy_limit(
        &mut self,
        link_id: AddressHash,
        request_id: &[u8],
        limit: usize,
    ) {
        match self.entries.entry(request_id.to_vec()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                let (sender, _) = watch::channel(RequestSnapshot::sent());
                entry.insert(Record::Live(LiveRequest {
                    identity: Arc::new(RequestIdentity {
                        link_id,
                        request_id: request_id.to_vec(),
                    }),
                    source: RequestSource::Legacy,
                    timeout: None,
                    limit: Some(limit),
                    response_hashes: HashSet::new(),
                    sender,
                }));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if let Record::Live(live) = entry.get_mut() {
                    if matches!(live.source, RequestSource::Legacy)
                        && !live.sender.borrow().status.is_terminal()
                    {
                        live.limit = Some(limit);
                    }
                }
            }
        }
    }

    pub(in crate::destination::link) fn clear_legacy_limit(&mut self, request_id: &[u8]) {
        if let Some(Record::Live(live)) = self.entries.get_mut(request_id) {
            if matches!(live.source, RequestSource::Legacy)
                && !live.sender.borrow().status.is_terminal()
            {
                live.limit = None;
            }
        }
    }

    pub(in crate::destination::link) fn allows_response_size(
        &mut self,
        request_id: &[u8],
        size: u128,
    ) -> bool {
        let Some(record) = self.entries.get(request_id) else {
            // Compatibility for stateless legacy request_packet(&self) users.
            // Unknown traffic is never authority to complete an owned receipt.
            return true;
        };
        let Record::Live(live) = record else {
            return false;
        };
        let status = live.sender.borrow().status;
        if status.is_terminal() {
            return false;
        }
        if live.limit.is_some_and(|limit| size > limit as u128) {
            self.pending_failed(request_id, RequestFailure::Rejected);
            return false;
        }
        true
    }

    fn finish(
        &mut self,
        request_id: &[u8],
        outcome: Result<RequestResponse, RequestFailure>,
    ) -> bool {
        let Some(record) = self.entries.get_mut(request_id) else {
            return false;
        };
        let Record::Live(live) = std::mem::replace(record, Record::Terminal) else {
            return false;
        };
        let changed = live.sender.send_if_modified(|snapshot| {
            if snapshot.status.is_terminal() {
                return false;
            }
            match outcome {
                Ok(response) => {
                    snapshot.status = RequestStatus::Ready;
                    snapshot.packet_acknowledged |= matches!(live.source, RequestSource::Packet(_));
                    snapshot.response = Some(Arc::new(response));
                }
                Err(failure) => {
                    snapshot.status = RequestStatus::Failed;
                    snapshot.failure = Some(failure);
                }
            }
            true
        });
        let outbound = match live.source {
            RequestSource::Resource(hash) => Some(hash),
            _ => None,
        };
        if outbound.is_some() || !live.response_hashes.is_empty() {
            self.cleanup.push(RequestCleanup {
                link_id: live.identity.link_id,
                request_id: live.identity.request_id.clone(),
                outbound,
                inbound: live.response_hashes,
            });
        }
        changed
    }

    pub(in crate::destination::link) fn reset(&mut self, reason: RequestFailure) {
        let ids = self.entries.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.finish(&id, Err(reason));
        }
        self.entries.clear();
    }

    pub(in crate::destination::link) fn drain_cleanup(&mut self) -> Vec<RequestCleanup> {
        std::mem::take(&mut self.cleanup)
    }

    #[cfg(test)]
    pub(in crate::destination::link) fn pending_count(&self) -> usize {
        self.entries
            .values()
            .filter(|record| {
                matches!(record, Record::Live(live)
            if !live.sender.borrow().status.is_terminal())
            })
            .count()
    }
}
