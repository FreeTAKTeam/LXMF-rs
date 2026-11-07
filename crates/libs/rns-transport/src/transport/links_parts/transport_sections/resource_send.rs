impl Transport {
    pub async fn send_resource(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<Hash, RnsError> {
        self.send_resource_with_compression(link_id, data, metadata, true).await
    }

    /// Send a resource with explicit control over opportunistic compression.
    ///
    /// The default [`Self::send_resource`] path keeps the reference behavior
    /// of trying bzip2 compression when it reduces the transfer. Callers
    /// that need wire-level uncompressed data, such as `rncp --no-compress`,
    /// can disable that choice without changing the resource framing.
    pub async fn send_resource_with_compression(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        auto_compress: bool,
    ) -> Result<Hash, RnsError> {
        self.send_resource_observed_with_compression(link_id, data, metadata, |_| {}, auto_compress)
            .await
    }

    pub async fn send_resource_observed(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash),
    ) -> Result<Hash, RnsError> {
        self.send_resource_observed_with_compression(
            link_id,
            data,
            metadata,
            observe_resource,
            true,
        )
        .await
    }

    pub async fn send_resource_observed_with_compression(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash),
        auto_compress: bool,
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        self.send_resource_observed_inner(
            &link,
            data,
            metadata,
            |_, hash| observe_resource(hash),
            auto_compress,
        )
        .await
    }

    /// Send using the caller's retained Link without selecting it again by ID.
    /// The observer runs before dispatch under the transport handler lock.
    pub(crate) async fn send_resource_on_link_observed(
        &self,
        link: &Arc<Mutex<Link>>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash),
    ) -> Result<Hash, RnsError> {
        self.send_resource_observed_inner(
            link,
            data,
            metadata,
            |_, hash| observe_resource(hash),
            true,
        )
        .await
    }

    /// Registers a retained completion receiver before dispatching any Resource packet.
    /// The observer runs under the transport handler lock and must not re-enter it.
    /// Once invoked, the receiver owns observation even if dispatch returns an error.
    ///
    /// Dropping the receiver does not cancel the transfer. This send operation is
    /// not cancellation-safe after registration: drive it to completion, or drop
    /// this future and explicitly cancel the observed registration before abandoning
    /// its native lifecycle. Do not keep driving a send concurrently with cancellation.
    /// A pending registration may already have queued an advertisement, so cancellation
    /// guarantees local native cleanup, not a cancel-frame handoff to the peer.
    pub async fn send_resource_observed_with_completion(
        &self,
        link_id: &AddressHash,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash, crate::resource::ResourceCompletion),
    ) -> Result<Hash, RnsError> {
        let link = self.find_any_link(link_id).await.ok_or(RnsError::InvalidArgument)?;
        self.send_resource_on_link_observed_with_completion(&link, data, metadata, observe_resource)
            .await
    }

    /// Like [`Self::send_resource_observed_with_completion`], but retains the
    /// supplied Link for preparation and initial dispatch without an ID lookup.
    /// This is not a lifetime lease: the caller must still handle native closure.
    /// Registration, error and cancellation guarantees are the same as that API.
    pub async fn send_resource_on_link_observed_with_completion(
        &self,
        link: &Arc<Mutex<Link>>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(Hash, crate::resource::ResourceCompletion),
    ) -> Result<Hash, RnsError> {
        self.send_resource_observed_inner(
            link,
            data,
            metadata,
            |manager, hash| observe_resource(hash, manager.observe_outbound(hash)),
            true,
        )
        .await
    }

    async fn send_resource_observed_inner(
        &self,
        link: &Arc<Mutex<Link>>,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        observe_resource: impl FnOnce(&mut ResourceManager, Hash),
        auto_compress: bool,
    ) -> Result<Hash, RnsError> {
        let iface = {
            let link_guard = link.lock().await;
            link_guard.ingress_iface()
        };
        let interface_mtu = self.resource_mtu_for_iface(iface).await;
        // Prepared before the handler lock is taken, not under it. Building the
        // advertisement compresses, encrypts and chunks the payload, which on a
        // large send is a long time to hold a mutex every other link, announce
        // and packet on this node also needs.
        let prepared = {
            let link_guard = link.lock().await;
            // See `resource_wire.rs`: the negotiated link MTU, not the local
            // interface alone, is what a fragment has to fit through.
            let interface_mtu = interface_mtu.min(link_guard.link_mtu());
            ResourceManager::prepare_send_with_compression(
                &link_guard,
                data,
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
        let outcome = self.send_link_packet_on_bound_iface(link, packet).await;
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
