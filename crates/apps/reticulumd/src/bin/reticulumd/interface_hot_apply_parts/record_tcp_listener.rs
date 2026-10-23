use rns_rpc::InterfaceRecord;
use std::io;
use std::net::IpAddr;

use super::record_settings::{setting_bool, setting_str, setting_u64};

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
    let prefer_ipv6 = setting_bool(record, "prefer_ipv6").unwrap_or(false);
    Ok(format_endpoint(tcp_server_hot_apply_host(host.as_str(), prefer_ipv6), port))
}

pub(super) fn tcp_server_bind_candidates(record: &InterfaceRecord, bind_addr: &str) -> Vec<String> {
    let preferred_localhost = setting_bool(record, "prefer_ipv6") == Some(true)
        && record.host.as_deref().is_some_and(|host| {
            tcp_server_hot_apply_host(host, true).eq_ignore_ascii_case("localhost")
        });
    match (record.port, preferred_localhost) {
        // The native resolver may fall back, so reserve both known loopback aliases.
        (Some(port), true) => vec![
            bind_addr.to_owned(),
            format_endpoint("127.0.0.1", port),
            format_endpoint("::1", port),
        ],
        _ => vec![bind_addr.to_owned()],
    }
}

pub(crate) fn tcp_server_client_mtu(record: &InterfaceRecord) -> Option<usize> {
    setting_u64(record, "mtu").and_then(|value| usize::try_from(value).ok())
}

fn format_endpoint(host: &str, port: u16) -> String {
    let host = host.trim();
    let host = host.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(host);
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(address)) => format!("[{address}]:{port}"),
        Ok(IpAddr::V4(address)) => format!("{address}:{port}"),
        Err(_) if host.contains(':') => format!("[{host}]:{port}"),
        Err(_) => format!("{host}:{port}"),
    }
}

pub(super) fn host_is_unspecified(host: &str) -> bool {
    let host = host.trim();
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_unspecified())
}

pub(super) fn tcp_server_hot_apply_host(host: &str, prefer_ipv6: bool) -> &str {
    let host = host.trim();
    let host = host.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(host);
    if host.eq_ignore_ascii_case("localhost") && !prefer_ipv6 {
        "127.0.0.1"
    } else {
        host
    }
}
