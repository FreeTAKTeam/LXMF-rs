//! The AutoInterface driver lives in `rns_transport::iface::auto_runtime`.
//! This module keeps the daemon's names for it and owns the one thing that is
//! the daemon's: turning an ini stanza into a typed plan.

use reticulum_daemon::config::InterfaceConfig;

use rns_transport::iface::auto::{
    AutoDiscoveryScope, AutoInterfaceConfig, AutoInterfaceDeviceFilter, MulticastAddressType,
};

pub(crate) use rns_transport::iface::auto_runtime::{
    discovery_runtime_summary_json, AutoInterfaceTransportRuntime, AutoRuntimePlan,
    AutoRuntimeStatusHandle,
};

pub(crate) fn build_native_startup_plan(
    iface: &InterfaceConfig,
) -> Result<AutoRuntimePlan, String> {
    let config = auto_config(iface)?;
    let filter = AutoInterfaceDeviceFilter {
        allowed: iface.devices.clone().unwrap_or_default(),
        ignored: iface.ignored_devices.clone().unwrap_or_default(),
    };
    AutoRuntimePlan::from_system(config, filter)
}

fn auto_config(iface: &InterfaceConfig) -> Result<AutoInterfaceConfig, String> {
    Ok(AutoInterfaceConfig {
        group_id: iface.group_id.clone().unwrap_or_else(|| "reticulum".to_string()),
        discovery_scope: AutoDiscoveryScope::parse(
            iface.discovery_scope.as_deref().unwrap_or("link"),
        )
        .ok()
        .flatten()
        .ok_or_else(|| "auto discovery_scope was not normalized".to_string())?,
        multicast_address_type: MulticastAddressType::parse(
            iface.multicast_address_type.as_deref().unwrap_or("temporary"),
        )
        .ok()
        .flatten()
        .ok_or_else(|| "auto multicast_address_type was not normalized".to_string())?,
        discovery_port: iface.discovery_port.unwrap_or(29_716),
        data_port: iface.data_port.unwrap_or(42_671),
    })
}
