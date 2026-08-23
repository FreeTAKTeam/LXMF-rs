impl InterfaceManager {
    pub fn receiver(&self) -> Arc<tokio::sync::Mutex<InterfaceRxReceiver>> {
        self.rx_recv.clone()
    }

    pub fn cleanup(&mut self) {
        self.ifaces.retain(|iface| !iface.stop.is_cancelled());
    }

    pub fn stop_interface(&mut self, address: AddressHash) -> bool {
        let mut stopped = false;
        for iface in &self.ifaces {
            if iface.address == address {
                iface.stop.cancel();
                stopped = true;
            }
        }
        self.cleanup();
        stopped
    }

    pub fn lowest_interface_bitrate(&self) -> Option<u64> {
        self.ifaces
            .iter()
            .filter(|iface| {
                !iface.stop.is_cancelled() && iface.online.load(Ordering::Acquire)
            })
            .map(|iface| iface.announce_bitrate_bps)
            .filter(|bitrate| *bitrate > 0)
            .min()
    }

    /// Drops every pending announce and returns the number removed.
    pub fn drop_announce_queues(&mut self) -> usize {
        self.ifaces
            .iter_mut()
            .map(|iface| {
                let dropped = iface.announce_queue.len();
                iface.announce_queue.clear();
                dropped
            })
            .sum()
    }

    pub fn policy(&self, address: &AddressHash) -> Option<InterfacePolicy> {
        self.ifaces.iter().find(|i| i.address == *address).map(|iface| InterfacePolicy {
            mode: iface.mode,
            gravity: iface.gravity,
        })
    }

    pub fn interface_hashes(&self) -> std::collections::HashSet<AddressHash> {
        self.ifaces.iter().map(|iface| iface.address).collect()
    }

    pub fn is_shared_instance(&self, address: &AddressHash) -> bool {
        self.ifaces
            .iter()
            .find(|iface| iface.address == *address)
            .is_some_and(|iface| iface.is_shared_instance)
    }

    pub fn gravity(&self, address: &AddressHash) -> Option<i64> {
        self.ifaces.iter().find(|i| i.address == *address).map(|i| i.gravity)
    }

    pub fn set_gravity(&mut self, address: AddressHash, gravity: i64) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|i| i.address == address) {
            iface.gravity = gravity;
            true
        } else {
            false
        }
    }

    pub fn set_shared_instance(&mut self, address: AddressHash, enabled: bool) -> bool {
        if let Some(iface) = self.ifaces.iter_mut().find(|iface| iface.address == address) {
            iface.is_shared_instance = enabled;
            true
        } else {
            false
        }
    }

    pub fn detach_interfaces(&mut self) -> usize {
        let detached = self.ifaces.len();
        for interface in &self.ifaces {
            interface.stop.cancel();
        }
        self.cleanup();
        detached
    }

    pub fn prioritize_interfaces(&mut self) {
        self.ifaces.sort_by(|left, right| {
            right.announce_bitrate_bps.cmp(&left.announce_bitrate_bps)
        });
    }
}
