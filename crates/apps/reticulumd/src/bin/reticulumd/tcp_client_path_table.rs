use reticulum_daemon::config::InterfaceConfig;
use rns_rpc::InterfaceRecord;
use rns_transport::iface::TcpClientPathTableMetadata;

pub(crate) fn from_config(interface: &InterfaceConfig) -> Option<TcpClientPathTableMetadata> {
    from_parts(
        &interface.kind,
        interface.name.as_deref(),
        interface.host.as_deref().or(interface.target_host.as_deref()),
        interface.port.or(interface.target_port),
    )
}

pub(crate) fn from_record(interface: &InterfaceRecord) -> Option<TcpClientPathTableMetadata> {
    let target_host = interface
        .host
        .as_deref()
        .or_else(|| setting_str(interface, "target_host"))
        .or_else(|| setting_str(interface, "host"));
    let target_port = interface
        .port
        .or_else(|| setting_u16(interface, "target_port"))
        .or_else(|| setting_u16(interface, "port"));

    from_parts(&interface.kind, interface.name.as_deref(), target_host, target_port)
}

fn from_parts(
    kind: &str,
    name: Option<&str>,
    target_host: Option<&str>,
    target_port: Option<u16>,
) -> Option<TcpClientPathTableMetadata> {
    if kind != "tcp_client" {
        return None;
    }
    let name = name?.trim();
    let target_host = target_host?;
    let target_port = target_port?;
    if name.is_empty() || target_host.trim().is_empty() {
        return None;
    }

    Some(TcpClientPathTableMetadata {
        name: name.to_owned(),
        target_host: target_host.to_owned(),
        target_port,
    })
}

fn setting_str<'a>(interface: &'a InterfaceRecord, key: &str) -> Option<&'a str> {
    interface.settings.as_ref()?.as_object()?.get(key)?.as_str()
}

fn setting_u16(interface: &InterfaceRecord, key: &str) -> Option<u16> {
    u16::try_from(interface.settings.as_ref()?.as_object()?.get(key)?.as_u64()?).ok()
}

pub(crate) fn render(metadata: &TcpClientPathTableMetadata) -> String {
    let target_host = if metadata.target_host.contains(':') {
        format!("[{}]", metadata.target_host)
    } else {
        metadata.target_host.clone()
    };
    format!("TCPInterface[{}/{}:{}]", metadata.name, target_host, metadata.target_port)
}

#[cfg(test)]
mod tests {
    use super::{from_parts, render};
    use rns_transport::iface::TcpClientPathTableMetadata;

    #[test]
    fn renders_tcp_client_name_and_ipv4_target() {
        let metadata = from_parts("tcp_client", Some("relay"), Some("127.0.0.1"), Some(4242))
            .expect("complete TCP client metadata");

        assert_eq!(render(&metadata), "TCPInterface[relay/127.0.0.1:4242]");
    }

    #[test]
    fn renders_ipv6_target_in_brackets() {
        let metadata = TcpClientPathTableMetadata {
            name: "relay".to_owned(),
            target_host: "2001:db8::1".to_owned(),
            target_port: 4242,
        };

        assert_eq!(render(&metadata), "TCPInterface[relay/[2001:db8::1]:4242]");
    }

    #[test]
    fn omits_metadata_for_unsupported_or_incomplete_interfaces() {
        assert!(from_parts("tcp_server", Some("relay"), Some("127.0.0.1"), Some(4242)).is_none());
        assert!(from_parts("tcp_client", None, Some("127.0.0.1"), Some(4242)).is_none());
        assert!(from_parts("tcp_client", Some("relay"), None, Some(4242)).is_none());
    }
}
