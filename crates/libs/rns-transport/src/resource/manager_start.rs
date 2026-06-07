impl ResourceManager {
    pub fn new() -> Self {
        Self::new_with_config(
            Duration::from_secs(DEFAULT_RESOURCE_RETRY_INTERVAL_SECS),
            DEFAULT_RESOURCE_MAX_RETRIES,
        )
    }

    pub fn new_with_config(retry_interval: Duration, retry_limit: u8) -> Self {
        Self {
            pending_outgoing: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
            events: Vec::new(),
            retry_interval,
            retry_limit,
            link_stats: HashMap::new(),
        }
    }

    pub fn start_send(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
    ) -> Result<(Hash, Packet), RnsError> {
        let sender = ResourceSender::new(link, data, metadata)?;
        self.track_sender(sender)
    }

    pub fn start_send_with_mtu(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        interface_mtu: usize,
    ) -> Result<(Hash, Packet), RnsError> {
        let sender = ResourceSender::new_with_mtu(link, data, metadata, interface_mtu)?;
        self.track_sender(sender)
    }

    pub fn start_send_with_options(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
    ) -> Result<(Hash, Packet), RnsError> {
        let sender = ResourceSender::new_with_options(link, data, metadata, request_id, is_response)?;
        self.track_sender(sender)
    }

    pub fn start_send_with_options_mtu(
        &mut self,
        link: &Link,
        data: Vec<u8>,
        metadata: Option<Vec<u8>>,
        request_id: Option<Vec<u8>>,
        is_response: bool,
        interface_mtu: usize,
    ) -> Result<(Hash, Packet), RnsError> {
        let sender = ResourceSender::new_with_options_mtu(
            link,
            data,
            metadata,
            request_id,
            is_response,
            interface_mtu,
        )?;
        self.track_sender(sender)
    }

    fn track_sender(&mut self, sender: ResourceSender) -> Result<(Hash, Packet), RnsError> {
        let resource_hash = sender.resource_hash;
        let packet = sender.advertisement_packet();
        self.pending_outgoing.insert(resource_hash, sender);
        Ok((resource_hash, packet))
    }
}
