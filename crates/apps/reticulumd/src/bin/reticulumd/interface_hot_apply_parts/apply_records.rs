use super::*;

pub(super) async fn apply_hot_apply_interface_records(
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<String, ManagedHotApplyInterface>,
    interfaces: Vec<InterfaceRecord>,
    transport: Option<&Arc<Transport>>,
    refreshes: &HotApplyRuntimeRefreshes,
    daemon: Option<&Weak<RpcDaemon>>,
) -> Vec<InterfaceRecord> {
    let mut effective = interfaces.clone();
    let desired = interfaces
        .into_iter()
        .filter_map(|record| {
            let key = hot_apply_interface_key(&record)?;
            Some((key, record))
        })
        .collect::<HashMap<_, _>>();

    let current_keys = managed.keys().cloned().collect::<Vec<_>>();
    for key in current_keys {
        let should_remove = match (managed.get(&key), desired.get(&key)) {
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
                    .remove(&key);
                refreshes.udp.lock().expect("udp refresh mutex poisoned").remove(&key);
                refreshes.pipe.lock().expect("pipe refresh mutex poisoned").remove(&key);
                stop_hot_apply_interface(iface_manager, transport, current.address).await;
            }
        }
    }

    for (key, record) in desired {
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
            match runtime_status.clone() {
                Some(HotApplyRuntimeStatus::TcpClient(_)) => {}
                Some(HotApplyRuntimeStatus::TcpListener(status)) => {
                    attach_hot_apply_tcp_listener_runtime_status(daemon, &record, address, &status);
                    refreshes
                        .tcp_listeners
                        .lock()
                        .expect("tcp listener refresh mutex poisoned")
                        .insert(
                            key.clone(),
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
                    attach_hot_apply_udp_runtime_status(daemon, &record, address, &status);
                    refreshes.udp.lock().expect("udp refresh mutex poisoned").insert(
                        key.clone(),
                        HotApplyUdpRefresh {
                            record: record.clone(),
                            runtime_iface: address,
                            status,
                        },
                    );
                }
                Some(HotApplyRuntimeStatus::Pipe(status)) => {
                    attach_hot_apply_pipe_runtime_status(daemon, &record, address, &status);
                    refreshes.pipe.lock().expect("pipe refresh mutex poisoned").insert(
                        key.clone(),
                        HotApplyPipeRefresh {
                            record: record.clone(),
                            runtime_iface: address,
                            status,
                        },
                    );
                }
                None => {}
            }
            managed.insert(key, ManagedHotApplyInterface { record, address, runtime_status });
        }
    }
    record_publication::committed_records(&mut effective, managed);
    effective
}
