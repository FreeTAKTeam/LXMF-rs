use super::{
    mark_interface_startup_status, tcp_startup, InterfaceStartupFailure, TcpServerSelection,
};
use crate::interfaces::common::interface_label;
use reticulum_daemon::config::InterfaceConfig;
use rns_rpc::InterfaceRecord;
use rns_transport::hash::AddressHash;
use rns_transport::iface::tcp_client::TcpSocketTuning;
use rns_transport::iface::tcp_server::{FastFlapPolicy, TcpListenerRuntimeStatusHandle, TcpServer};
use rns_transport::iface::InterfaceManager;
use std::sync::Arc;

pub(super) async fn startup_selected_tcp_server(
    selection: &TcpServerSelection,
    configured: Option<&InterfaceConfig>,
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    strict: bool,
    record: Option<&mut InterfaceRecord>,
    startup_successes: &mut usize,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) -> Option<(AddressHash, TcpListenerRuntimeStatusHandle)> {
    let addr = selection.bind_addr.as_ref()?;
    let server = build_selected_tcp_server_adapter(addr.clone(), manager.clone(), selection);
    let (server, startup_result) = if strict {
        let (server, result) = server.with_startup_result();
        (server, Some(result))
    } else {
        (server, None)
    };
    let runtime_status = server.runtime_status_handle();
    let started = {
        let mut manager = manager.lock().await;
        match selection.selected_index {
            Some(_) => match configured {
                Some(iface) => crate::interfaces::common::spawn_configured(
                    &mut manager,
                    server,
                    TcpServer::spawn,
                    rns_transport::iface::IfaceRole::Unicast,
                    iface,
                ),
                None => Err(rns_transport::error::RnsError::InvalidArgument),
            },
            None => Ok(manager.spawn(server, TcpServer::spawn)),
        }
    };
    let label = selection
        .selected_index
        .and_then(|index| configured.map(|iface| interface_label(iface, index)))
        .unwrap_or_else(|| selection.kind.clone());
    match started {
        Ok(active_iface) => {
            log::info!("[daemon] {} enabled iface={} bind={}", selection.kind, active_iface, addr);
            if let Some(startup_result) = startup_result {
                tcp_startup::record_initial_bind_result(
                    startup_result.await,
                    label,
                    selection.kind.clone(),
                    startup_successes,
                    startup_failures,
                );
            } else {
                *startup_successes += 1;
            }
            Some((active_iface, runtime_status))
        }
        Err(error) => {
            let error = format!("interface runtime policy could not be installed: {error}");
            log::error!("[daemon] interface startup rejected name={label} err={error}");
            if let Some(record) = record {
                mark_interface_startup_status(record, "failed", Some(&error), None);
            }
            startup_failures.push(InterfaceStartupFailure {
                label,
                kind: selection.kind.clone(),
                error,
            });
            None
        }
    }
}

pub(super) fn build_selected_tcp_server_adapter(
    addr: String,
    iface_manager: Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    selected_tcp_server: &TcpServerSelection,
) -> TcpServer {
    let mut server = selected_tcp_server
        .client_mtu
        .map(|mtu| TcpServer::new(addr.clone(), iface_manager.clone()).with_client_mtu(mtu))
        .unwrap_or_else(|| TcpServer::new(addr, iface_manager));
    server = server
        .with_prefer_ipv6(selected_tcp_server.prefer_ipv6)
        .with_plaintext_shared_clients(selected_tcp_server.native_shared_clients);
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

pub(super) fn selected_fast_flap_policy(
    selected_tcp_server: &TcpServerSelection,
) -> FastFlapPolicy {
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
