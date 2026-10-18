use super::*;

pub(super) async fn apply_hot_apply_interface_records(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    mut interfaces: Vec<InterfaceRecord>,
    transport: Option<&Arc<Transport>>,
    refreshes: &HotApplyRuntimeRefreshes,
    daemon: Option<&Weak<RpcDaemon>>,
) -> Vec<InterfaceRecord> {
    for active in managed.values().filter(|active| !active.origin.configuration_owned()) {
        interfaces.retain(|record| {
            let same_name = active.record.name.is_some() && record.name == active.record.name;
            !same_name && !discovery_runtime::same_record(record, &active.record)
        });
        let mut record = active.record.clone();
        if active.runtime_status.is_none() {
            if let Some(current) = daemon.and_then(Weak::upgrade).and_then(|daemon| {
                daemon
                    .interface_records()
                    .into_iter()
                    .find(|current| discovery_runtime::same_record(current, &record))
            }) {
                if let Some(runtime) = current
                    .settings
                    .and_then(|mut settings| settings.as_object_mut()?.remove("_runtime"))
                {
                    crate::bootstrap::with_interface_runtime_metadata(&mut record, |metadata| {
                        if let Some(runtime) = runtime.as_object() {
                            *metadata = runtime.clone();
                        }
                    });
                }
            }
        }
        interfaces.push(record);
    }
    let mut effective = interfaces.clone();
    let desired = interfaces
        .into_iter()
        .filter_map(|record| {
            if managed.values().any(|active| {
                !active.origin.configuration_owned()
                    && discovery_runtime::same_record(&active.record, &record)
            }) {
                return None;
            }
            let key = hot_apply_interface_key(&record)?;
            if managed
                .get(&configured_key(&key))
                .is_some_and(|active| !active.origin.configuration_owned())
            {
                return None;
            }
            Some((key, record))
        })
        .collect::<HashMap<_, _>>();

    let current_keys = managed.keys().cloned().collect::<Vec<_>>();
    for key in current_keys {
        let ManagedInterfaceKey::Configured(ref name) = key else {
            continue;
        };
        let should_remove = match (managed.get(&key), desired.get(name)) {
            (Some(current), _) if !current.origin.configuration_owned() => false,
            (Some(current), Some(next)) => {
                !next.enabled || hot_apply_interface_record_changed(&current.record, next)
            }
            (Some(_), None) => true,
            (None, _) => false,
        };
        if should_remove {
            if let Some(current) = managed.remove(&key) {
                refreshes
                    .tcp_listeners
                    .lock()
                    .expect("tcp listener refresh mutex poisoned")
                    .remove(name);
                refreshes.udp.lock().expect("udp refresh mutex poisoned").remove(name);
                refreshes.pipe.lock().expect("pipe refresh mutex poisoned").remove(name);
                stop_hot_apply_interface(iface_manager, transport, current.address).await;
            }
        }
    }

    for (name, record) in desired {
        let key = configured_key(&name);
        if !record.enabled {
            continue;
        }
        if let Some(current) = managed.get_mut(&key) {
            let mut guard = iface_manager.lock().await;
            match apply_record_runtime_config(&mut guard, current.address, &record) {
                Ok(()) => current.record = record,
                Err(error) => {
                    log::warn!(
                        "[daemon] hot-apply policy rejected for interface {}: {error}",
                        current.address
                    );
                    crate::bootstrap::with_interface_runtime_metadata(
                        &mut current.record,
                        |runtime| {
                            runtime.insert(
                                "apply".to_string(),
                                serde_json::json!({
                                    "state": "rejected", "error": error.to_string(),
                                }),
                            );
                        },
                    );
                }
            }
            continue;
        }
        if let Some((address, runtime_status)) =
            spawn_hot_apply_interface(iface_manager, transport, &record).await
        {
            register_runtime_refresh(
                &name,
                &record,
                address,
                runtime_status.as_ref(),
                refreshes,
                daemon,
            );
            managed.insert(
                key,
                ManagedHotApplyInterface {
                    record,
                    address,
                    runtime_status,
                    origin: ManagedInterfaceOrigin::Configured,
                },
            );
        }
    }
    record_publication::committed_records(&mut effective, managed);
    effective
}

pub(super) fn register_runtime_refresh(
    key: &str,
    record: &InterfaceRecord,
    address: AddressHash,
    runtime_status: Option<&HotApplyRuntimeStatus>,
    refreshes: &HotApplyRuntimeRefreshes,
    daemon: Option<&Weak<RpcDaemon>>,
) {
    match runtime_status.cloned() {
        Some(HotApplyRuntimeStatus::TcpClient(_)) => {}
        Some(HotApplyRuntimeStatus::TcpListener(status)) => {
            attach_hot_apply_tcp_listener_runtime_status(daemon, record, address, &status);
            refreshes.tcp_listeners.lock().expect("tcp listener refresh mutex poisoned").insert(
                key.to_owned(),
                HotApplyTcpListenerRefresh {
                    record: record.clone(),
                    runtime_iface: address,
                    status,
                },
            );
        }
        Some(HotApplyRuntimeStatus::Udp(status)) => {
            status.update(|status| {
                status.iface = Some(address.to_string());
            });
            attach_hot_apply_udp_runtime_status(daemon, record, address, &status);
            refreshes.udp.lock().expect("udp refresh mutex poisoned").insert(
                key.to_owned(),
                HotApplyUdpRefresh { record: record.clone(), runtime_iface: address, status },
            );
        }
        Some(HotApplyRuntimeStatus::Pipe(status)) => {
            attach_hot_apply_pipe_runtime_status(daemon, record, address, &status);
            refreshes.pipe.lock().expect("pipe refresh mutex poisoned").insert(
                key.to_owned(),
                HotApplyPipeRefresh { record: record.clone(), runtime_iface: address, status },
            );
        }
        None => {}
    }
}
