use reticulum_daemon::config::InterfaceConfig;

pub(crate) fn bind_and_forward_addr(
    iface: &InterfaceConfig,
) -> Result<(String, Option<String>), String> {
    bind_and_forward_addr_with_device_resolver(iface, resolve_device_broadcast_addr)
}

pub(crate) fn bind_and_forward_addr_with_device_resolver<F>(
    iface: &InterfaceConfig,
    resolve_device_broadcast: F,
) -> Result<(String, Option<String>), String>
where
    F: Fn(&str) -> Result<String, String>,
{
    let device = iface.device.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let needs_device_broadcast = device.is_some()
        && (iface.host.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none()
            || iface
                .target_host
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none());
    let device_broadcast = if needs_device_broadcast {
        Some(resolve_device_broadcast(device.expect("device checked"))?)
    } else {
        None
    };

    let bind_host = iface
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| device_broadcast.clone())
        .ok_or_else(|| "udp.host or udp.device is required".to_string())?;
    let bind_port = iface.port.ok_or_else(|| "udp.port is required".to_string())?;
    let bind_addr = format!("{}:{}", bind_host, bind_port);

    let target_host = iface
        .target_host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| device_broadcast.clone());
    let target_port = iface.target_port.or_else(|| target_host.as_ref().map(|_| bind_port));

    let forward_addr = match (target_host.as_deref(), target_port) {
        (Some(host), Some(port)) => {
            let host = host.trim();
            if host.is_empty() {
                return Err("udp.target_host cannot be empty".to_string());
            }
            Some(format!("{}:{}", host, port))
        }
        (None, None) => None,
        _ => {
            return Err("udp.target_host and udp.target_port must be provided together".to_string())
        }
    };

    Ok((bind_addr, forward_addr))
}

fn resolve_device_broadcast_addr(device: &str) -> Result<String, String> {
    let interfaces =
        if_addrs::get_if_addrs().map_err(|err| format!("udp.device lookup failed: {err}"))?;
    interfaces
        .into_iter()
        .find_map(|iface| {
            if iface.name != device {
                return None;
            }
            match iface.addr {
                if_addrs::IfAddr::V4(addr) => addr.broadcast.map(|broadcast| broadcast.to_string()),
                if_addrs::IfAddr::V6(_) => None,
            }
        })
        .ok_or_else(|| format!("udp.device {device} has no IPv4 broadcast address"))
}

pub(crate) async fn strict_preflight(bind_addr: &str) -> Result<(), String> {
    tokio::net::UdpSocket::bind(bind_addr)
        .await
        .map(|_| ())
        .map_err(|err| format!("udp startup preflight failed for {}: {}", bind_addr, err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_device_defaults_bind_and_forward_to_resolved_broadcast() {
        let iface = InterfaceConfig {
            kind: "udp".to_string(),
            enabled: Some(true),
            device: Some("eth0".to_string()),
            port: Some(4242),
            ..InterfaceConfig::default()
        };

        let (bind_addr, forward_addr) =
            bind_and_forward_addr_with_device_resolver(&iface, |device| {
                assert_eq!(device, "eth0");
                Ok("192.0.2.255".to_string())
            })
            .expect("device udp addresses");

        assert_eq!(bind_addr, "192.0.2.255:4242");
        assert_eq!(forward_addr.as_deref(), Some("192.0.2.255:4242"));
    }

    #[test]
    fn udp_device_preserves_explicit_bind_and_derives_missing_forward() {
        let iface = InterfaceConfig {
            kind: "udp".to_string(),
            enabled: Some(true),
            device: Some("eth0".to_string()),
            host: Some("0.0.0.0".to_string()),
            port: Some(4242),
            ..InterfaceConfig::default()
        };

        let (bind_addr, forward_addr) =
            bind_and_forward_addr_with_device_resolver(&iface, |_| Ok("192.0.2.255".to_string()))
                .expect("device udp addresses");

        assert_eq!(bind_addr, "0.0.0.0:4242");
        assert_eq!(forward_addr.as_deref(), Some("192.0.2.255:4242"));
    }
}
