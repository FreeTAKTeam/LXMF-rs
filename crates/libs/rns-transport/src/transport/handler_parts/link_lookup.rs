impl TransportHandler {
    pub(super) async fn can_relay_link(&self, link_id: &LinkId, received_on: AddressHash) -> bool {
        if self.config.transport_enabled {
            return true;
        }
        let manager = self.iface_manager.lock().await;
        manager.is_local_client_interface(&received_on)
            || self.link_table.relay_interfaces(link_id).is_some_and(|(from, next)| {
                manager.is_local_client_interface(&from) || manager.is_local_client_interface(&next)
            })
    }

    pub(super) async fn local_link_for_packet(&self, packet: &Packet) -> Option<Arc<Mutex<Link>>> {
        self.local_link_for_id(&packet.destination).await
    }

    pub(super) async fn local_link_for_id(
        &self,
        link_id: &AddressHash,
    ) -> Option<Arc<Mutex<Link>>> {
        let mut link =
            self.in_links.get(link_id).cloned().or_else(|| self.out_links.get(link_id).cloned());
        if link.is_none() {
            for candidate in self.out_links.values() {
                if *candidate.lock().await.id() == *link_id {
                    link = Some(candidate.clone());
                    break;
                }
            }
        }
        link
    }
}
