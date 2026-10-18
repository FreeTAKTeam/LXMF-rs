use super::*;
use reticulum_daemon::config::DaemonConfig;
use serde_json::{json, Value};
use std::time::Duration;

const MANAGEMENT_REPLY_TIMEOUT: Duration = Duration::from_secs(20);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) struct NamedInterfaceAction<'a> {
    operation: &'a str,
    name: &'a str,
    configured: Option<InterfaceRecord>,
}

impl<'a> NamedInterfaceAction<'a> {
    pub(super) fn new(
        operation: &'a str,
        name: &'a str,
        configured: Option<InterfaceRecord>,
    ) -> Self {
        Self { operation, name, configured }
    }
}

pub(super) fn dispatch(
    bridge: &InterfaceHotApplyBridge,
    operation: &str,
    name: &str,
) -> io::Result<Value> {
    if !bridge.management_enabled {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "interface management is disabled by configuration",
        ));
    }
    if !matches!(operation, "attach" | "detach" | "reload") || name.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid interface operation or name",
        ));
    }
    let configured = if operation == "detach" {
        None
    } else {
        Some(load_named_config_record(bridge.config_path.as_deref(), name)?)
    };
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    bridge
        .tx
        .try_send(InterfaceHotApplyCommand::Manage {
            operation: operation.to_string(),
            name: name.to_string(),
            configured,
            reply,
        })
        .map_err(|error| match error {
            TrySendError::Full(_) => {
                io::Error::new(io::ErrorKind::WouldBlock, "interface mutation queue is full")
            }
            TrySendError::Closed(_) => io::Error::new(
                io::ErrorKind::BrokenPipe,
                "interface mutation worker is not running",
            ),
        })?;
    receiver.recv_timeout(MANAGEMENT_REPLY_TIMEOUT).map_err(|error| match error {
        std::sync::mpsc::RecvTimeoutError::Timeout => {
            io::Error::new(io::ErrorKind::TimedOut, "interface management worker timed out")
        }
        std::sync::mpsc::RecvTimeoutError::Disconnected => {
            io::Error::new(io::ErrorKind::BrokenPipe, "interface management worker stopped")
        }
    })?
}

fn load_named_config_record(
    path: Option<&std::path::Path>,
    name: &str,
) -> io::Result<InterfaceRecord> {
    let path = path.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "daemon has no interface configuration file")
    })?;
    let config = DaemonConfig::from_path(path)?;
    let mut matches = config.interfaces.iter().filter(|iface| iface.name.as_deref() == Some(name));
    let iface = matches.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, format!("interface {name:?} is not configured"))
    })?;
    if matches.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("multiple configured interfaces are named {name:?}"),
        ));
    }
    let mut record = crate::bootstrap::interface_record_from_config(iface);
    if !is_manageable(&record) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("interface {name:?} kind {:?} requires daemon restart", record.kind),
        ));
    }
    record.enabled = true;
    validate_hot_apply_ifac_configuration(std::slice::from_ref(&record))?;
    Ok(record)
}

fn is_manageable(record: &InterfaceRecord) -> bool {
    matches!(record.kind.as_str(), "tcp_client" | "tcp_server" | "udp" | "pipe")
        && record.name.as_deref().is_some_and(|name| !name.trim().is_empty())
}

pub(super) async fn apply(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    action: NamedInterfaceAction<'_>,
    transport: Option<&Arc<Transport>>,
    refreshes: &HotApplyRuntimeRefreshes,
    daemon: Option<&Weak<RpcDaemon>>,
) -> io::Result<Value> {
    let NamedInterfaceAction { operation, name, configured } = action;
    let daemon = daemon.and_then(Weak::upgrade).ok_or_else(|| {
        io::Error::new(io::ErrorKind::BrokenPipe, "interface management daemon is unavailable")
    })?;
    let current = daemon.interface_records();
    let index = named_record_index(&current, name)?;
    if managed.values().any(|active| {
        !active.origin.configuration_owned() && active.record.name.as_deref() == Some(name)
    }) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "internal interface requires its native owner",
        ));
    }
    if index.is_some_and(|index| !is_manageable(&current[index])) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("interface {name:?} cannot be managed live"),
        ));
    }
    match operation {
        "attach" => {
            if managed.get(&configured_key(name)).is_some_and(|entry| entry.native.is_some()) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("interface {name:?} is already active"),
                ));
            }
            let record = configured.ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "configured interface is missing")
            })?;
            let next = with_named_record(&current, index, record);
            validate_hot_apply_uniqueness(&next)?;
            let mut applied = apply_hot_apply_interface_records(
                iface_manager,
                managed,
                next.clone(),
                transport,
                refreshes,
                Some(&Arc::downgrade(&daemon)),
            )
            .await;
            match wait_ready(managed.get(&configured_key(name))).await {
                Ok(()) => {
                    record_publication::committed_records(&mut applied, managed);
                    daemon.replace_interfaces(applied);
                    Ok(
                        json!({ "name": name, "operation": operation, "complete": true, "state": "active" }),
                    )
                }
                Err(error) => {
                    let mut rollback = current;
                    if let Some(index) = index {
                        rollback[index].enabled = false;
                    }
                    stop_named(
                        iface_manager,
                        managed,
                        name,
                        rollback.clone(),
                        transport,
                        refreshes,
                        &daemon,
                    )
                    .await?;
                    Err(error)
                }
            }
        }
        "detach" => {
            let current_managed = managed.get(&configured_key(name)).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, format!("interface {name:?} is not active"))
            })?;
            if current_managed.record.name.as_deref() != Some(name) {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "named interface is not active",
                ));
            }
            let mut next = current;
            match index {
                Some(index) => next[index].enabled = false,
                None => {
                    let mut record = current_managed.record.clone();
                    record.enabled = false;
                    next.push(record);
                }
            }
            stop_named(iface_manager, managed, name, next.clone(), transport, refreshes, &daemon)
                .await?;
            Ok(
                json!({ "name": name, "operation": operation, "complete": true, "state": "detached" }),
            )
        }
        "reload" => {
            let old = managed
                .get(&configured_key(name))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("interface {name:?} is not active"),
                    )
                })?
                .record
                .clone();
            let record = configured.ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "configured interface is missing")
            })?;
            let next = with_named_record(&current, index, record);
            validate_hot_apply_uniqueness(&next)?;
            let mut detached = current.clone();
            match index {
                Some(index) => detached[index].enabled = false,
                None => detached.push(InterfaceRecord { enabled: false, ..old.clone() }),
            }
            stop_named(
                iface_manager,
                managed,
                name,
                detached.clone(),
                transport,
                refreshes,
                &daemon,
            )
            .await?;
            let mut applied = apply_hot_apply_interface_records(
                iface_manager,
                managed,
                next.clone(),
                transport,
                refreshes,
                Some(&Arc::downgrade(&daemon)),
            )
            .await;
            match wait_ready(managed.get(&configured_key(name))).await {
                Ok(()) => {
                    record_publication::committed_records(&mut applied, managed);
                    daemon.replace_interfaces(applied);
                    Ok(
                        json!({ "name": name, "operation": operation, "complete": true, "state": "active" }),
                    )
                }
                Err(replacement_error) => {
                    stop_named(
                        iface_manager,
                        managed,
                        name,
                        detached.clone(),
                        transport,
                        refreshes,
                        &daemon,
                    )
                    .await?;
                    let restored = with_named_record(&current, index, old);
                    let mut restored_applied = apply_hot_apply_interface_records(
                        iface_manager,
                        managed,
                        restored.clone(),
                        transport,
                        refreshes,
                        Some(&Arc::downgrade(&daemon)),
                    )
                    .await;
                    match wait_ready(managed.get(&configured_key(name))).await {
                        Ok(()) => {
                            record_publication::committed_records(&mut restored_applied, managed);
                            daemon.replace_interfaces(restored_applied);
                            Err(io::Error::other(format!(
                                "reload of {name:?} failed: {replacement_error}; previous interface restored (state=active)"
                            )))
                        }
                        Err(rollback_error) => {
                            if let Err(error) = stop_named(
                                iface_manager,
                                managed,
                                name,
                                detached,
                                transport,
                                refreshes,
                                &daemon,
                            )
                            .await
                            {
                                log::warn!("[daemon] failed rollback cleanup name={name}: {error}");
                            }
                            Err(io::Error::other(format!(
                                "reload of {name:?} failed: {replacement_error}; rollback failed: {rollback_error} (state=detached)"
                            )))
                        }
                    }
                }
            }
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid management operation")),
    }
}

fn named_record_index(records: &[InterfaceRecord], name: &str) -> io::Result<Option<usize>> {
    let mut matches =
        records.iter().enumerate().filter(|(_, record)| record.name.as_deref() == Some(name));
    let index = matches.next().map(|(index, _)| index);
    if matches.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("multiple current interfaces are named {name:?}"),
        ));
    }
    Ok(index)
}

fn with_named_record(
    current: &[InterfaceRecord],
    index: Option<usize>,
    record: InterfaceRecord,
) -> Vec<InterfaceRecord> {
    let mut next = current.to_vec();
    match index {
        Some(index) => next[index] = record,
        None => next.push(record),
    }
    next
}

async fn stop_named(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    name: &str,
    desired: Vec<InterfaceRecord>,
    transport: Option<&Arc<Transport>>,
    refreshes: &HotApplyRuntimeRefreshes,
    daemon: &Arc<RpcDaemon>,
) -> io::Result<()> {
    #[cfg(test)]
    {
        refreshes.tcp_listeners.lock().expect("tcp listener refresh mutex poisoned").remove(name);
        refreshes.udp.lock().expect("udp refresh mutex poisoned").remove(name);
        refreshes.pipe.lock().expect("pipe refresh mutex poisoned").remove(name);
    }
    let stopped = match managed.remove(&configured_key(name)).and_then(|entry| entry.native) {
        Some(native) => {
            discovery_runtime::stop_owned_native(iface_manager, transport, native).await
        }
        None => Ok(()),
    };
    let applied = apply_hot_apply_interface_records(
        iface_manager,
        managed,
        desired,
        transport,
        refreshes,
        Some(&Arc::downgrade(daemon)),
    )
    .await;
    daemon.replace_interfaces(applied);
    record_publication::publish_control_bindings(managed, daemon);
    stopped
}

async fn wait_ready(active: Option<&ManagedHotApplyInterface>) -> io::Result<()> {
    let status = active
        .and_then(|active| active.native.as_ref()?.status.as_ref())
        .ok_or_else(|| io::Error::other("interface worker was not created"))?;
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    loop {
        let (state, ready, failed, error) = match status {
            HotApplyRuntimeStatus::TcpClient(status) => {
                let value = status.to_json();
                let state = value["stream_state"].as_str().unwrap_or("unknown").to_string();
                (
                    state.clone(),
                    matches!(state.as_str(), "connecting" | "connected" | "reconnecting"),
                    state == "closed",
                    value["last_error"].as_str().map(ToOwned::to_owned),
                )
            }
            HotApplyRuntimeStatus::TcpListener(status) => {
                let value = status.to_json();
                let state = value["listener_state"].as_str().unwrap_or("unknown").to_string();
                (
                    state.clone(),
                    state == "listening",
                    matches!(state.as_str(), "bind_error" | "closed"),
                    value["last_error"].as_str().map(ToOwned::to_owned),
                )
            }
            HotApplyRuntimeStatus::Udp(status) => {
                let value = status.to_json();
                let state = value["link_state"].as_str().unwrap_or("unknown").to_string();
                (
                    state.clone(),
                    state == "bound",
                    matches!(state.as_str(), "bind_failed" | "error" | "closed"),
                    value["last_error"].as_str().map(ToOwned::to_owned),
                )
            }
            HotApplyRuntimeStatus::Pipe(status) => {
                let value = status.to_json();
                let state = value["process_state"].as_str().unwrap_or("unknown").to_string();
                (
                    state.clone(),
                    state == "running",
                    matches!(state.as_str(), "respawning" | "stopped" | "closed"),
                    value["last_error"].as_str().map(ToOwned::to_owned),
                )
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "interface requires its configured runtime owner",
                ))
            }
        };
        if ready {
            return Ok(());
        }
        if failed {
            return Err(io::Error::other(format!(
                "interface startup failed state={state}: {}",
                error.unwrap_or_else(|| "unknown error".to_string())
            )));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("interface startup did not become ready (state={state})"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub(super) async fn stop_hot_apply_interface(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    transport: Option<&Arc<Transport>>,
    address: AddressHash,
) {
    let stopped = if let Some(transport) = transport {
        transport.stop_interface(address).await
    } else {
        iface_manager.lock().await.stop_interface(address)
    };
    if !stopped {
        log::debug!("[daemon] hot-apply interface already absent address={address}");
    }
}
