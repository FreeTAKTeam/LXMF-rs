impl From<&AutoPeeringPacket> for AutoPeerAnnounceDatagram {
    fn from(packet: &AutoPeeringPacket) -> Self {
        Self {
            kind: packet.kind,
            ifname: packet.ifname.clone(),
            source_link_local_address: packet.source_link_local_address.clone(),
            destination_address: packet.destination_address.clone(),
            destination_port: packet.destination_port,
            payload: packet.payload().to_vec(),
        }
    }
}

pub(crate) fn build_native_startup_plan(
    iface: &InterfaceConfig,
) -> Result<AutoDaemonStartupPlan, String> {
    let candidates = enumerate_link_local_candidates()?;
    build_startup_plan_from_candidates(iface, candidates)
}

fn build_startup_plan_from_candidates(
    iface: &InterfaceConfig,
    candidates: Vec<AutoInterfaceDeviceCandidate>,
) -> Result<AutoDaemonStartupPlan, String> {
    let config = auto_config(iface)?;
    let platform = current_platform();
    let timing = AutoInterfaceTiming::for_platform(platform);
    let filter = AutoInterfaceDeviceFilter {
        allowed: iface.devices.clone().unwrap_or_default(),
        ignored: iface.ignored_devices.clone().unwrap_or_default(),
    };
    let adopted_devices = filter.adopt_devices(&candidates, platform);
    let startup_plan = config.startup_plan(&adopted_devices, platform, timing);
    let peering_packets =
        adopted_devices.iter().map(|adopted| config.multicast_peering_packet(adopted)).collect();
    Ok(AutoDaemonStartupPlan {
        config,
        platform,
        device_filter: filter,
        candidates,
        adopted_devices,
        peering_packets,
        startup_plan,
    })
}

fn enumerate_link_local_candidates() -> Result<Vec<AutoInterfaceDeviceCandidate>, String> {
    let mut by_name = BTreeMap::<String, Vec<String>>::new();
    for iface in if_addrs::get_if_addrs().map_err(|err| format!("enumerate interfaces: {err}"))? {
        if !iface.is_oper_up() || iface.is_loopback() || !iface.is_link_local() {
            continue;
        }
        let if_addrs::IfAddr::V6(addr) = iface.addr else {
            continue;
        };
        by_name.entry(iface.name).or_default().push(addr.ip.to_string());
    }
    Ok(by_name
        .into_iter()
        .map(|(ifname, ipv6_addresses)| AutoInterfaceDeviceCandidate { ifname, ipv6_addresses })
        .collect())
}

fn auto_config(iface: &InterfaceConfig) -> Result<AutoInterfaceConfig, String> {
    Ok(AutoInterfaceConfig {
        group_id: iface.group_id.clone().unwrap_or_else(|| "reticulum".to_string()),
        discovery_scope: AutoDiscoveryScope::parse(
            iface.discovery_scope.as_deref().unwrap_or("link"),
        )
        .ok_or_else(|| "auto discovery_scope was not normalized".to_string())?,
        multicast_address_type: MulticastAddressType::parse(
            iface.multicast_address_type.as_deref().unwrap_or("temporary"),
        )
        .ok_or_else(|| "auto multicast_address_type was not normalized".to_string())?,
        discovery_port: iface.discovery_port.unwrap_or(29_716),
        data_port: iface.data_port.unwrap_or(42_671),
    })
}

fn startup_plan_json(plan: &AutoStartupPlan) -> JsonValue {
    json!({
        "discovery_listeners": plan.discovery_listeners.iter().map(discovery_listener_json).collect::<Vec<_>>(),
        "data_listeners": plan.data_listeners.iter().map(data_listener_json).collect::<Vec<_>>(),
        "peer_job_interval_ms": plan.peer_job_interval.as_millis() as u64,
        "initial_peering_wait_ms": plan.initial_peering_wait.as_millis() as u64,
    })
}

fn discovery_listener_json(listener: &AutoDiscoveryListenerBinding) -> JsonValue {
    json!({
        "ifname": listener.ifname,
        "link_local_address": listener.link_local_address,
        "unicast_bind_address": listener.unicast_bind_address,
        "unicast_bind_port": listener.unicast_bind_port,
        "multicast_group_address": listener.multicast_group_address,
        "multicast_bind_address": listener.multicast_bind_address,
        "multicast_bind_port": listener.multicast_bind_port,
    })
}

fn data_listener_json(listener: &AutoDataListenerBinding) -> JsonValue {
    json!({
        "ifname": listener.ifname,
        "link_local_address": listener.link_local_address,
        "bind_address": listener.bind_address,
        "bind_port": listener.bind_port,
    })
}

fn candidate_json(candidate: &AutoInterfaceDeviceCandidate) -> JsonValue {
    json!({
        "ifname": candidate.ifname,
        "ipv6_addresses": candidate.ipv6_addresses,
    })
}

fn adopted_json(adopted: &AutoInterfaceAdoptedDevice) -> JsonValue {
    json!({
        "ifname": adopted.ifname,
        "link_local_address": adopted.link_local_address,
    })
}

fn peering_datagram_json(datagram: &AutoPeerAnnounceDatagram) -> JsonValue {
    let target = datagram.socket_target();
    json!({
        "kind": peering_packet_kind(datagram.kind),
        "ifname": datagram.ifname,
        "source_link_local_address": datagram.source_link_local_address,
        "destination_address": datagram.destination_address,
        "destination_port": datagram.destination_port,
        "destination_host": target.host,
        "destination_scope_ifname": target.scope_ifname,
        "destination_socket_target": target.display(),
        "payload_hex": hex::encode(&datagram.payload),
    })
}

fn discovery_socket_bind_json(target: &AutoDiscoverySocketBindTarget) -> JsonValue {
    json!({
        "kind": discovery_socket_kind(target.kind),
        "ifname": target.ifname,
        "bind_host": target.bind_host,
        "bind_port": target.bind_port,
        "scope_ifname": target.scope_ifname,
        "bind_socket_target": target.display_bind_addr(),
        "multicast_group_host": target.multicast_group_host,
    })
}

fn data_socket_bind_json(target: &AutoDataSocketBindTarget) -> JsonValue {
    json!({
        "ifname": target.ifname,
        "bind_host": target.bind_host,
        "bind_port": target.bind_port,
        "scope_ifname": target.scope_ifname,
        "bind_socket_target": target.display_bind_addr(),
    })
}

pub(crate) fn auto_carrier_runtime_json(
    state: &AutoRuntimeState,
    carrier_events: &[AutoMulticastCarrierEvent],
    link_local_update: Option<&AutoLinkLocalAddressUpdate>,
) -> JsonValue {
    json!({
        "online": state.online,
        "final_init_done": state.final_init_done,
        "carrier_changed": state.carrier_changed,
        "carrier_event_count": carrier_events.len(),
        "carrier_events": carrier_events.iter().map(carrier_event_json).collect::<Vec<_>>(),
        "link_local_update": link_local_update.map(link_local_update_json),
    })
}

impl AutoRuntimeStatusHandle {
    pub(crate) fn from_startup_plan(plan: &AutoStartupPlan) -> Self {
        let adopted_devices = plan
            .data_listeners
            .iter()
            .map(|listener| AutoInterfaceAdoptedDevice {
                ifname: listener.ifname.clone(),
                link_local_address: listener.link_local_address.clone(),
            })
            .collect();
        Self {
            inner: Arc::new(std::sync::Mutex::new(AutoRuntimeStatus {
                state: AutoRuntimeState::from_startup_plan(
                    plan,
                    core::time::Duration::ZERO,
                ),
                started_at: Instant::now(),
                carrier_events: Vec::new(),
                link_local_update: None,
                adopted_devices,
                adopted_add_count: 0,
                adopted_remove_count: 0,
                link_local_replacement_count: 0,
                last_adopted_change: None,
            })),
        }
    }

    pub(crate) fn record_carrier_events(&self, events: &[AutoMulticastCarrierEvent]) -> bool {
        let mut guard = self.inner.lock().expect("auto runtime status mutex poisoned");
        if !guard.state.record_carrier_events(events) {
            return false;
        }
        guard.carrier_events = events.to_vec();
        true
    }

    #[allow(dead_code)]
    pub(crate) fn record_link_local_update(
        &self,
        update: Option<&AutoLinkLocalAddressUpdate>,
    ) -> bool {
        let mut guard = self.inner.lock().expect("auto runtime status mutex poisoned");
        if !guard.state.record_link_local_update(update) {
            return false;
        }
        if let Some(update) = update {
            guard
                .adopted_devices
                .iter_mut()
                .filter(|device| device.ifname == update.ifname)
                .for_each(|device| {
                    device.link_local_address = update.new_link_local_address.clone();
                });
            guard.link_local_replacement_count += 1;
            guard.last_adopted_change =
                Some(AutoAdoptedInterfaceChange::LinkLocalChanged(update.clone()));
            guard.link_local_update = Some(update.clone());
        }
        true
    }

    pub(crate) fn record_adopted_interface_change(&self, change: &AutoAdoptedInterfaceChange) {
        let mut guard = self.inner.lock().expect("auto runtime status mutex poisoned");
        match change {
            AutoAdoptedInterfaceChange::Added { adopted, .. } => {
                if let Some(existing) =
                    guard.adopted_devices.iter_mut().find(|device| device.ifname == adopted.ifname)
                {
                    *existing = adopted.clone();
                } else {
                    guard.adopted_devices.push(adopted.clone());
                }
                guard.adopted_devices.sort_by(|left, right| left.ifname.cmp(&right.ifname));
                guard.adopted_add_count += 1;
            }
            AutoAdoptedInterfaceChange::Removed { adopted, .. } => {
                guard.adopted_devices.retain(|device| device.ifname != adopted.ifname);
                guard.adopted_remove_count += 1;
            }
            AutoAdoptedInterfaceChange::LinkLocalChanged(update) => {
                if let Some(existing) =
                    guard.adopted_devices.iter_mut().find(|device| device.ifname == update.ifname)
                {
                    existing.link_local_address = update.new_link_local_address.clone();
                }
                guard.link_local_update = Some(update.clone());
                guard.link_local_replacement_count += 1;
            }
        }
        guard.state.carrier_changed = true;
        guard.last_adopted_change = Some(change.clone());
    }

    pub(crate) fn to_json(&self) -> JsonValue {
        let mut guard = self.inner.lock().expect("auto runtime status mutex poisoned");
        let elapsed = guard.started_at.elapsed();
        guard.state.advance(elapsed);
        let mut value = auto_carrier_runtime_json(
            &guard.state,
            &guard.carrier_events,
            guard.link_local_update.as_ref(),
        );
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "adopted_device_count".to_string(),
                json!(guard.adopted_devices.len()),
            );
            object.insert(
                "adopted_devices".to_string(),
                json!(guard.adopted_devices.iter().map(adopted_json).collect::<Vec<_>>()),
            );
            object.insert("adopted_add_count".to_string(), json!(guard.adopted_add_count));
            object.insert("adopted_remove_count".to_string(), json!(guard.adopted_remove_count));
            object.insert(
                "link_local_replacement_count".to_string(),
                json!(guard.link_local_replacement_count),
            );
            object.insert(
                "last_adopted_change".to_string(),
                guard
                    .last_adopted_change
                    .as_ref()
                    .map(adopted_change_json)
                    .unwrap_or(JsonValue::Null),
            );
        }
        value
    }
}

fn adopted_change_json(change: &AutoAdoptedInterfaceChange) -> JsonValue {
    match change {
        AutoAdoptedInterfaceChange::Added { adopted, .. } => json!({
            "event": "added",
            "ifname": adopted.ifname,
            "link_local_address": adopted.link_local_address,
        }),
        AutoAdoptedInterfaceChange::Removed { adopted, .. } => json!({
            "event": "removed",
            "ifname": adopted.ifname,
            "link_local_address": adopted.link_local_address,
        }),
        AutoAdoptedInterfaceChange::LinkLocalChanged(update) => json!({
            "event": "link_local_changed",
            "ifname": update.ifname,
            "old_link_local_address": update.old_link_local_address,
            "new_link_local_address": update.new_link_local_address,
        }),
    }
}

fn carrier_event_json(event: &AutoMulticastCarrierEvent) -> JsonValue {
    match event {
        AutoMulticastCarrierEvent::CarrierLost { ifname } => {
            json!({
                "event": "carrier_lost",
                "ifname": ifname,
            })
        }
        AutoMulticastCarrierEvent::CarrierRecovered { ifname } => {
            json!({
                "event": "carrier_recovered",
                "ifname": ifname,
            })
        }
    }
}

fn link_local_update_json(update: &AutoLinkLocalAddressUpdate) -> JsonValue {
    json!({
        "ifname": update.ifname,
        "old_link_local_address": update.old_link_local_address,
        "new_link_local_address": update.new_link_local_address,
        "restart_data_listener": data_listener_json(&update.listener_binding),
    })
}

pub(crate) fn discovery_runtime_summary_json(summary: &AutoDiscoveryRuntimeSummary) -> JsonValue {
    json!({
        "bound_socket_count": summary.bound_socket_count,
        "receive_loop_count": summary.receive_loop_count,
        "initial_peer_announce_count": summary.initial_peer_announce_count,
        "repeat_peer_announce_scheduler_count": summary.repeat_peer_announce_scheduler_count,
        "peer_job_scheduler_count": summary.peer_job_scheduler_count,
        "adopted_interface_reconciler_count": summary.adopted_interface_reconciler_count,
        "data_socket_count": summary.data_socket_count,
        "data_receive_loop_count": summary.data_receive_loop_count,
    })
}

fn discovery_source_address(datagram: &AutoDiscoveryDatagram) -> String {
    datagram.source_addr.ip().to_string()
}

fn peer_data_source_address(datagram: &AutoPeerDataDatagram) -> String {
    datagram.source_addr.ip().to_string()
}

fn log_auto_discovery_loop_event(event: AutoDiscoveryLoopEvent) {
    match event {
        AutoDiscoveryLoopEvent::Processed(processed) => {
            log::debug!(
                "[daemon-auto] discovery accepted iface={} source={} event={:?}",
                processed.datagram.ifname,
                processed.source_address,
                processed.event
            );
        }
        AutoDiscoveryLoopEvent::Rejected { datagram, source_address, reason } => {
            log::debug!(
                "[daemon-auto] discovery rejected iface={} source={} reason={:?}",
                datagram.ifname,
                source_address,
                reason
            );
        }
        AutoDiscoveryLoopEvent::ReceiveFailed { ifname, kind, bind_addr, error } => {
            log::warn!(
                "[daemon-auto] discovery receive failed iface={} kind={} bind={} err={}",
                ifname,
                discovery_socket_kind(kind),
                bind_addr,
                error
            );
        }
    }
}

fn log_auto_peer_data_loop_event(event: AutoPeerDataLoopEvent) {
    match event {
        AutoPeerDataLoopEvent::Processed(processed) => {
            log::debug!(
                "[daemon-auto] peer data processed iface={} peer={} decision={:?}",
                processed.datagram.ifname,
                processed.peer_address,
                processed.decision
            );
        }
        AutoPeerDataLoopEvent::ReceiveFailed { ifname, bind_addr, error } => {
            log::warn!(
                "[daemon-auto] peer data receive failed iface={} bind={} err={}",
                ifname,
                bind_addr,
                error
            );
        }
    }
}

fn peering_packet_kind(kind: AutoPeeringPacketKind) -> &'static str {
    match kind {
        AutoPeeringPacketKind::Multicast => "multicast",
        AutoPeeringPacketKind::ReverseUnicast => "reverse_unicast",
    }
}

fn discovery_socket_kind(kind: AutoDiscoverySocketKind) -> &'static str {
    match kind {
        AutoDiscoverySocketKind::Unicast => "unicast",
        AutoDiscoverySocketKind::Multicast => "multicast",
    }
}

fn current_platform() -> AutoInterfacePlatform {
    if cfg!(target_os = "windows") {
        AutoInterfacePlatform::Windows
    } else if cfg!(target_os = "macos") {
        AutoInterfacePlatform::Darwin
    } else if cfg!(target_os = "android") {
        AutoInterfacePlatform::Android
    } else {
        AutoInterfacePlatform::Other
    }
}

fn platform_name(platform: AutoInterfacePlatform) -> &'static str {
    match platform {
        AutoInterfacePlatform::Other => "other",
        AutoInterfacePlatform::Darwin => "darwin",
        AutoInterfacePlatform::Windows => "windows",
        AutoInterfacePlatform::Android => "android",
    }
}

fn socket_target(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn is_link_scope_ipv6_multicast(address: &str) -> bool {
    let first_segment = address.split(':').next().unwrap_or_default();
    let bytes = first_segment.as_bytes();
    bytes.len() >= 4
        && bytes[0].eq_ignore_ascii_case(&b'f')
        && bytes[1].eq_ignore_ascii_case(&b'f')
        && bytes[3] == b'2'
}

fn split_ipv6_scope(address: &str) -> (&str, Option<&str>) {
    match address.split_once('%') {
        Some((host, scope)) => (host, Some(scope)),
        None => (address, None),
    }
}

fn bind_host_and_scope(address: &str, fallback_scope_ifname: &str) -> (String, Option<String>) {
    if address.trim().is_empty() {
        return ("::".to_string(), None);
    }
    let (host, explicit_scope) = split_ipv6_scope(address);
    let scope_ifname = explicit_scope
        .map(str::to_string)
        .or_else(|| is_link_scope_ipv6_multicast(host).then(|| fallback_scope_ifname.to_string()));
    (host.to_string(), scope_ifname)
}
