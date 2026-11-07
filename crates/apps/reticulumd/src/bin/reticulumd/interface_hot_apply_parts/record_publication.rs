use super::*;

pub(super) fn preserve_unmanaged_records(
    mut records: Vec<InterfaceRecord>,
    managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    daemon: Option<&RpcDaemon>,
) -> Vec<InterfaceRecord> {
    let Some(daemon) = daemon else {
        return records;
    };
    let current = daemon.interface_records();
    let protected = current
        .into_iter()
        .enumerate()
        .filter(|(position, record)| {
            !matches!(record.kind.as_str(), "tcp_client" | "tcp_server" | "udp" | "pipe")
                || managed.values().any(|entry| {
                    !entry.origin.configuration_owned()
                        && (entry
                            .configured
                            .as_ref()
                            .is_some_and(|binding| binding.position == *position)
                            || (entry.configured.is_none()
                                && discovery_runtime::same_record(record, &entry.record)))
                })
        })
        .collect::<Vec<_>>();
    // Preserve every existing protected occurrence. Unmatched unsupported records
    // remain legacy display-only configuration and acquire no native ownership.
    records.retain(|record| {
        !protected.iter().any(|(_, protected)| {
            discovery_runtime::same_record(record, protected)
                || protected.name.is_some() && protected.name == record.name
        })
    });
    let mut ordered = records.into_iter().map(|record| (None, record)).collect::<Vec<_>>();
    for (position, record) in protected {
        ordered.insert(position.min(ordered.len()), (Some(position), record));
    }
    for (key, entry) in managed.iter_mut() {
        if let Some(binding) = entry.configured.as_mut() {
            let position = if !entry.origin.configuration_owned() {
                ordered.iter().position(|(previous, _)| *previous == Some(binding.position))
            } else {
                ordered.iter().position(|(_, record)| {
                    hot_apply_interface_key(record).as_deref() == key.configured_name()
                })
            };
            if let Some(position) = position {
                binding.position = position;
            }
        }
    }
    ordered.into_iter().map(|(_, record)| record).collect()
}

/// Keep the submitted order and disabled/startup-only entries while exposing
/// only the policy the native mutation owner actually installed.
pub(super) fn committed_records(
    records: &mut [InterfaceRecord],
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
) {
    for (position, record) in records.iter_mut().enumerate() {
        let key = hot_apply_interface_key(record);
        let active = key
            .as_ref()
            .and_then(|key| managed.get(&configured_key(key)))
            .filter(|active| {
                active.origin.configuration_owned()
                    && active.record.kind == record.kind
                    && active.record.name == record.name
            })
            .or_else(|| {
                managed.values().find(|active| {
                    !active.origin.configuration_owned()
                        && match &active.configured {
                            Some(binding) => binding.position == position,
                            None => discovery_runtime::same_record(&active.record, record),
                        }
                })
            });
        if !record.enabled {
            if key.is_some() || active.is_some() || record.kind == "backbone_client" {
                clear_inactive_runtime(record, "detached", "stopped", None);
            }
            continue;
        }
        let Some(active) = active else {
            if key.is_none() {
                continue;
            }
            clear_inactive_runtime(
                record,
                "failed",
                "failed",
                Some("native interface configuration was not installed"),
            );
            continue;
        };
        project_native_record(record, active);
    }
}

pub(super) fn clear_inactive_runtime(
    record: &mut InterfaceRecord,
    startup: &str,
    runtime: &str,
    error: Option<&str>,
) {
    crate::bootstrap::with_interface_runtime_metadata(record, |metadata| metadata.clear());
    mark_interface_startup_status(record, startup, error, None);
    mark_interface_runtime_fields(record, runtime, 0);
}

pub(super) fn project_native_record(
    record: &mut InterfaceRecord,
    active: &ManagedHotApplyInterface,
) {
    let Some(native) = active.native.as_ref() else {
        *record = active.record.clone();
        return;
    };
    if active.configured.is_some() {
        // Preserve trusted startup outcomes (attached, active, degraded) and metadata.
        *record = active.record.clone();
    } else if native.status.is_none() {
        let previous =
            record.settings.as_ref().and_then(|settings| settings.get("_runtime")).cloned();
        *record = active.record.clone();
        if let Some(previous) = previous.and_then(|previous| previous.as_object().cloned()) {
            crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
                let apply = runtime.get("apply").cloned();
                runtime.extend(previous);
                if let Some(apply) = apply {
                    runtime.insert("apply".into(), apply);
                }
            });
        }
    } else {
        *record = active.record.clone();
        let address = native.address.to_string();
        mark_interface_startup_status(record, "spawned", None, Some(&address));
        mark_interface_runtime_managed(record, "daemon_transport");
        mark_interface_runtime_fields(record, "running", 0);
    }
    if let ManagedInterfaceOrigin::Discovered { endpoint, source, .. } = &active.origin {
        crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
            runtime.insert(
                "discovery".to_owned(),
                serde_json::json!({"endpoint_hash": hex::encode(endpoint), "network_id": source}),
            );
        });
    }
    if let Some(status) = &native.status {
        let (namespace, field, value) = status.projection();
        crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
            let namespace = runtime.entry(namespace).or_insert_with(|| serde_json::json!({}));
            if !namespace.is_object() {
                *namespace = serde_json::json!({});
            }
            namespace[field] = value;
        });
    }
}

pub(super) fn publish_control_bindings(
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    daemon: &RpcDaemon,
) {
    use crate::bridge_rnode_management::{
        DaemonRNodeManagementBinding, DaemonRNodeManagementBridge,
    };
    use crate::bridge_weave_control::{DaemonWeaveControlBinding, DaemonWeaveDisplayControlBridge};
    let mut rnode = Vec::new();
    let mut weave = Vec::new();
    for native in managed.values().filter_map(|entry| entry.native.as_ref()) {
        for (name, handle) in &native.rnode {
            rnode.push(DaemonRNodeManagementBinding {
                runtime_iface: native.address,
                name: name.clone(),
                handle: handle.clone(),
            });
        }
        for (name, handle) in &native.weave {
            weave.push(DaemonWeaveControlBinding {
                runtime_iface: native.address,
                name: name.clone(),
                handle: handle.clone(),
            });
        }
    }
    daemon.set_rnode_management_bridge(Arc::new(DaemonRNodeManagementBridge::new(rnode)));
    daemon.set_weave_display_control_bridge(Arc::new(DaemonWeaveDisplayControlBridge::new(weave)));
}
