async fn startup_ble(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    ble_gatt_runtime_refreshes: &mut Vec<BleGattRuntimeRefresh>,
) -> bool {
    match ble::spawn(iface_manager.clone(), iface).await {
        Ok(spawned) => {
            mark_ble_spawn_success(iface, label, record, spawned.iface);
            ble_gatt_runtime_refreshes.push(BleGattRuntimeRefresh {
                runtime_iface: spawned.iface,
                status: spawned.status,
            });
            true
        }
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            mark_interface_runtime_fields(record, "degraded", 0);
            false
        }
    }
}

async fn startup_reticulum_ble(
    iface: &InterfaceConfig,
    label: &str,
    iface_manager: &Arc<tokio::sync::Mutex<rns_transport::iface::InterfaceManager>>,
    record: &mut InterfaceRecord,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
    reticulum_ble_runtime_refreshes: &mut Vec<ReticulumBleRuntimeRefresh>,
    transport_identity_hash: Option<[u8; 16]>,
) -> bool {
    let Some(local_identity) = transport_identity_hash else {
        let err = "reticulum_ble requires the daemon transport identity hash".to_string();
        record_startup_failure(
            record,
            startup_failures,
            label.to_string(),
            iface.kind.clone(),
            err,
        );
        mark_interface_runtime_fields(record, "degraded", 0);
        return false;
    };
    match reticulum_ble::spawn(iface_manager.clone(), iface, local_identity).await {
        Ok(spawned) => {
            log::info!(
                "[daemon] reticulum_ble enabled iface={} name={} service_uuid={}",
                spawned.iface,
                label,
                iface.service_uuid.as_deref().unwrap_or(reticulum_ble::SERVICE_UUID)
            );
            let runtime_iface = spawned.iface.to_string();
            mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
            mark_interface_runtime_fields(record, "degraded", 0);
            mark_reticulum_ble_runtime_status(record, iface, spawned.iface, local_identity);
            reticulum_ble_runtime_refreshes.push(ReticulumBleRuntimeRefresh {
                runtime_iface: spawned.iface,
                status: spawned.status,
            });
            true
        }
        Err(err) => {
            record_startup_failure(
                record,
                startup_failures,
                label.to_string(),
                iface.kind.clone(),
                err,
            );
            mark_interface_runtime_fields(record, "degraded", 0);
            false
        }
    }
}

fn mark_ble_spawn_success(
    iface: &InterfaceConfig,
    label: &str,
    record: &mut InterfaceRecord,
    ble_iface: AddressHash,
) {
    log::info!(
        "[daemon] ble_gatt enabled iface={} name={} peripheral_id={}",
        ble_iface,
        label,
        iface.peripheral_id.as_deref().unwrap_or("<unset>")
    );
    let runtime_iface = ble_iface.to_string();
    mark_interface_startup_status(record, "spawned", None, Some(runtime_iface.as_str()));
    mark_interface_runtime_fields(record, "running", 0);
    mark_ble_gatt_runtime_status(record, iface, ble_iface);
}
