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
        self.send_resource_from_reader_observed(link_id, reader, data_size, metadata, |_| {}).await
    }

    pub async fn send_resource_from_reader_observed<R: Read + Send + Sync + 'static>(
        &self,
        link_id: &AddressHash,
        reader: R,
        data_size: u64,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash),
    ) -> Result<Hash, RnsError> {
        self.send_resource_from_reader_inner(
            link_id,
            reader,
            data_size,
            metadata,
            true,
            |_, hash| observe_resource(hash),
        )
        .await
    }

    /// Streams a Resource from `reader` with explicit compression control and a
    /// retained completion receiver registered before the first packet is sent.
    ///
    /// Only the current segment (at most ~1 MiB) is read into memory, so large
    /// files are sent with bounded memory. `auto_compress: false` keeps the
    /// wire representation identical to the logical bytes, as RPP requires.
    /// Registration, error and cancellation guarantees match
    /// [`Self::send_resource_observed_with_completion`].
    pub async fn send_resource_from_reader_with_completion<R: Read + Send + Sync + 'static>(
        &self,
        link_id: &AddressHash,
        reader: R,
        data_size: u64,
        metadata: Option<Vec<u8>>,
        auto_compress: bool,
        observe_resource: impl FnOnce(Hash, crate::resource::ResourceCompletion),
    ) -> Result<Hash, RnsError> {
        self.send_resource_from_reader_inner(
            link_id,
            reader,
            data_size,
            metadata,
            auto_compress,
            |manager, hash| observe_resource(hash, manager.observe_outbound(hash)),
        )
        .await
    }

    /// Sets (or with `None` clears) the inbound Resource budget for one Link.
    /// Advertisements above it are refused with a receiver cancel before any
    /// receiver state is allocated. The budget is dropped with the Link.
    pub async fn set_link_resource_admission(
        &self,
        link_id: &AddressHash,
        admission: Option<crate::resource::LinkResourceAdmission>,
    ) {
        self.handler.lock().await.resource_manager.set_link_admission(*link_id, admission);
    }

    async fn send_resource_from_reader_inner<R: Read + Send + Sync + 'static>(
        &self,
        link_id: &AddressHash,
        reader: R,
        data_size: u64,
        metadata: Option<Vec<u8>>,
        auto_compress: bool,
        observe_resource: impl FnOnce(&mut ResourceManager, Hash),
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
                auto_compress,
            )?
        };
        let mut handler = self.handler.lock().await;
        let (resource_hash, packet) = handler.resource_manager.track_prepared(prepared);
        observe_resource(&mut handler.resource_manager, resource_hash);
        drop(handler);
        let outcome = self.send_link_packet_on_bound_iface(&link, packet).await;
        let mut handler = self.handler.lock().await;
        let sent =
            matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast);
        handler.resource_manager.confirm_outbound_dispatch(resource_hash, sent);
        super::resource_wire::publish_resource_events(&mut handler).await;
        if sent {
            Ok(resource_hash)
        } else {
            Err(RnsError::ConnectionError)
        }
    }
}
