use super::TcpServerSelection;
use rns_transport::iface::tcp_client::TcpSocketTuning;
use rns_transport::iface::tcp_server::{FastFlapPolicy, TcpServer};
use std::sync::Arc;

pub(super) fn build_selected_tcp_server_adapter(
    addr: String,
    iface_manager: Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    selected_tcp_server: &TcpServerSelection,
) -> TcpServer {
    let mut server = selected_tcp_server
        .client_mtu
        .map(|mtu| TcpServer::new(addr.clone(), iface_manager.clone()).with_client_mtu(mtu))
        .unwrap_or_else(|| TcpServer::new(addr, iface_manager));
    server = server.with_prefer_ipv6(selected_tcp_server.prefer_ipv6);
    if let Some(bitrate_bps) = selected_tcp_server.client_forced_bitrate_bps {
        server = server.with_client_forced_bitrate(bitrate_bps);
    }
    if selected_tcp_server.kind == "backbone" {
        let fast_flap_policy = selected_fast_flap_policy(selected_tcp_server);
        server = server
            .with_client_socket_tuning(TcpSocketTuning::backbone())
            .with_backbone_client_liveness()
            .with_fast_flapping(
                fast_flap_policy.enabled,
                fast_flap_policy.threshold,
                fast_flap_policy.grace,
                fast_flap_policy.expiry,
            );
    } else if selected_tcp_server.kind == "tcp_server" && selected_tcp_server.i2p_tunneled {
        server = server.with_client_socket_tuning(TcpSocketTuning::i2p_tunneled());
    }
    server
}

pub(super) fn selected_fast_flap_policy(selected_tcp_server: &TcpServerSelection) -> FastFlapPolicy {
    FastFlapPolicy {
        enabled: selected_tcp_server.block_fast_flapping.unwrap_or(true),
        threshold: std::time::Duration::from_secs_f64(
            selected_tcp_server.fast_flapping_threshold.unwrap_or(20.0).max(0.0),
        ),
        grace: selected_tcp_server.fast_flapping_grace.unwrap_or(5),
        expiry: std::time::Duration::from_secs_f64(
            selected_tcp_server.fast_flapping_block_time.unwrap_or(12.0 * 60.0).max(0.0) * 60.0,
        ),
    }
}
