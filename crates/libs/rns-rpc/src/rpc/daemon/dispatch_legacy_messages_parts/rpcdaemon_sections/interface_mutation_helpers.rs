impl RpcDaemon {
    pub(super) fn restart_required_response(
        id: u64,
        operation: &str,
        affected_interfaces: Vec<String>,
    ) -> RpcResponse {
        let mut error = RpcError::new(
            "CONFIG_RESTART_REQUIRED",
            "requested interface mutation requires daemon restart",
        );
        error.machine_code = Some("UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART".to_string());
        error.category = Some("Config".to_string());
        error.retryable = Some(false);
        error.is_user_actionable = Some(true);

        let mut details = serde_json::Map::new();
        details.insert("operation".to_string(), JsonValue::String(operation.to_string()));
        details.insert(
            "affected_interfaces".to_string(),
            JsonValue::Array(
                affected_interfaces
                    .iter()
                    .map(|item| JsonValue::String(item.clone()))
                    .collect::<Vec<_>>(),
            ),
        );
        details.insert(
            "legacy_hot_apply_supported_kinds".to_string(),
            json!(["tcp_client", "tcp_server", "udp"]),
        );
        error.details = Some(Box::new(details));

        RpcResponse { id, result: None, error: Some(error) }
    }

    pub(super) fn interface_identifier(iface: &InterfaceRecord, index: usize) -> String {
        iface
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{}[{index}]", iface.kind))
    }

    pub(super) fn is_reload_hot_apply_compatible(
        current: &[InterfaceRecord],
        next: &[InterfaceRecord],
    ) -> bool {
        if current.len() != next.len() {
            return false;
        }
        current.iter().zip(next.iter()).all(|(before, after)| {
            before.kind == after.kind
                && Self::is_legacy_hot_apply_record(before)
                && Self::is_legacy_hot_apply_record(after)
        })
    }

    pub(super) fn validate_legacy_hot_apply_uniqueness(
        interfaces: &[InterfaceRecord],
    ) -> Result<(), std::io::Error> {
        let mut seen = std::collections::HashSet::new();
        let mut seen_tcp_server_bind_addresses = std::collections::HashSet::new();
        let mut seen_udp_bind_addresses = std::collections::HashSet::new();
        for (index, iface) in interfaces.iter().enumerate() {
            if !Self::is_legacy_hot_apply_record(iface) {
                continue;
            }
            let Some(key) = Self::legacy_hot_apply_interface_key(iface) else {
                continue;
            };
            if !seen.insert(key.clone()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "duplicate legacy hot-apply interface key '{}' at {}",
                        key,
                        Self::interface_identifier(iface, index)
                    ),
                ));
            }
            if iface.kind == "tcp_server" && iface.enabled {
                let Some(bind_addr) = Self::legacy_tcp_server_bind_addr(iface) else {
                    continue;
                };
                if !seen_tcp_server_bind_addresses.insert(bind_addr.clone()) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "duplicate legacy tcp_server bind address '{}' at {}",
                            bind_addr,
                            Self::interface_identifier(iface, index)
                        ),
                    ));
                }
            }
            if iface.kind == "udp" && iface.enabled {
                let Some(bind_addr) = Self::legacy_udp_bind_addr(iface) else {
                    continue;
                };
                if !seen_udp_bind_addresses.insert(bind_addr.clone()) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "duplicate legacy udp bind address '{}' at {}",
                            bind_addr,
                            Self::interface_identifier(iface, index)
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn legacy_tcp_interface_key(iface: &InterfaceRecord) -> Option<String> {
        if iface.kind != "tcp_client" {
            return None;
        }
        if let Some(name) = iface.name.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            return Some(name.to_string());
        }
        let host = iface.host.as_deref()?.trim();
        let port = iface.port?;
        Some(format!("{host}:{port}"))
    }

    pub(super) fn is_legacy_hot_apply_record(iface: &InterfaceRecord) -> bool {
        match iface.kind.as_str() {
            "tcp_client" => true,
            "tcp_server" => Self::tcp_server_record_is_hot_apply_safe(iface),
            "udp" => Self::udp_record_is_hot_apply_safe(iface),
            _ => false,
        }
    }

    pub(super) fn legacy_hot_apply_interface_key(iface: &InterfaceRecord) -> Option<String> {
        match iface.kind.as_str() {
            "tcp_client" => Self::legacy_tcp_interface_key(iface),
            "tcp_server" => Self::legacy_tcp_server_interface_key(iface),
            "udp" => Self::legacy_udp_interface_key(iface),
            _ => None,
        }
    }

    fn legacy_tcp_server_interface_key(iface: &InterfaceRecord) -> Option<String> {
        if iface.kind != "tcp_server" {
            return None;
        }
        if let Some(name) = iface.name.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            return Some(name.to_string());
        }
        Self::legacy_tcp_server_bind_addr(iface)
    }

    fn tcp_server_record_is_hot_apply_safe(iface: &InterfaceRecord) -> bool {
        if iface.kind != "tcp_server"
            || iface.host.as_deref().map(str::trim).is_none_or(str::is_empty)
            || iface.port.is_none()
            || Self::interface_setting_str(iface, "device").is_some()
            || Self::interface_setting_bool(iface, "prefer_ipv6").unwrap_or(false)
            || Self::interface_setting_bool(iface, "i2p_tunneled").unwrap_or(false)
        {
            return false;
        }
        iface.host.as_deref().is_some_and(Self::host_is_loopback)
    }

    fn legacy_udp_interface_key(iface: &InterfaceRecord) -> Option<String> {
        if iface.kind != "udp" {
            return None;
        }
        if let Some(name) = iface.name.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            return Some(name.to_string());
        }
        let host = iface.host.as_deref()?.trim();
        let port = iface.port?;
        Some(format!("{host}:{port}"))
    }

    fn udp_record_is_hot_apply_safe(iface: &InterfaceRecord) -> bool {
        if iface.kind != "udp" || iface.host.as_deref().map(str::trim).is_none_or(str::is_empty) {
            return false;
        }
        if iface.port.is_none() || Self::interface_setting_str(iface, "device").is_some() {
            return false;
        }
        let target_host = Self::interface_setting_str(iface, "target_host")
            .or_else(|| Self::interface_setting_str(iface, "forward_ip"));
        let target_port = Self::interface_setting_u64(iface, "target_port")
            .or_else(|| Self::interface_setting_u64(iface, "forward_port"));
        if target_host.is_some() ^ target_port.is_some() {
            return false;
        }
        if target_port.is_some_and(|value| u16::try_from(value).is_err()) {
            return false;
        }
        !Self::host_is_multicast(target_host)
    }

    fn legacy_udp_bind_addr(iface: &InterfaceRecord) -> Option<String> {
        if iface.kind != "udp" {
            return None;
        }
        let host = iface.host.as_deref()?.trim();
        let port = iface.port?;
        Some(format!("{host}:{port}"))
    }

    fn legacy_tcp_server_bind_addr(iface: &InterfaceRecord) -> Option<String> {
        if iface.kind != "tcp_server" {
            return None;
        }
        let host = iface.host.as_deref()?.trim();
        let port = iface.port?;
        Some(Self::format_endpoint(Self::tcp_server_hot_apply_host(host), port))
    }

    fn interface_setting<'a>(iface: &'a InterfaceRecord, key: &str) -> Option<&'a JsonValue> {
        iface.settings.as_ref()?.as_object()?.get(key)
    }

    fn interface_setting_str<'a>(iface: &'a InterfaceRecord, key: &str) -> Option<&'a str> {
        Self::interface_setting(iface, key)?.as_str().map(str::trim).filter(|value| !value.is_empty())
    }

    fn interface_setting_u64(iface: &InterfaceRecord, key: &str) -> Option<u64> {
        Self::interface_setting(iface, key)?.as_u64()
    }

    fn interface_setting_bool(iface: &InterfaceRecord, key: &str) -> Option<bool> {
        Self::interface_setting(iface, key)?.as_bool()
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
        if host.eq_ignore_ascii_case("localhost") { "127.0.0.1" } else { host }
    }

    fn host_is_multicast(host: Option<&str>) -> bool {
        host.and_then(|value| value.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_multicast())
    }
}
