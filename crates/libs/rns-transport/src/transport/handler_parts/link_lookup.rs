impl TransportHandler {
    pub(super) async fn local_link_for_packet(&self, packet: &Packet) -> Option<Arc<Mutex<Link>>> {
        let mut link = self
            .in_links
            .get(&packet.destination)
            .cloned()
            .or_else(|| self.out_links.get(&packet.destination).cloned());
        if link.is_none() {
            for candidate in self.out_links.values() {
                if *candidate.lock().await.id() == packet.destination {
                    link = Some(candidate.clone());
                    break;
                }
            }
        }
        link
    }
}
