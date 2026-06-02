use super::lora_state::ensure_state_file;
use reticulum_daemon::config::InterfaceConfig;

pub(crate) fn startup(iface: &InterfaceConfig) -> Result<(), String> {
    let path = iface
        .state_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "lora.state_path is required".to_string())?;

    let state = ensure_state_file(path)?;

    log::info!(
        "[daemon] lora configured name={} region={} state_path={} duty_cycle_debt_ms={} debt_elapsed_ms={} uncertain={}",
        iface.name.as_deref().unwrap_or("<unnamed>"),
        iface.region.as_deref().unwrap_or("<unset>"),
        path,
        state.duty_cycle_debt_ms,
        state.debt_elapsed_ms,
        state.uncertain
    );

    if state.duty_cycle_debt_ms > 0 {
        log::info!(
            "[daemon] lora compliance gate name={} debt_remaining_ms={} tx_allowed_after_additional_wait_ms={}",
            iface.name.as_deref().unwrap_or("<unnamed>"),
            state.duty_cycle_debt_ms,
            state.duty_cycle_debt_ms
        );
    }

    Ok(())
}
