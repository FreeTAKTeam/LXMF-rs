impl Link {
    pub(crate) fn register_request(
        &mut self,
        request_id: Vec<u8>,
        source: RequestSource,
        timeout: Option<Duration>,
        limit: Option<usize>,
    ) -> Result<(RequestReceipt, RequestDispatchGuard), RnsError> {
        if self.status != LinkStatus::Active {
            return Err(RnsError::ConnectionError);
        }
        self.requests.register(self.id, request_id, source, timeout, limit)
    }

    pub(crate) fn request_dispatch_matches(&self, dispatch: &RequestDispatchGuard) -> bool {
        self.status == LinkStatus::Active && self.requests.matches_dispatch(self.id, dispatch)
    }

    pub(crate) fn cancel_request(&mut self, receipt: &RequestReceipt) -> bool {
        self.requests.cancel(self.id, receipt)
    }

    pub(crate) fn poll_requests(&mut self, now: Instant) -> Vec<RequestCleanup> {
        self.requests.poll(now);
        self.requests.drain_cleanup()
    }

    pub(crate) fn response_resource_allowed(&mut self, request_id: &[u8], size: u64) -> bool {
        self.requests.response_resource_allowed(self.id, request_id, size)
    }

    pub(crate) fn register_response_resource(
        &mut self,
        request_id: &[u8],
        original: Hash,
        active: Hash,
        size: u64,
    ) -> bool {
        self.requests.register_response_resource(self.id, request_id, original, active, size)
    }

    pub(crate) fn start_response_resource(&mut self, hash: Hash, total: u64) {
        self.requests.response_progress(self.id, hash, 0, total);
    }

    /// Apply native state before the legacy broadcast, which can be lost. The
    /// returned false suppresses a completed response rejected by this owner.
    pub(crate) fn request_resource_event(
        &mut self,
        event: &crate::resource::ResourceEvent,
    ) -> bool {
        use crate::resource::ResourceEventKind;
        if event.link_id != self.id {
            return true;
        }
        match &event.kind {
            ResourceEventKind::OutboundComplete => {
                self.requests.request_resource_delivered(self.id, event.hash, Instant::now())
            }
            ResourceEventKind::OutboundFailed | ResourceEventKind::InboundFailed(_) => {
                self.requests.resource_failed(self.id, event.hash, RequestFailure::TransferFailed)
            }
            ResourceEventKind::OutboundRejected => {
                self.requests.resource_failed(self.id, event.hash, RequestFailure::Rejected)
            }
            ResourceEventKind::OutboundCancelled => {
                self.requests.resource_failed(self.id, event.hash, RequestFailure::Cancelled)
            }
            ResourceEventKind::Progress(progress) => self.requests.response_progress(
                self.id,
                event.hash,
                progress.received_bytes,
                progress.total_bytes,
            ),
            ResourceEventKind::Complete(complete) if complete.is_response => {
                let Some(id) = complete.request_id.as_deref() else {
                    log::warn!(
                        "link({}): response Resource has no request ID resource={}",
                        self.id,
                        event.hash
                    );
                    return false;
                };
                return self.requests.resource_response(
                    self.id,
                    id,
                    event.hash,
                    &complete.data,
                    complete.metadata.as_deref(),
                );
            }
            _ => {}
        }
        true
    }
}
