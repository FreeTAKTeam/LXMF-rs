impl TransportHandler {
    fn reject_oversized_request(
        &self,
        packet: &Packet,
        destination: &SingleInputDestination,
        payload_len: usize,
    ) -> bool {
        let Some(limit) = destination.max_request_size().filter(|limit| payload_len > *limit)
        else {
            return false;
        };
        log::warn!(
            "tp({}): rejecting oversized request destination={} size={} limit={limit}",
            self.config.name,
            packet.destination,
            payload_len,
        );
        true
    }
}
