use reticulum_daemon::config::InterfaceConfig;

pub(crate) fn bind_and_forward_addr(
    iface: &InterfaceConfig,
) -> Result<(String, Option<String>), String> {
    let bind_host = iface
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "udp.host is required".to_string())?;
    let bind_port = iface.port.ok_or_else(|| "udp.port is required".to_string())?;
    let bind_addr = format!("{}:{}", bind_host, bind_port);

    let forward_addr = match (iface.target_host.as_deref(), iface.target_port) {
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

pub(crate) async fn strict_preflight(bind_addr: &str) -> Result<(), String> {
    tokio::net::UdpSocket::bind(bind_addr)
        .await
        .map(|_| ())
        .map_err(|err| format!("udp startup preflight failed for {}: {}", bind_addr, err))
}
