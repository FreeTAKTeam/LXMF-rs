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
