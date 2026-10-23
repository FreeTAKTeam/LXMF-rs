impl Transport {
    pub async fn cancel_resource(
        &self,
        link_id: &AddressHash,
        resource_hash: Hash,
    ) -> Result<bool, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        self.cancel_resource_on_link(&link, resource_hash).await
    }

    /// Cancel the logical Resource on the supplied Link without another lookup.
    /// A mismatched Link returns InvalidArgument without changing Resource state.
    /// The boolean reports cancel-frame handoff, not native cancellation: a pending
    /// Resource needs no frame, and handoff failure does not undo a Cancelled outcome.
    /// Drive registered dispatch to completion, or drop that send future before
    /// explicitly cancelling its registration here. Do not drive it concurrently
    /// with cancellation. A pending registration may already have queued its
    /// advertisement; cancellation then stops native work without a cancel frame.
    pub async fn cancel_resource_on_link(
        &self,
        link: &Arc<Mutex<Link>>,
        resource_hash: Hash,
    ) -> Result<bool, RnsError> {
        let (iface, packet) = loop {
            let mut handler = self.handler.lock().await;
            // Match the handler -> Link order without waiting on a busy Link
            // while unrelated transport work holds behind the handler.
            let mut link_guard = match link.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    drop(handler);
                    drop(link.lock().await);
                    continue;
                }
            };
            let packet = handler.resource_manager.cancel_outgoing(resource_hash, &link_guard)?;
            let iface = link_guard.ingress_iface();
            super::resource_wire::publish_resource_events_for_link(
                &mut handler,
                *link_guard.id(),
                Some(&mut link_guard),
            );
            break (iface, packet);
        };
        self.dispatch_link_lifecycle_packet(iface, packet).await
    }

    /// Close the current retained Link and fail its native Resources atomically.
    /// Only pointer-equal map entries are removed, preserving replacement Links.
    /// Captured frames use the original interface after releasing the Link lock.
    /// The result reports frame handoff; native closure/Resource failure has already
    /// occurred even when handoff fails. This does not lease a Link against a
    /// public restart before or after the atomic close/removal section.
    pub async fn teardown_link(&self, link: &Arc<Mutex<Link>>) -> Result<bool, RnsError> {
        self.teardown_link_observed(link, || {}).await
    }

    /// Observe local closure and native Resource cleanup before optional frame
    /// dispatch. After observation, dropping the future can omit that frame but
    /// cannot undo local cleanup. The observer must not re-enter Transport.
    pub async fn teardown_link_observed(
        &self,
        link: &Arc<Mutex<Link>>,
        locally_closed: impl FnOnce(),
    ) -> Result<bool, RnsError> {
        let mut locally_closed = Some(locally_closed);
        let (iface, packet) = loop {
            let mut handler = self.handler.lock().await;
            let mut link_guard = match link.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    drop(handler);
                    drop(link.lock().await);
                    continue;
                }
            };
            let link_id = *link_guard.id();
            let iface = link_guard.ingress_iface();
            super::resource_wire::publish_resource_events_for_link(
                &mut handler,
                link_id,
                Some(&mut link_guard),
            );
            let packet = link_guard.try_teardown();
            super::requests::retire_closed_link(&mut handler, &mut link_guard);
            handler.out_links.retain(|_, current| !Arc::ptr_eq(current, link));
            handler.in_links.retain(|_, current| !Arc::ptr_eq(current, link));
            handler.resource_manager.remove_link_state(link_id);
            super::resource_wire::publish_resource_events_for_link(
                &mut handler,
                link_id,
                Some(&mut link_guard),
            );
            locally_closed.take().expect("local close observed once")();
            break (iface, packet?);
        };
        self.dispatch_link_lifecycle_packet(iface, packet).await
    }

    async fn dispatch_link_lifecycle_packet(
        &self,
        iface: Option<AddressHash>,
        packet: Option<Packet>,
    ) -> Result<bool, RnsError> {
        let Some(packet) = packet else {
            return Ok(false);
        };
        let iface = iface.ok_or(RnsError::ConnectionError)?;
        let dispatch = self
            .handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet })
            .await;
        if dispatch.sent_ifaces > 0 {
            Ok(true)
        } else {
            Err(RnsError::ConnectionError)
        }
    }
}
