impl AutoDiscoveryState {
    pub fn new(
        adopted_devices: Vec<AutoInterfaceAdoptedDevice>,
        peering_timeout: core::time::Duration,
        reverse_peering_interval: core::time::Duration,
    ) -> Self {
        Self {
            adopted_devices: adopted_devices
                .into_iter()
                .map(|device| (device.ifname, descope_link_local(&device.link_local_address)))
                .collect(),
            peers: AutoPeerTable::new(peering_timeout, reverse_peering_interval),
            multicast_echoes: BTreeMap::new(),
            initial_echoes: BTreeMap::new(),
            last_multicast_announces: BTreeMap::new(),
            timed_out_interfaces: BTreeMap::new(),
        }
    }

    pub fn from_timing(
        adopted_devices: Vec<AutoInterfaceAdoptedDevice>,
        timing: AutoInterfaceTiming,
    ) -> Self {
        Self::new(adopted_devices, timing.peering_timeout, timing.reverse_peering_interval)
    }

    pub fn observe_discovery_packet(
        &mut self,
        source_address: &str,
        ifname: &str,
        now: core::time::Duration,
    ) -> AutoDiscoveryEvent {
        let source_address = descope_link_local(source_address);
        if let Some((echo_ifname, _)) = self
            .adopted_devices
            .iter()
            .find(|(_, link_local_address)| **link_local_address == source_address)
        {
            self.multicast_echoes.insert(echo_ifname.clone(), now);
            self.initial_echoes.entry(echo_ifname.clone()).or_insert(now);
            return AutoDiscoveryEvent::LocalMulticastEcho { ifname: echo_ifname.clone() };
        }

        AutoDiscoveryEvent::Peer(self.peers.observe_peer(&source_address, ifname, now))
    }

    pub fn observe_authenticated_discovery_packet(
        &mut self,
        packet: &[u8],
        group_id: &[u8],
        source_address: &str,
        ifname: &str,
        now: core::time::Duration,
    ) -> Result<AutoDiscoveryEvent, AutoDiscoveryRejectReason> {
        if !verify_peering_token(packet, group_id, source_address) {
            return Err(AutoDiscoveryRejectReason::InvalidToken);
        }
        Ok(self.observe_discovery_packet(source_address, ifname, now))
    }

    pub fn peer(&self, address: &str) -> Option<&AutoPeer> {
        self.peers.peer(address)
    }

    pub fn handle_spawned_peer_inbound(
        &mut self,
        dedupe: &mut AutoInboundPacketDeduplicator,
        peer_address: &str,
        packet: &[u8],
        now: core::time::Duration,
    ) -> AutoPeerInboundDecision {
        if self.peer(peer_address).is_none() {
            return AutoPeerInboundDecision::UnknownPeer;
        }
        if !dedupe.should_accept(packet, now) {
            return AutoPeerInboundDecision::Duplicate;
        }
        let peer = self
            .peers
            .refresh_peer(peer_address, now)
            .expect("known peer should refresh after dedupe accept");
        AutoPeerInboundDecision::Accepted { peer }
    }

    pub fn adopted_devices(&self) -> Vec<AutoInterfaceAdoptedDevice> {
        self.adopted_devices
            .iter()
            .map(|(ifname, link_local_address)| AutoInterfaceAdoptedDevice {
                ifname: ifname.clone(),
                link_local_address: link_local_address.clone(),
            })
            .collect()
    }

    pub fn plan_adopted_link_local_address_update(
        &self,
        config: &AutoInterfaceConfig,
        ifname: &str,
        link_local_address: &str,
    ) -> Option<AutoLinkLocalAddressUpdate> {
        let new_link_local_address = descope_link_local(link_local_address);
        let old_link_local_address = self.adopted_devices.get(ifname)?.clone();
        if old_link_local_address == new_link_local_address {
            return None;
        }

        let adopted = AutoInterfaceAdoptedDevice {
            ifname: ifname.to_string(),
            link_local_address: new_link_local_address.clone(),
        };

        Some(AutoLinkLocalAddressUpdate {
            ifname: ifname.to_string(),
            old_link_local_address,
            new_link_local_address,
            listener_binding: config.data_listener_binding(&adopted),
        })
    }

    pub fn plan_adopted_interface_changes(
        &self,
        config: &AutoInterfaceConfig,
        platform: AutoInterfacePlatform,
        desired_adopted: &[AutoInterfaceAdoptedDevice],
    ) -> Vec<AutoAdoptedInterfaceChange> {
        let desired_by_ifname = desired_adopted
            .iter()
            .map(|adopted| {
                (
                    adopted.ifname.clone(),
                    AutoInterfaceAdoptedDevice {
                        ifname: adopted.ifname.clone(),
                        link_local_address: descope_link_local(&adopted.link_local_address),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut changes = Vec::new();
        for (ifname, old_link_local_address) in &self.adopted_devices {
            if !desired_by_ifname.contains_key(ifname) {
                let adopted = AutoInterfaceAdoptedDevice {
                    ifname: ifname.clone(),
                    link_local_address: old_link_local_address.clone(),
                };
                changes.push(AutoAdoptedInterfaceChange::Removed {
                    discovery_listener: config.discovery_listener_binding(&adopted, platform),
                    data_listener: config.data_listener_binding(&adopted),
                    removed_peers: self.peers.peers_by_ifname(ifname),
                    adopted,
                });
            }
        }
        for adopted in desired_by_ifname.values() {
            match self.adopted_devices.get(&adopted.ifname) {
                None => changes.push(AutoAdoptedInterfaceChange::Added {
                    adopted: adopted.clone(),
                    discovery_listener: config.discovery_listener_binding(adopted, platform),
                    data_listener: config.data_listener_binding(adopted),
                }),
                Some(existing) if existing != &adopted.link_local_address => {
                    let update = self
                        .plan_adopted_link_local_address_update(
                            config,
                            &adopted.ifname,
                            &adopted.link_local_address,
                        )
                        .expect("known adopted interface has a planned link-local update");
                    changes.push(AutoAdoptedInterfaceChange::LinkLocalChanged(update));
                }
                Some(_) => {}
            }
        }
        changes
    }

    pub fn apply_adopted_link_local_address_update(&mut self, update: &AutoLinkLocalAddressUpdate) {
        self.adopted_devices
            .insert(update.ifname.clone(), update.new_link_local_address.clone());
        self.clear_interface_runtime_state(&update.ifname);
    }

    #[deprecated(
        note = "use apply_adopted_interface_change_at so runtime additions seed multicast freshness with the current runtime time"
    )]
    pub fn apply_adopted_interface_change(&mut self, change: &AutoAdoptedInterfaceChange) {
        self.apply_adopted_interface_change_at(change, core::time::Duration::ZERO);
    }

    pub fn apply_adopted_interface_change_at(
        &mut self,
        change: &AutoAdoptedInterfaceChange,
        now: core::time::Duration,
    ) {
        match change {
            AutoAdoptedInterfaceChange::Added { adopted, .. } => {
                self.adopted_devices
                    .insert(adopted.ifname.clone(), descope_link_local(&adopted.link_local_address));
                self.multicast_echoes.insert(adopted.ifname.clone(), now);
            }
            AutoAdoptedInterfaceChange::Removed { adopted, .. } => {
                self.adopted_devices.remove(&adopted.ifname);
                self.clear_interface_runtime_state(&adopted.ifname);
                self.peers.remove_by_ifname(&adopted.ifname);
            }
            AutoAdoptedInterfaceChange::LinkLocalChanged(update) => {
                self.apply_adopted_link_local_address_update(update);
            }
        }
    }

    fn clear_interface_runtime_state(&mut self, ifname: &str) {
        self.multicast_echoes.remove(ifname);
        self.initial_echoes.remove(ifname);
        self.last_multicast_announces.remove(ifname);
        self.timed_out_interfaces.remove(ifname);
    }

    pub fn update_adopted_link_local_address(
        &mut self,
        config: &AutoInterfaceConfig,
        ifname: &str,
        link_local_address: &str,
    ) -> Option<AutoLinkLocalAddressUpdate> {
        let update =
            self.plan_adopted_link_local_address_update(config, ifname, link_local_address)?;
        self.apply_adopted_link_local_address_update(&update);
        Some(update)
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn last_multicast_echo(&self, ifname: &str) -> Option<core::time::Duration> {
        self.multicast_echoes.get(ifname).copied()
    }

    pub fn initial_multicast_echo(&self, ifname: &str) -> Option<core::time::Duration> {
        self.initial_echoes.get(ifname).copied()
    }

    pub fn missing_initial_multicast_echoes(&self) -> Vec<String> {
        self.adopted_devices
            .keys()
            .filter(|ifname| !self.initial_echoes.contains_key(*ifname))
            .cloned()
            .collect()
    }

    pub fn peer_job_plan(
        &self,
        config: &AutoInterfaceConfig,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
        now: core::time::Duration,
    ) -> AutoPeerJobPlan {
        let expired_peers = self.peers.stale_peers(now);
        let reverse_peering_packets = self
            .peers
            .reverse_announces_due(now)
            .into_iter()
            .filter(|peer| !expired_peers.iter().any(|expired| expired.address == peer.address))
            .filter_map(|peer| {
                let adopted =
                    adopted_devices.iter().find(|adopted| adopted.ifname == peer.ifname)?;
                Some(config.reverse_peering_packet(adopted, &peer.address))
            })
            .collect();

        AutoPeerJobPlan {
            expired_peers,
            reverse_peering_packets,
            missing_initial_echo_interfaces: self.missing_initial_multicast_echoes(),
        }
    }

    pub fn run_peer_job(
        &mut self,
        config: &AutoInterfaceConfig,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
        now: core::time::Duration,
        multicast_echo_timeout: core::time::Duration,
    ) -> AutoPeerJobRun {
        let expired_peers = self.expire_stale_peers(now);
        let reverse_peering_packets = self
            .peers
            .reverse_announces_due(now)
            .into_iter()
            .filter_map(|peer| {
                let adopted =
                    adopted_devices.iter().find(|adopted| adopted.ifname == peer.ifname)?;
                let packet = config.reverse_peering_packet(adopted, &peer.address);
                self.peers.mark_reverse_announced(&peer.address, now);
                Some(packet)
            })
            .collect();
        let missing_initial_echo_interfaces = self.missing_initial_multicast_echoes();
        let carrier_events = self.update_multicast_echo_timeouts(now, multicast_echo_timeout);

        AutoPeerJobRun {
            expired_peers,
            reverse_peering_packets,
            missing_initial_echo_interfaces,
            carrier_events,
        }
    }

    pub fn run_multicast_announce_job(
        &mut self,
        config: &AutoInterfaceConfig,
        adopted_devices: &[AutoInterfaceAdoptedDevice],
        now: core::time::Duration,
        announce_interval: core::time::Duration,
    ) -> Vec<AutoPeeringPacket> {
        let mut packets = Vec::new();
        for adopted in adopted_devices {
            let due = match self.last_multicast_announces.get(&adopted.ifname) {
                Some(last_announce) => now >= *last_announce + announce_interval,
                None => true,
            };
            if due {
                packets.push(config.multicast_peering_packet(adopted));
                self.last_multicast_announces.insert(adopted.ifname.clone(), now);
            }
        }
        packets
    }

    pub fn update_multicast_echo_timeouts(
        &mut self,
        now: core::time::Duration,
        multicast_echo_timeout: core::time::Duration,
    ) -> Vec<AutoMulticastCarrierEvent> {
        let mut events = Vec::new();
        for ifname in self.adopted_devices.keys() {
            let last_echo = self
                .multicast_echoes
                .get(ifname)
                .copied()
                .unwrap_or_else(|| core::time::Duration::from_secs(0));
            let timed_out = now > last_echo + multicast_echo_timeout;
            match (timed_out, self.timed_out_interfaces.get(ifname).copied()) {
                (true, Some(false)) => {
                    events.push(AutoMulticastCarrierEvent::CarrierLost { ifname: ifname.clone() });
                }
                (false, Some(true)) => {
                    events.push(AutoMulticastCarrierEvent::CarrierRecovered {
                        ifname: ifname.clone(),
                    });
                }
                _ => {}
            }
            self.timed_out_interfaces.insert(ifname.clone(), timed_out);
        }
        events
    }

    pub fn multicast_echo_timed_out(&self, ifname: &str) -> Option<bool> {
        self.timed_out_interfaces.get(ifname).copied()
    }

    pub fn expire_stale_peers(&mut self, now: core::time::Duration) -> Vec<AutoPeer> {
        self.peers.expire_stale(now)
    }

    pub fn reverse_announces_due(&self, now: core::time::Duration) -> Vec<AutoPeer> {
        self.peers.reverse_announces_due(now)
    }

    pub fn mark_reverse_announced(&mut self, address: &str, now: core::time::Duration) -> bool {
        self.peers.mark_reverse_announced(address, now)
    }
}

#[derive(Debug, Clone)]
pub struct AutoInboundPacketDeduplicator {
    entries: VecDeque<(Hash, core::time::Duration)>,
    capacity: usize,
    ttl: core::time::Duration,
}

impl AutoInboundPacketDeduplicator {
    pub fn new(capacity: usize, ttl: core::time::Duration) -> Self {
        Self { entries: VecDeque::with_capacity(capacity), capacity, ttl }
    }

    pub fn from_timing(timing: AutoInterfaceTiming) -> Self {
        Self::new(timing.multi_interface_dedupe_len, timing.multi_interface_dedupe_ttl)
    }

    pub fn should_accept(&mut self, packet: &[u8], now: core::time::Duration) -> bool {
        let packet_hash = Hash::new_from_slice(packet);
        if self.entries.iter().any(|(entry_hash, entry_time)| {
            *entry_hash == packet_hash && now < *entry_time + self.ttl
        }) {
            return false;
        }

        if self.capacity == 0 {
            return true;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back((packet_hash, now));
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
