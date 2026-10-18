use super::*;

pub(super) async fn apply_hot_apply_interface_records(
    context: NativeInterfaceContext<'_>,
    managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    mut interfaces: Vec<InterfaceRecord>,
    refreshes: &HotApplyRuntimeRefreshes,
    daemon: Option<&Weak<RpcDaemon>>,
) -> Vec<InterfaceRecord> {
    let NativeInterfaceContext { iface_manager, transport, .. } = context;
    interfaces = record_publication::preserve_unmanaged_records(
        interfaces,
        managed,
        daemon.and_then(Weak::upgrade).as_deref(),
    );
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
                #[cfg(test)]
                {
                    refreshes
                        .tcp_listeners
                        .lock()
                        .expect("tcp listener refresh mutex poisoned")
                        .remove(name);
                    refreshes.udp.lock().expect("udp refresh mutex poisoned").remove(name);
                    refreshes.pipe.lock().expect("pipe refresh mutex poisoned").remove(name);
                }
                if let Some(native) = current.native {
                    if let Err(error) =
                        discovery_runtime::stop_owned_native(iface_manager, transport, native).await
                    {
                        log::warn!(
                            "[daemon] replaced interface cleanup failed name={name}: {error}"
                        );
                    }
                }
            }
        }
    }

    for (name, record) in desired {
        let key = configured_key(&name);
        if !record.enabled {
            continue;
        }
        if let Some(current) = managed.get_mut(&key).filter(|current| current.native.is_some()) {
            let address = current.native.as_ref().expect("active entry has native state").address;
            let mut guard = iface_manager.lock().await;
            match apply_record_runtime_config(&mut guard, address, &record) {
                Ok(()) => {
                    let startup_runtime = current
                        .configured
                        .as_ref()
                        .and_then(|_| current.record.settings.as_ref()?.get("_runtime").cloned());
                    current.record = record;
                    if let Some(runtime) =
                        startup_runtime.and_then(|runtime| runtime.as_object().cloned())
                    {
                        crate::bootstrap::with_interface_runtime_metadata(
                            &mut current.record,
                            |metadata| {
                                *metadata = runtime;
                                metadata.remove("apply");
                            },
                        );
                    }
                }
                Err(error) => {
                    log::warn!(
                        "[daemon] hot-apply policy rejected for interface {}: {error}",
                        address
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
        if let Some((address, runtime_status)) = spawn_hot_apply_interface(context, &record).await {
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
                ManagedHotApplyInterface::active(
                    record,
                    address,
                    runtime_status,
                    ManagedInterfaceOrigin::Configured,
                ),
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
    if let Some(HotApplyRuntimeStatus::Udp(status)) = runtime_status {
        status.update(|status| status.iface = Some(address.to_string()));
    }
    let _ = daemon;
    #[cfg(not(test))]
    let _ = (key, record, refreshes);
    #[cfg(test)]
    match runtime_status.cloned() {
        Some(HotApplyRuntimeStatus::TcpClient(_)) => {}
        Some(HotApplyRuntimeStatus::TcpListener(status)) => {
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
            refreshes.udp.lock().expect("udp refresh mutex poisoned").insert(
                key.to_owned(),
                HotApplyUdpRefresh { record: record.clone(), runtime_iface: address, status },
            );
        }
        Some(HotApplyRuntimeStatus::Pipe(status)) => {
            refreshes.pipe.lock().expect("pipe refresh mutex poisoned").insert(
                key.to_owned(),
                HotApplyPipeRefresh { record: record.clone(), runtime_iface: address, status },
            );
        }
        _ => {}
    }
}
