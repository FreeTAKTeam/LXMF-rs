use super::*;

impl RequestBook {
    pub(in crate::destination::link) fn packet_response(
        &mut self,
        link_id: AddressHash,
        request_id: &[u8],
        size: usize,
        data: &[u8],
    ) -> bool {
        if let Some(Record::Live(live)) = self.entries.get(request_id) {
            if live.identity.link_id != link_id {
                return false;
            }
        }
        if !self.allows_response_size(request_id, size as u128) {
            return false;
        }
        if self.entries.contains_key(request_id) {
            self.finish(
                request_id,
                Ok(RequestResponse { data: data.to_vec(), metadata: None, is_resource: false }),
            );
        }
        true
    }

    pub(in crate::destination::link) fn response_resource_allowed(
        &mut self,
        link_id: AddressHash,
        request_id: &[u8],
        uncompressed_size: u64,
    ) -> bool {
        if let Some(Record::Live(live)) = self.entries.get(request_id) {
            if live.identity.link_id != link_id {
                return false;
            }
        }
        self.allows_response_size(request_id, u128::from(uncompressed_size))
    }

    pub(in crate::destination::link) fn register_response_resource(
        &mut self,
        link_id: AddressHash,
        request_id: &[u8],
        original_hash: Hash,
        active_hash: Hash,
        uncompressed_size: u64,
    ) -> bool {
        if !self.response_resource_allowed(link_id, request_id, uncompressed_size) {
            return false;
        }
        if let Some(Record::Live(live)) = self.entries.get_mut(request_id) {
            if live.identity.link_id != link_id {
                return false;
            }
            live.response_hashes.insert(original_hash);
            live.response_hashes.insert(active_hash);
        }
        true
    }

    pub(in crate::destination::link) fn response_progress(
        &mut self,
        link_id: AddressHash,
        hash: Hash,
        received: u64,
        total: u64,
    ) {
        for record in self.entries.values() {
            if let Record::Live(live) = record {
                if live.identity.link_id == link_id && live.response_hashes.contains(&hash) {
                    live.sender.send_if_modified(|snapshot| {
                        if snapshot.status.is_terminal() {
                            return false;
                        }
                        if snapshot.status == RequestStatus::Receiving && received == 0 {
                            return false;
                        }
                        let acknowledged = snapshot.packet_acknowledged
                            || matches!(live.source, RequestSource::Packet(_));
                        let changed = snapshot.status != RequestStatus::Receiving
                            || snapshot.packet_acknowledged != acknowledged
                            || snapshot.received_bytes != received
                            || snapshot.total_bytes != total;
                        snapshot.status = RequestStatus::Receiving;
                        snapshot.packet_acknowledged = acknowledged;
                        snapshot.received_bytes = received;
                        snapshot.total_bytes = total;
                        changed
                    });
                }
            }
        }
    }

    pub(in crate::destination::link) fn resource_response(
        &mut self,
        link_id: AddressHash,
        request_id: &[u8],
        original_hash: Hash,
        data: &[u8],
        metadata: Option<&[u8]>,
    ) -> bool {
        match self.entries.get(request_id) {
            Some(Record::Terminal) => return false,
            Some(Record::Live(live))
                if live.identity.link_id != link_id
                    || !live.response_hashes.contains(&original_hash)
                    || live.sender.borrow().status.is_terminal() =>
            {
                return false
            }
            _ => {}
        }
        // Ordinary responses require their packed envelope's exact ID. Metadata
        // responses are raw files and are bound by the authenticated advertisement.
        if metadata.is_none() {
            match crate::destination::link::unpack_response_envelope(data) {
                Ok((id, _)) if id.as_slice() == request_id => {}
                _ => {
                    log::warn!(
                        "invalid response Resource envelope link={} resource={}",
                        link_id,
                        original_hash
                    );
                    self.finish(request_id, Err(RequestFailure::InvalidResponse));
                    return false;
                }
            }
        }
        if self.entries.contains_key(request_id) {
            self.finish(
                request_id,
                Ok(RequestResponse {
                    data: data.to_vec(),
                    metadata: metadata.map(<[u8]>::to_vec),
                    is_resource: true,
                }),
            );
        }
        true
    }
}
