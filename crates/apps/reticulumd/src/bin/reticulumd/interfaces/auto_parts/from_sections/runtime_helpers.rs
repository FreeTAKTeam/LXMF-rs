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
        AutoDiscoveryLoopEvent::Rejected {
            datagram,
            source_address,
            reason,
        } => {
            log::debug!(
                "[daemon-auto] discovery rejected iface={} source={} reason={:?}",
                datagram.ifname,
                source_address,
                reason
            );
        }
        AutoDiscoveryLoopEvent::ReceiveFailed {
            ifname,
            kind,
            bind_addr,
            error,
        } => {
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
        AutoPeerDataLoopEvent::ReceiveFailed {
            ifname,
            bind_addr,
            error,
        } => {
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
