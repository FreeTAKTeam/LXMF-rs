pub(super) fn configure_startup_rpc_token_auth(args: &Args, daemon: &RpcDaemon) {
    let token_args = [
        args.rpc_token_issuer.as_ref().map(|_| "--rpc-token-issuer"),
        args.rpc_token_audience.as_ref().map(|_| "--rpc-token-audience"),
        args.rpc_token_secret_env.as_ref().map(|_| "--rpc-token-secret-env"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if token_args.is_empty() {
        return;
    }
    let issuer = args
        .rpc_token_issuer
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("--rpc-token-issuer is required for startup token auth"));
    let audience = args
        .rpc_token_audience
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("--rpc-token-audience is required for startup token auth"));
    let secret_env = args
        .rpc_token_secret_env
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("--rpc-token-secret-env is required for startup token auth"));
    let shared_secret = std::env::var(secret_env)
        .unwrap_or_else(|_| panic!("startup token auth secret env var {secret_env} is not set"));
    if shared_secret.trim().is_empty() {
        panic!("startup token auth secret env var {secret_env} is empty");
    }

    daemon
        .configure_remote_token_auth_for_startup(
            issuer,
            audience,
            shared_secret,
            args.rpc_token_jti_ttl_ms,
            args.rpc_token_clock_skew_ms,
        )
        .unwrap_or_else(|err| panic!("invalid startup token auth configuration: {}", err.message));
}

fn interface_record_from_config(iface: &InterfaceConfig) -> InterfaceRecord {
    InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: iface.enabled(),
        host: iface.host.clone(),
        port: iface.port,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    }
}

#[derive(Debug, Default)]
pub(super) struct TcpServerSelection {
    pub(super) bind_addr: Option<String>,
    pub(super) selected_index: Option<usize>,
    pub(super) kind: String,
    pub(super) client_mtu: Option<usize>,
    pub(super) client_forced_bitrate_bps: Option<u64>,
    pub(super) prefer_ipv6: bool,
    pub(super) i2p_tunneled: bool,
    pub(super) local_attach_addr: Option<String>,
    pub(super) local_attach_index: Option<usize>,
}

pub(super) fn select_tcp_server_bind(
    args: &Args,
    daemon_config: Option<&DaemonConfig>,
) -> Result<TcpServerSelection, String> {
    if let Some(addr) = args.transport.as_ref() {
        return Ok(TcpServerSelection {
            bind_addr: Some(addr.clone()),
            selected_index: None,
            kind: "tcp_server".to_string(),
            client_mtu: None,
            client_forced_bitrate_bps: None,
            prefer_ipv6: false,
            i2p_tunneled: false,
            local_attach_addr: None,
            local_attach_index: None,
        });
    }

    let Some(config) = daemon_config else {
        return Ok(TcpServerSelection::default());
    };

    let mut matches = Vec::new();
    for (index, iface) in config.interfaces.iter().enumerate() {
        if !iface.enabled() || !is_tcp_listener_interface(iface) {
            continue;
        }
        let Some(port) = iface.port else {
            continue;
        };
        let host = tcp_listener_bind_host(iface)
            .map_err(|err| format!("interfaces[{index}] {err}"))?;
        matches.push(TcpListenerMatch {
            index,
            kind: iface.kind.clone(),
            client_mtu: iface.mtu,
            client_forced_bitrate_bps: (iface.kind == "local")
                .then_some(iface.force_shared_instance_bitrate)
                .flatten(),
            prefer_ipv6: iface.prefer_ipv6.unwrap_or(false),
            i2p_tunneled: iface.i2p_tunneled.unwrap_or(false),
            bind_addr: tcp_bind_addr(host.as_str(), port),
            synthetic_shared_tcp_local: is_synthetic_shared_tcp_local(iface),
        });
    }

    if matches.len() > 1 && matches.iter().any(|entry| !entry.synthetic_shared_tcp_local) {
        matches.retain(|entry| !entry.synthetic_shared_tcp_local);
    }

    if matches.len() > 1 {
        return Err(format!(
            "multiple enabled TCP listener interfaces configured without --transport override: {}",
            matches
                .iter()
                .map(|entry| entry.bind_addr.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let Some(selected) = matches.into_iter().next() else {
        return Ok(TcpServerSelection::default());
    };

    if selected.kind == "local" && tcp_bind_addr_is_in_use(&selected.bind_addr) {
        return Ok(TcpServerSelection {
            bind_addr: None,
            selected_index: None,
            kind: selected.kind,
            client_mtu: selected.client_mtu,
            client_forced_bitrate_bps: selected.client_forced_bitrate_bps,
            prefer_ipv6: selected.prefer_ipv6,
            i2p_tunneled: selected.i2p_tunneled,
            local_attach_addr: Some(selected.bind_addr),
            local_attach_index: Some(selected.index),
        });
    }

    Ok(TcpServerSelection {
        bind_addr: Some(selected.bind_addr),
        selected_index: Some(selected.index),
        kind: selected.kind,
        client_mtu: selected.client_mtu,
        client_forced_bitrate_bps: selected.client_forced_bitrate_bps,
        prefer_ipv6: selected.prefer_ipv6,
        i2p_tunneled: selected.i2p_tunneled,
        local_attach_addr: None,
        local_attach_index: None,
    })
}

#[derive(Debug)]
struct TcpListenerMatch {
    index: usize,
    kind: String,
    client_mtu: Option<usize>,
    client_forced_bitrate_bps: Option<u64>,
    prefer_ipv6: bool,
    i2p_tunneled: bool,
    bind_addr: String,
    synthetic_shared_tcp_local: bool,
}

fn is_tcp_listener_interface(iface: &InterfaceConfig) -> bool {
    match iface.kind.as_str() {
        "tcp_server" | "backbone" => true,
        "local" => iface.shared_instance_type.as_deref() != Some("unix"),
        _ => false,
    }
}

fn is_synthetic_shared_tcp_local(iface: &InterfaceConfig) -> bool {
    iface.synthetic_shared_instance
        && iface.kind == "local"
        && iface.shared_instance_type.as_deref() != Some("unix")
}

fn tcp_listener_bind_host(iface: &InterfaceConfig) -> Result<String, String> {
    if let Some(host) = iface.host.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(host.to_string());
    }
    let Some(device) = iface.device.as_deref().map(str::trim).filter(|value| !value.is_empty())
    else {
        return Ok("0.0.0.0".to_string());
    };
    resolve_tcp_listener_device_bind_host(device, iface.prefer_ipv6.unwrap_or(false))
}

fn tcp_bind_addr(host: &str, port: u16) -> String {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return std::net::SocketAddr::new(ip, port).to_string();
    }
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn resolve_tcp_listener_device_bind_host(device: &str, prefer_ipv6: bool) -> Result<String, String> {
    let interfaces = if_addrs::get_if_addrs()
        .map_err(|err| format!("failed to inspect network interfaces for device {device}: {err}"))?;
    let candidates = interfaces
        .iter()
        .map(|iface| (iface.name.as_str(), iface.ip(), iface.is_oper_up()))
        .collect::<Vec<_>>();
    select_tcp_listener_device_ip(device, prefer_ipv6, &candidates)
        .map(|addr| addr.to_string())
}

pub(crate) fn select_tcp_listener_device_ip(
    device: &str,
    prefer_ipv6: bool,
    candidates: &[(&str, std::net::IpAddr, bool)],
) -> Result<std::net::IpAddr, String> {
    let mut matches = candidates
        .iter()
        .copied()
        .filter(|(name, _, is_up)| *name == device && *is_up)
        .map(|(_, ip, _)| ip)
        .filter(|ip| !ip.is_unspecified())
        .filter(|ip| !ip.is_loopback() || device.starts_with("lo"))
        .collect::<Vec<_>>();
    matches.sort_by_key(|ip| match (prefer_ipv6, ip) {
        (true, std::net::IpAddr::V6(_)) | (false, std::net::IpAddr::V4(_)) => 0,
        _ => 1,
    });
    matches.into_iter().next().ok_or_else(|| {
        format!(
            "device {device} did not resolve to an operational bindable interface address"
        )
    })
}

fn tcp_bind_addr_is_in_use(bind_addr: &str) -> bool {
    match std::net::TcpListener::bind(bind_addr) {
        Ok(listener) => {
            drop(listener);
            false
        }
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => true,
        Err(_) => false,
    }
}

pub(super) fn mark_interface_startup_status(
    record: &mut InterfaceRecord,
    status: &str,
    startup_error: Option<&str>,
    runtime_iface: Option<&str>,
) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("startup_status".to_string(), JsonValue::String(status.to_string()));
        if let Some(startup_error) = startup_error {
            runtime
                .insert("startup_error".to_string(), JsonValue::String(startup_error.to_string()));
        } else {
            runtime.remove("startup_error");
        }
        if let Some(runtime_iface) = runtime_iface {
            runtime.insert("iface".to_string(), JsonValue::String(runtime_iface.to_string()));
        } else {
            runtime.remove("iface");
        }
    });
}

fn parse_hex_list_env(key: &str) -> Vec<String> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn env_capabilities(key: &str) -> Option<Vec<String>> {
    std::env::var(key)
        .ok()
        .map(|value| {
            let values = value
                .split([',', ';', ' ', '\t', '\r', '\n'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            normalize_capabilities(&values)
        })
        .filter(|capabilities| !capabilities.is_empty())
}

fn env_u64(key: &str) -> Result<Option<u64>, &'static str> {
    let value = match std::env::var(key) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let parsed = value.trim().parse::<u64>().map_err(|_| "invalid u64 value")?;
    Ok((parsed > 0).then_some(parsed))
}

fn spawn_destination_announce_scheduler(
    transport: Arc<Transport>,
    destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    app_data: Option<Vec<u8>>,
    interval_secs: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            transport.send_announce(&destination, app_data.as_deref()).await;
        }
    });
}

fn spawn_bridge_announce_scheduler(bridge: Arc<TransportBridge>, interval_secs: u64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            let _ = bridge.announce_now();
        }
    });
}

fn spawn_bridge_propagation_announce_scheduler(bridge: Arc<TransportBridge>, interval_secs: u64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            let _ = bridge.announce_propagation_now();
        }
    });
}

fn encode_propagation_node_app_data(
    display_name: Option<&str>,
    config: PropagationNodeAnnounceConfig,
) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    encode_python_propagation_node_app_data(
        display_name,
        PropagationNodeAnnounceConfig {
            timebase: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            ..config
        },
    )
}

pub(super) fn mark_interface_runtime_managed(record: &mut InterfaceRecord, managed_by: &str) {
    with_interface_runtime_metadata(record, |runtime| {
        runtime.insert("managed_by".to_string(), JsonValue::String(managed_by.to_string()));
    });
}

pub(super) fn with_interface_runtime_metadata(
    record: &mut InterfaceRecord,
    update: impl FnOnce(&mut JsonMap<String, JsonValue>),
) {
    let mut settings = match record.settings.take() {
        Some(JsonValue::Object(existing)) => existing,
        Some(other) => {
            let mut wrapped = JsonMap::new();
            wrapped.insert("configured_settings".to_string(), other);
            wrapped
        }
        None => JsonMap::new(),
    };

    let runtime_value =
        settings.entry("_runtime".to_string()).or_insert_with(|| JsonValue::Object(JsonMap::new()));
    let runtime = match runtime_value {
        JsonValue::Object(existing) => existing,
        other => {
            *other = JsonValue::Object(JsonMap::new());
            match other {
                JsonValue::Object(existing) => existing,
                _ => unreachable!("runtime metadata must be an object"),
            }
        }
    };
    update(runtime);
    record.settings = Some(JsonValue::Object(settings));
}

pub(super) fn mark_interface_runtime_fields(
    record: &mut InterfaceRecord,
    runtime_status: &str,
    reconnect_attempts: u64,
) {
    let mut settings = match record.settings.take() {
        Some(JsonValue::Object(existing)) => existing,
        Some(other) => {
            let mut wrapped = JsonMap::new();
            wrapped.insert("configured_settings".to_string(), other);
            wrapped
        }
        None => JsonMap::new(),
    };

    let mut runtime = match settings.remove("_runtime") {
        Some(JsonValue::Object(existing)) => existing,
        _ => JsonMap::new(),
    };

    runtime.insert("runtime_status".to_string(), JsonValue::String(runtime_status.to_string()));
    runtime.insert("reconnect_attempts".to_string(), JsonValue::Number(reconnect_attempts.into()));
    settings.insert("_runtime".to_string(), JsonValue::Object(runtime));
    record.settings = Some(JsonValue::Object(settings));
}

pub(super) fn enforce_startup_policy(
    strict_interface_startup: bool,
    startup_failures: &[InterfaceStartupFailure],
) -> Result<(), String> {
    if !strict_interface_startup || startup_failures.is_empty() {
        return Ok(());
    }

    let details = startup_failures
        .iter()
        .map(|failure| format!("{} ({}): {}", failure.label, failure.kind, failure.error))
        .collect::<Vec<_>>()
        .join("; ");
    Err(format!(
        "strict interface startup policy rejected {} interface(s): {}",
        startup_failures.len(),
        details
    ))
}

async fn strict_tcp_client_preflight(endpoint: &str) -> Result<(), String> {
    let connect = timeout(Duration::from_secs(2), TcpStream::connect(endpoint))
        .await
        .map_err(|_| format!("tcp_client preflight connect timed out endpoint={endpoint}"))?;
    connect
        .map(|_| ())
        .map_err(|err| format!("tcp_client preflight connect failed endpoint={endpoint} err={err}"))
}
