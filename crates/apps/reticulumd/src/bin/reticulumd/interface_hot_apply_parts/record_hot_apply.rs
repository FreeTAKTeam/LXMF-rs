use rns_rpc::InterfaceRecord;
use rns_transport::hash::AddressHash;
use rns_transport::iface::pipe::PipeInterface;
use rns_transport::iface::{InterfaceManager, InterfaceMode};
use std::io;
use std::time::Duration;

use super::record_settings::interface_record_shared_config;
use super::record_settings::{setting_bool, setting_f64, setting_str, setting_u64};

pub(crate) fn validate_hot_apply_uniqueness(
    interfaces: &[InterfaceRecord],
) -> Result<(), io::Error> {
    let mut seen = std::collections::HashSet::new();
    let mut seen_tcp_server_bind_addresses = std::collections::HashSet::new();
    let mut seen_tcp_server_any_bind_ports = std::collections::HashSet::new();
    let mut seen_tcp_server_wildcard_bind_ports = std::collections::HashSet::new();
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
            if record.enabled {
                let port = record.port.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "tcp_server hot-apply requires port",
                    )
                })?;
                let host = tcp_server_hot_apply_host(record.host.as_deref().unwrap_or_default());
                if host_is_ipv4_unspecified(host) {
                    if !seen_tcp_server_any_bind_ports.insert(port) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("duplicate tcp_server bind address '{bind_addr}'"),
                        ));
                    }
                    seen_tcp_server_wildcard_bind_ports.insert(port);
                } else {
                    if seen_tcp_server_wildcard_bind_ports.contains(&port) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("duplicate tcp_server bind address '{bind_addr}'"),
                        ));
                    }
                    seen_tcp_server_any_bind_ports.insert(port);
                }
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
        if record.kind == "pipe" && record.enabled {
            pipe_adapter(record)?;
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
        "pipe" => pipe_interface_key(record),
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
        "pipe" => {
            if record.enabled {
                pipe_adapter(record).ok()?;
            }
            pipe_interface_key(record)
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
        || (current.kind == "tcp_server"
            && setting_str(current, "device") != setting_str(next, "device"))
        || (current.kind == "tcp_server"
            && setting_bool(current, "prefer_ipv6") != setting_bool(next, "prefer_ipv6"))
        || (current.kind == "udp" && udp_forward_addr(current) != udp_forward_addr(next))
        || (current.kind == "udp" && setting_str(current, "device") != setting_str(next, "device"))
        || (current.kind == "pipe"
            && pipe_runtime_signature(current) != pipe_runtime_signature(next))
}

pub(crate) fn tcp_endpoint(record: &InterfaceRecord) -> Option<String> {
    Some(format!("{}:{}", record.host.as_ref()?, record.port?))
}

pub(crate) fn tcp_server_bind_addr(record: &InterfaceRecord) -> Result<String, io::Error> {
    tcp_server_bind_addr_with_device_resolver(
        record,
        crate::bootstrap::resolve_tcp_listener_device_bind_host,
    )
}

pub(crate) fn tcp_server_bind_addr_with_device_resolver<F>(
    record: &InterfaceRecord,
    resolve_device_host: F,
) -> Result<String, io::Error>
where
    F: Fn(&str, bool) -> Result<String, String>,
{
    let host = if let Some(host) =
        record.host.as_deref().map(str::trim).filter(|value| !value.is_empty())
    {
        host.to_string()
    } else if let Some(device) = setting_str(record, "device") {
        resolve_device_host(device, setting_bool(record, "prefer_ipv6").unwrap_or(false)).map_err(
            |err| {
                io::Error::new(io::ErrorKind::InvalidInput, format!("tcp_server hot-apply {err}"))
            },
        )?
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tcp_server hot-apply requires host or device",
        ));
    };
    if setting_bool(record, "i2p_tunneled").unwrap_or(false) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tcp_server hot-apply does not support i2p_tunneled records",
        ));
    }
    let port = record.port.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "tcp_server hot-apply requires port")
    })?;
    Ok(format_endpoint(tcp_server_hot_apply_host(host.as_str()), port))
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

fn host_is_ipv4_unspecified(host: &str) -> bool {
    let host = host.trim();
    host.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| ip.is_unspecified())
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
    udp_bind_and_forward_addr_with_device_resolver(
        record,
        crate::interfaces::udp::resolve_device_broadcast_addr,
    )
}

pub(crate) fn udp_bind_and_forward_addr_with_device_resolver<F>(
    record: &InterfaceRecord,
    resolve_device_broadcast: F,
) -> Result<(String, Option<String>), io::Error>
where
    F: Fn(&str) -> Result<String, String>,
{
    let configured_host = record.host.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let device = setting_str(record, "device").map(str::trim).filter(|value| !value.is_empty());
    let configured_target_host =
        setting_str(record, "target_host").or_else(|| setting_str(record, "forward_ip"));
    let needs_device_broadcast =
        device.is_some() && (configured_host.is_none() || configured_target_host.is_none());
    let device_broadcast = if needs_device_broadcast {
        Some(resolve_device_broadcast(device.expect("device checked")).map_err(|err| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("udp hot-apply {err}"))
        })?)
    } else {
        None
    };
    let host =
        configured_host.map(ToOwned::to_owned).or_else(|| device_broadcast.clone()).ok_or_else(
            || io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires host or device"),
        )?;
    let port = record.port.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "udp hot-apply requires port")
    })?;
    let forward_addr = udp_forward_addr_result(record, device_broadcast.as_deref())?;
    Ok((format!("{host}:{port}"), forward_addr))
}

fn udp_forward_addr(record: &InterfaceRecord) -> Option<String> {
    udp_forward_addr_result(record, None).ok().flatten()
}

fn udp_forward_addr_result(
    record: &InterfaceRecord,
    device_broadcast: Option<&str>,
) -> Result<Option<String>, io::Error> {
    let target_host = setting_str(record, "target_host");
    let forward_ip = setting_str(record, "forward_ip");
    let configured_host = target_host.or(forward_ip);
    let host = configured_host.or(device_broadcast);
    let port = setting_u64(record, "target_port")
        .or_else(|| setting_u64(record, "forward_port"))
        .or_else(|| {
            if (target_host.is_none() && forward_ip.is_some())
                || (configured_host.is_none() && device_broadcast.is_some())
            {
                record.port.map(u64::from)
            } else {
                None
            }
        });
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
        let forward_host =
            setting_str(record, "target_host").or_else(|| setting_str(record, "forward_ip"));
        let role = if record.host.as_deref().is_some_and(host_is_multicast)
            || forward_host.is_some_and(host_is_multicast)
        {
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

pub(crate) fn pipe_interface_key(record: &InterfaceRecord) -> Option<String> {
    if record.kind != "pipe" {
        return None;
    }
    if let Some(name) =
        record.name.as_ref().map(|value| value.trim()).filter(|value| !value.is_empty())
    {
        return Some(name.to_string());
    }
    pipe_command(record).ok()
}

pub(crate) fn pipe_adapter(record: &InterfaceRecord) -> Result<PipeInterface, io::Error> {
    let command = pipe_command(record)?;
    PipeInterface::parse_command(command.as_str()).map_err(|err| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("pipe hot-apply {err}"))
    })?;
    let respawn_delay = pipe_respawn_delay(record)?;
    Ok(PipeInterface::new(command)
        .with_respawn_delay(respawn_delay)
        .with_mtu(pipe_mtu(record).unwrap_or(PipeInterface::DEFAULT_MTU)))
}

pub(crate) fn mark_pipe_record_runtime_status(
    record: &mut InterfaceRecord,
    runtime_iface: Option<AddressHash>,
) {
    let Ok(adapter) = pipe_adapter(record) else {
        return;
    };
    let iface = runtime_iface.map(|value| value.to_string());
    crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
        runtime.insert(
            "pipe".to_string(),
            serde_json::json!({
                "status": adapter.runtime_status_json(),
                "mtu": adapter.mtu_value(),
                "iface": iface,
            }),
        );
    });
}

fn pipe_runtime_signature(record: &InterfaceRecord) -> Option<(String, u64, usize)> {
    let command = pipe_command(record).ok()?;
    let respawn_millis = u64::try_from(pipe_respawn_delay(record).ok()?.as_millis()).ok()?;
    let mtu = pipe_mtu(record).unwrap_or(PipeInterface::DEFAULT_MTU).max(256);
    Some((command, respawn_millis, mtu))
}

fn pipe_command(record: &InterfaceRecord) -> Result<String, io::Error> {
    setting_str(record, "command").map(ToOwned::to_owned).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "pipe hot-apply requires command")
    })
}

fn pipe_respawn_delay(record: &InterfaceRecord) -> Result<Duration, io::Error> {
    let delay = setting_f64(record, "respawn_delay").unwrap_or(5.0);
    if delay < 0.0 || !delay.is_finite() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pipe hot-apply respawn_delay must be finite and >= 0",
        ));
    }
    Ok(Duration::from_secs_f64(delay))
}

fn pipe_mtu(record: &InterfaceRecord) -> Option<usize> {
    setting_u64(record, "mtu").and_then(|value| usize::try_from(value).ok())
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
