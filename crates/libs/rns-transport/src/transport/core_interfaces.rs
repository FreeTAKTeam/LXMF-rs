use super::*;

impl Transport {
    /// Spawn a multicast UDP interface with per-peer routing.
    ///
    /// Announces and path requests broadcast to the multicast group
    /// exactly as before. Point-to-point `Direct` tx for a peer
    /// discovered via this iface is delivered through the same host
    /// socket, unicast — the tx task resolves the virtual iface hash
    /// to a `SocketAddr` via the `PeerRouting` map this method
    /// registers with the transport handler.
    ///
    /// Callers (backend_lxmf.rs) should use this rather than spawning
    /// a `UdpInterface` with `is_multicast=true` directly; going
    /// through this helper ensures the transport handler knows about
    /// the routing map so `unicast_iface_for_source` can register
    /// peers into it.
    pub async fn add_multicast_udp_interface(
        &self,
        bind_addr: String,
        forward_addr: Option<String>,
    ) -> AddressHash {
        self.add_multicast_udp_interface_with_status(bind_addr, forward_addr).await.0
    }

    pub async fn add_multicast_udp_interface_with_status(
        &self,
        bind_addr: String,
        forward_addr: Option<String>,
    ) -> (AddressHash, crate::iface::udp::UdpRuntimeStatusHandle) {
        self.try_add_multicast_udp_interface_with_status(
            bind_addr,
            forward_addr,
            crate::iface::InterfaceMode::default(),
            |_, _| Ok::<(), std::convert::Infallible>(()),
        )
        .await
        .unwrap_or_else(|never| match never {})
    }

    pub async fn try_add_multicast_udp_interface_with_status<P, E>(
        &self,
        bind_addr: String,
        forward_addr: Option<String>,
        mode: crate::iface::InterfaceMode,
        prepare: P,
    ) -> Result<(AddressHash, crate::iface::udp::UdpRuntimeStatusHandle), E>
    where
        P: FnOnce(&mut crate::iface::InterfaceManager, AddressHash) -> Result<(), E>,
    {
        let (iface_hash, peer_routing, status) = {
            let mut mgr = self.iface_manager.lock().await;
            let (iface_hash, peer_routing, status) =
                crate::iface::udp::try_spawn_multicast_udp_with_status(
                    &mut mgr,
                    bind_addr,
                    forward_addr,
                    mode,
                    prepare,
                )?;
            (iface_hash, peer_routing, status)
        };
        self.handler.lock().await.register_multicast_peer_routing(iface_hash, peer_routing);
        Ok((iface_hash, status))
    }
}
