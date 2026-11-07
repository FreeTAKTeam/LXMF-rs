use reticulum_daemon::config::InterfaceConfig;

pub(crate) fn interface_label(iface: &InterfaceConfig, index: usize) -> String {
    iface
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{}[{index}]", iface.kind))
}

use rns_transport::hash::AddressHash;
use rns_transport::iface::{
    IfaceRole, Interface, InterfaceContext, InterfaceManager, InterfaceMode,
};
use std::sync::{Arc, Mutex};

pub(crate) fn spawn_configured<T: Interface, F, R>(
    manager: &mut InterfaceManager,
    inner: T,
    worker: F,
    role: IfaceRole,
    iface: &InterfaceConfig,
) -> Result<AddressHash, rns_transport::error::RnsError>
where
    F: FnOnce(InterfaceContext<T>) -> R,
    R: std::future::Future<Output = ()> + Send + 'static,
{
    spawn_configured_with_handle(manager, inner, worker, role, iface).map(|(address, _)| address)
}

pub(crate) fn spawn_configured_with_handle<T: Interface, F, R>(
    manager: &mut InterfaceManager,
    inner: T,
    worker: F,
    role: IfaceRole,
    iface: &InterfaceConfig,
) -> Result<(AddressHash, Arc<Mutex<T>>), rns_transport::error::RnsError>
where
    F: FnOnce(InterfaceContext<T>) -> R,
    R: std::future::Future<Output = ()> + Send + 'static,
{
    manager.try_spawn_as_with_mode_and_handle(
        inner,
        worker,
        role,
        iface.interface_mode().unwrap_or(InterfaceMode::Full),
        |manager, address| apply_runtime_config(manager, address, iface),
    )
}

/// The owned wrapper cannot perform carrier I/O until the transport has
/// committed shared-client mode. Dropping startup closes the gate and cancels
/// the context; stopping its manager tree also interrupts the gated wrapper.
pub(crate) async fn spawn_configured_attachment<T: Interface + Send + 'static, F, R>(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: &rns_transport::transport::Transport,
    inner: T,
    worker: F,
    iface: &InterfaceConfig,
) -> Result<AddressHash, rns_transport::error::RnsError>
where
    F: FnOnce(InterfaceContext<T>) -> R + Send + 'static,
    R: std::future::Future<Output = ()> + Send + 'static,
{
    let (address, activate, cancel) = manager.lock().await.try_spawn_deferred_as_with_mode(
        inner,
        worker,
        IfaceRole::Unicast,
        iface.interface_mode().unwrap_or(InterfaceMode::Full),
        |manager, address| apply_runtime_config(manager, address, iface),
    )?;
    if let Err(error) = transport.activate_shared_instance_interface(activate, &cancel).await {
        cancel.cancel();
        manager.lock().await.stop_interface(address);
        return Err(error);
    }
    Ok(address)
}

pub(crate) fn apply_runtime_config(
    manager: &mut rns_transport::iface::InterfaceManager,
    address: AddressHash,
    iface: &InterfaceConfig,
) -> Result<(), rns_transport::error::RnsError> {
    let mut shared_config = shared_config(iface);
    if iface.synthetic_shared_instance && matches!(iface.kind.as_str(), "local" | "local_client") {
        // Native automatic shared-instance connections are plaintext. Explicit
        // Local records retain their configured authentication contract.
        shared_config.ifac_size = None;
        shared_config.network_name = None;
        shared_config.passphrase = None;
    }
    if !manager.try_set_shared_config(address, shared_config)? {
        return Err(rns_transport::error::RnsError::InvalidArgument);
    }
    manager.set_mode(address, iface.interface_mode().unwrap_or(InterfaceMode::Full));
    manager.set_shared_instance(address, matches!(iface.kind.as_str(), "local" | "local_client"));
    manager.set_gravity(address, iface.interface_gravity());
    manager.set_outgoing(address, iface.outgoing());
    manager.set_display_name(
        address,
        iface.name.as_deref().map(str::trim).filter(|name| !name.is_empty()).map(ToOwned::to_owned),
    );
    manager.set_tcp_client_path_table_metadata(
        address,
        crate::tcp_client_path_table::from_config(iface),
    );
    if iface.bitrate.is_some() || iface.announce_cap.is_some() {
        let (current_bitrate, current_announce_cap) =
            manager.announce_pacing(&address).unwrap_or((62_500, 2));
        let bitrate = iface.bitrate.unwrap_or(current_bitrate);
        let announce_cap = iface.announce_cap.unwrap_or(current_announce_cap);
        manager.set_announce_pacing(address, bitrate, announce_cap);
    }
    Ok(())
}

fn shared_config(iface: &InterfaceConfig) -> rns_transport::iface::InterfaceSharedConfig {
    rns_transport::iface::InterfaceSharedConfig {
        announce_rate_target: iface.announce_rate_target,
        announce_rate_grace: iface.announce_rate_grace,
        announce_rate_penalty: iface.announce_rate_penalty,
        bootstrap_only: iface.bootstrap_only,
        announces_from_internal: iface.announces_from_internal,
        announces_to_internal: iface.announces_to_internal,
        ifac_size: iface.ifac_size,
        network_name: iface.ifac_network_name().cloned(),
        passphrase: iface.ifac_passphrase().cloned(),
        ingress_control: iface.ingress_control,
        egress_control: iface.egress_control,
        ic_max_held_announces: iface.ic_max_held_announces,
        ic_burst_hold: iface.ic_burst_hold,
        ic_burst_freq_new: iface.ic_burst_freq_new,
        ic_burst_freq: iface.ic_burst_freq,
        ic_pr_burst_freq_new: iface.ic_pr_burst_freq_new,
        ic_pr_burst_freq: iface.ic_pr_burst_freq,
        ec_pr_freq: iface.ec_pr_freq,
        ic_new_time: iface.ic_new_time,
        ic_burst_penalty: iface.ic_burst_penalty,
        ic_held_release_interval: iface.ic_held_release_interval,
        discoverable: iface.discoverable,
        announce_interval: iface.discovery_announce_interval_secs(),
        discovery_stamp_value: iface.discovery_stamp_value,
        discovery_name: iface.discovery_name.clone(),
        discovery_lxmf_address: iface.discovery_lxmf_address.clone(),
        discovery_encrypt: iface.discovery_encrypt,
        reachable_on: iface.reachable_on.clone(),
        publish_ifac: iface.publish_ifac,
        latitude: iface.latitude,
        longitude: iface.longitude,
        height: iface.height,
        discovery_frequency: iface.discovery_frequency,
        discovery_bandwidth: iface.discovery_bandwidth,
        discovery_modulation: iface.discovery_modulation,
    }
}

#[cfg(test)]
#[path = "tests/common.rs"]
mod tests;
