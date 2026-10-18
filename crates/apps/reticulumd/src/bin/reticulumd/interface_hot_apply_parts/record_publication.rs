use super::*;

pub(super) fn preserve_unmanaged_records(
    mut records: Vec<InterfaceRecord>,
    daemon: Option<&RpcDaemon>,
) -> Vec<InterfaceRecord> {
    if let Some(daemon) = daemon {
        for (index, current) in daemon.interface_records().into_iter().enumerate() {
            if !matches!(current.kind.as_str(), "tcp_client" | "tcp_server" | "udp" | "pipe")
                && !records.iter().any(|record| {
                    record.kind == current.kind
                        && record.name == current.name
                        && record.host == current.host
                        && record.port == current.port
                })
            {
                records.insert(index.min(records.len()), current);
            }
        }
    }
    records
}

/// Keep the submitted order and disabled/startup-only entries while exposing
/// only the policy the native mutation owner actually installed.
pub(super) fn committed_records(
    records: &mut [InterfaceRecord],
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
) {
    for record in records {
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
                        && discovery_runtime::same_record(&active.record, record)
                })
            });
        if !record.enabled {
            if key.is_some() || record.kind == "backbone_client" {
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
        let retained_runtime = if active.runtime_status.is_none() {
            record
                .settings
                .as_ref()
                .and_then(|settings| settings.get("_runtime"))
                .and_then(serde_json::Value::as_object)
                .cloned()
        } else {
            None
        };
        *record = active.record.clone();
        if let Some(retained_runtime) = retained_runtime {
            crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
                *runtime = retained_runtime
            });
            continue;
        }
        let address = active.address.to_string();
        mark_interface_startup_status(record, "spawned", None, Some(&address));
        mark_interface_runtime_managed(record, "daemon_transport");
        mark_interface_runtime_fields(record, "running", 0);
        if let ManagedInterfaceOrigin::Discovered { endpoint, source, .. } = &active.origin {
            crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
                runtime.insert(
                    "discovery".to_owned(),
                    serde_json::json!({
                        "endpoint_hash": hex::encode(endpoint), "network_id": source,
                    }),
                );
            });
        }
        let status = match &active.runtime_status {
            Some(HotApplyRuntimeStatus::TcpClient(status)) => {
                ("tcp", "stream_status", status.to_json())
            }
            Some(HotApplyRuntimeStatus::TcpListener(status)) => {
                ("tcp", "listener_status", status.to_json())
            }
            Some(HotApplyRuntimeStatus::Udp(status)) => ("udp", "status", status.to_json()),
            Some(HotApplyRuntimeStatus::Pipe(status)) => ("pipe", "status", status.to_json()),
            None => continue,
        };
        crate::bootstrap::with_interface_runtime_metadata(record, |runtime| {
            let namespace = runtime.entry(status.0).or_insert_with(|| serde_json::json!({}));
            if !namespace.is_object() {
                *namespace = serde_json::json!({});
            }
            namespace[status.1] = status.2;
        });
    }
}

fn clear_inactive_runtime(
    record: &mut InterfaceRecord,
    startup: &str,
    runtime: &str,
    error: Option<&str>,
) {
    crate::bootstrap::with_interface_runtime_metadata(record, |metadata| metadata.clear());
    mark_interface_startup_status(record, startup, error, None);
    mark_interface_runtime_fields(record, runtime, 0);
}
