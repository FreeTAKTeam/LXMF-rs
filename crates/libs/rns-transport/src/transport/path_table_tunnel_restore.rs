pub(super) struct TunnelPathRestore {
    pub destination: AddressHash,
    pub received_from: AddressHash,
    pub hops: u8,
    pub iface: AddressHash,
    pub packet_hash: Hash,
    pub random_blobs: Vec<RandomBlob>,
    pub existing_mode: Option<InterfaceMode>,
    pub now: Instant,
}

impl PathTable {
    pub(super) fn restore_tunnel_path_with_random_blobs(
        &mut self,
        restore: TunnelPathRestore,
    ) -> bool {
        let TunnelPathRestore {
            destination,
            received_from,
            hops,
            iface,
            packet_hash,
            random_blobs,
            existing_mode,
            now,
        } = restore;
        let random_blobs = bounded_random_blobs(random_blobs);
        if let Some(existing) = self.map.get(&destination) {
            let mode = existing_mode.unwrap_or(InterfaceMode::Full);
            let existing_expired = path_expired_for_mode(existing, now, mode);
            if hops > existing.hops && !existing_expired {
                return false;
            }
            if newest_random_blob_timebase(&random_blobs)
                < newest_random_blob_timebase(&existing.random_blobs)
            {
                return false;
            }
        }

        self.map.insert(
            destination,
            PathEntry {
                timestamp: now,
                received_from,
                hops,
                iface,
                packet_hash,
                random_blobs,
                state: PathState::Unknown,
            },
        );
        true
    }
}
