use rns_rpc::InterfaceRecord;
use rns_transport::hash::AddressHash;
use rns_transport::iface::{InterfaceManager, InterfaceMode};
use std::io;

use super::record_settings::interface_record_shared_config;
use super::record_settings::{setting_bool, setting_str, setting_u64};

pub(crate) fn validate_hot_apply_uniqueness(
    interfaces: &[InterfaceRecord],
) -> Result<(), io::Error> {
    let mut seen = std::collections::HashSet::new();
    let mut seen_tcp_server_bind_addresses = std::collections::HashSet::new();
    let mut seen_udp_bind_addresses = std::collections::HashSet::new();
    for record in interfaces {
        if record.kind == "tcp_server" {
            let bind_addr = tcp_server_bind_addr(record)?;
            if record.enabled && !seen_tcp_server_bind_addresses.insert(bind_addr.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("duplicate tcp_server bind address '{bind_addr}'"),
                ));
            }
        }
        if record.kind == "udp" {
            let (bind_addr, _) = udp_bind_and_forward_addr(record)?;
            if record.enabled && !seen_udp_bind_addresses.insert(bind_addr.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("duplicate udp bind address '{bind_addr}'"),
                ));
            }
        }
        let Some(key) = hot_apply_interface_key(record) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate hot-apply interface key '{key}'"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn hot_apply_interface_key(record: &InterfaceRecord) -> Option<String> {
    match record.kind.as_str() {
        "tcp_client" => tcp_interface_key(record),
        "tcp_server" => tcp_server_interface_key(record),
        "udp" => udp_interface_key(record),
        _ => None,
    }
}

pub(crate) fn tcp_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "tcp_client" {
        return None;
    }
    if let Some(name) =
        record.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
    {
        return Some(name.to_string());
    }
    let host = record.host.as_ref()?.trim();
    let port = record.port?;
    Some(format!("{host}:{port}"))
}

pub(crate) fn hot_apply_interface_seed_key(record: &InterfaceRecord) -> Option<String> {
    match record.kind.as_str() {
        "tcp_client" => tcp_interface_key(record),
        "tcp_server" => {
            tcp_server_bind_addr(record).ok()?;
            tcp_server_interface_key(record)
        }
        "udp" => {
            udp_bind_and_forward_addr(record).ok()?;
            udp_interface_key(record)
        }
        _ => None,
    }
}

pub(crate) fn tcp_server_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "tcp_server" {
        return None;
    }
    if let Some(name) =
        record.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
    {
        return Some(name.to_string());
    }
    tcp_server_bind_addr(record).ok()
}

fn udp_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "udp" {
        return None;
    }
    if let Some(name) =
        record.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
    {
        return Some(name.to_string());
    }
    let host = record.host.as_ref()?.trim();
    let port = record.port?;
    Some(format!("{host}:{port}"))
}

pub(crate) fn hot_apply_interface_record_changed(
    current: &InterfaceRecord,
    next: &InterfaceRecord,
) -> bool {
    current.kind != next.kind
        || current.enabled != next.enabled
        || current.host != next.host
        || current.port != next.port
        || (current.kind == "tcp_server"
            && tcp_server_client_mtu(current) != tcp_server_client_mtu(next))
        || (current.kind == "udp" && udp_forward_addr(current) != udp_forward_addr(next))
}

pub(crate) fn tcp_endpoint(record: &InterfaceRecord) -> Option<String> {
    Some(format!("{}:{}", record.host.as_ref()?, record.port?))
}

pub(crate) fn tcp_server_bind_addr(record: &InterfaceRecord) -> Result<String, io::Error> {
    let host = record.host.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(
        || io::Error::new(io::ErrorKind::InvalidInput, "tcp_server hot-apply requires host"),
    )?;
    if setting_str(record, "device").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tcp_server hot-apply does not support device-bound records",
        ));
    }
    if setting_bool(record, "prefer_ipv6").unwrap_or(false) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tcp_server hot-apply does not support prefer_ipv6 records",
        ));
    }
    if setting_bool(record, "i2p_tunneled").unwrap_or(false) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tcp_server hot-apply does not support i2p_tunneled records",
        ));
    }
    if !host_is_loopback(host) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tcp_server hot-apply requires an explicit loopback host",
        ));
    }
    let port = record.port.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "tcp_server hot-apply requires port")
    })?;
    Ok(format_endpoint(tcp_server_hot_apply_host(host), port))
}

pub(crate) fn tcp_server_client_mtu(record: &InterfaceRecord) -> Option<usize> {
    setting_u64(record, "mtu").and_then(|value| usize::try_from(value).ok())
}

fn format_endpoint(host: &str, port: u16) -> String {
    let host = host.trim();
    let host = host.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(host);
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn host_is_loopback(host: &str) -> bool {
    let host = host.trim();
    let host = host.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(host);
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn tcp_server_hot_apply_host(host: &str) -> &str {
    let host = host.trim();
    let host = host.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(host);
    if host.eq_ignore_ascii_case("localhost") {
        "127.0.0.1"
    } else {
        host
    }
}

pub(crate) fn udp_bind_and_forward_addr(
    record: &InterfaceRecord,
) -> Result<(String, Option<String>), io::Error> {
    let host = record.host.as_deref().map(str::trim).filter(|value| !value.is_empty()).ok_or_else(
        || io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires host"),
    )?;
    if setting_str(record, "device").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "udp hot-apply does not support device-bound records",
        ));
    }
    let port = record.port.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires port")
    })?;
    Ok((format!("{host}:{port}"), udp_forward_addr_result(record)?))
}

fn udp_forward_addr(record: &InterfaceRecord) -> Option<String> {
    udp_forward_addr_result(record).ok().flatten()
}

fn udp_forward_addr_result(record: &InterfaceRecord) -> Result<Option<String>, io::Error> {
    let host = setting_str(record, "target_host").or_else(|| setting_str(record, "forward_ip"));
    let port = setting_u64(record, "target_port").or_else(|| setting_u64(record, "forward_port"));
    let (host, port) = match (host, port) {
        (Some(host), Some(port)) => (host, port),
        (None, None) => return Ok(None),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "udp hot-apply target_host and target_port must be provided together",
            ))
        }
    };
    if host_is_multicast(host) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "udp hot-apply does not support multicast targets",
        ));
    }
    let port = u16::try_from(port).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply target_port is out of range")
    })?;
    Ok(Some(format!("{host}:{port}")))
}

pub(crate) fn mark_udp_record_runtime_status(
    record: &mut InterfaceRecord,
    runtime_iface: Option<AddressHash>,
) {
    if let Ok((bind_addr, forward_addr)) = udp_bind_and_forward_addr(record) {
        let role = if record.host.as_deref().is_some_and(host_is_multicast) {
            "multicast"
        } else if forward_addr.is_some() {
            "peer"
        } else {
            "listener"
        };
        let iface = runtime_iface.map(|value| value.to_string());
        crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
            runtime.insert(
                "udp".to_string(),
                serde_json::json!({
                    "status": {
                        "link_state": "configured",
                        "role": role,
                        "bind_addr": bind_addr,
                        "forward_addr": forward_addr,
                        "iface": iface,
                    }
                }),
            );
        });
    }
}

pub(crate) fn mark_tcp_server_record_runtime_status(
    record: &mut InterfaceRecord,
    runtime_iface: Option<AddressHash>,
) {
    if let Ok(bind_addr) = tcp_server_bind_addr(record) {
        let iface = runtime_iface.map(|value| value.to_string());
        crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
            runtime.insert(
                "tcp".to_string(),
                serde_json::json!({
                    "listener_status": {
                        "bind_addr": bind_addr,
                        "listener_state": "configured",
                        "iface": iface,
                    }
                }),
            );
        });
    }
}

fn host_is_multicast(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_multicast())
}

pub(crate) fn apply_record_runtime_config(
    manager: &mut InterfaceManager,
    address: AddressHash,
    record: &InterfaceRecord,
) {
    manager.set_mode(address, interface_record_mode(record));
    manager.set_outgoing(address, setting_bool(record, "outgoing").unwrap_or(true));
    manager.set_announce_pacing(
        address,
        setting_u64(record, "bitrate").unwrap_or(62_500),
        setting_u64(record, "announce_cap").unwrap_or(2),
    );
    manager.set_shared_config(address, interface_record_shared_config(record));
}

pub(crate) fn interface_record_mode(record: &InterfaceRecord) -> InterfaceMode {
    setting_str(record, "interface_mode")
        .or_else(|| setting_str(record, "mode"))
        .and_then(|value| InterfaceMode::parse(value).ok().flatten())
        .unwrap_or(InterfaceMode::Full)
}
