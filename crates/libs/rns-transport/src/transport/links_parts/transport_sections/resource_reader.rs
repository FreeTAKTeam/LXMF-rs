impl Transport {
    /// Send a resource from a synchronous reader. Split resources retain the
    /// reader and read one segment only after the preceding segment is proved,
    /// so callers do not need to materialize a large file in memory first.
    pub async fn send_resource_from_reader<R: Read + Send + Sync + 'static>(
        &self,
        link_id: &AddressHash,
        reader: R,
        data_size: u64,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        self.send_resource_from_reader_observed(link_id, reader, data_size, metadata, |_| {})
            .await
    }

    pub async fn send_resource_from_reader_observed<R: Read + Send + Sync + 'static>(
        &self,
        link_id: &AddressHash,
        reader: R,
        data_size: u64,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash),
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        let iface = {
            let link_guard = link.lock().await;
            link_guard.ingress_iface()
        };
        let interface_mtu = self.resource_mtu_for_iface(iface).await;
        let prepared = {
            let link_guard = link.lock().await;
            let interface_mtu = interface_mtu.min(link_guard.link_mtu());
            ResourceManager::prepare_send_from_reader(
                &link_guard,
                reader,
                data_size,
                metadata,
                None,
                false,
                interface_mtu,
                true,
            )?
        };
        let mut handler = self.handler.lock().await;
        let (resource_hash, packet) = handler.resource_manager.track_prepared(prepared);
        observe_resource(resource_hash);
        drop(handler);
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        let mut handler = self.handler.lock().await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        handler.resource_manager.confirm_outbound_dispatch(resource_hash, sent);
        let events = handler.resource_manager.drain_events();
        super::resource_wire::publish_resource_events(&handler, events);
        if sent {
            Ok(resource_hash)
        } else {
            Err(RnsError::ConnectionError)
        }
    }
}
