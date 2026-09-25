impl InterfaceManager {
    pub fn inherit_runtime_config(&mut self, source: AddressHash, target: AddressHash) -> bool {
        self.inherit_runtime_config_with_ifac(source, target, true)
    }

    pub(crate) fn inherit_runtime_config_with_ifac(
        &mut self,
        source: AddressHash,
        target: AddressHash,
        inherit_ifac: bool,
    ) -> bool {
        let Some(source_iface) = self.ifaces.iter().find(|iface| iface.address == source) else {
            return false;
        };
        let mode = source_iface.mode;
        let gravity = source_iface.gravity;
        let outgoing = source_iface.outgoing;
        let announce_bitrate_bps = source_iface.announce_bitrate_bps;
        let announce_cap_percent = source_iface.announce_cap_percent;
        let mut shared_config = source_iface.shared_config.clone();
        let ifac_default_size_bytes = source_iface.ifac_default_size_bytes;
        let is_shared_instance = source_iface.is_shared_instance;
        let ifac_context = if inherit_ifac {
            let Ok(context) = source_iface.ifac_state.read() else {
                return false;
            };
            context.clone()
        } else {
            shared_config.ifac_size = None;
            shared_config.network_name = None;
            shared_config.passphrase = None;
            None
        };

        let Some(target_iface) = self.ifaces.iter_mut().find(|iface| iface.address == target) else {
            return false;
        };
        let Ok(mut target_context) = target_iface.ifac_state.write() else {
            return false;
        };

        *target_context = ifac_context;
        target_iface.mode = mode;
        target_iface.parent = Some(source);
        target_iface.gravity = gravity;
        target_iface.outgoing = outgoing;
        target_iface.announce_bitrate_bps = announce_bitrate_bps;
        target_iface.announce_cap_percent = announce_cap_percent;
        target_iface.shared_config = shared_config;
        target_iface.ifac_default_size_bytes = ifac_default_size_bytes;
        target_iface.inherit_ifac = inherit_ifac;
        target_iface.is_shared_instance = is_shared_instance;
        true
    }
}
