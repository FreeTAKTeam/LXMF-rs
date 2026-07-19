use rns_rpc::{InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::udp::UdpRuntimeStatusHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;

const HOT_APPLY_UDP_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct HotApplyUdpRefresh {
    pub(crate) record: InterfaceRecord,
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: UdpRuntimeStatusHandle,
}

pub(crate) fn attach_hot_apply_udp_runtime_status(
    daemon: Option<&Weak<RpcDaemon>>,
    record: &InterfaceRecord,
    runtime_iface: AddressHash,
    status: &UdpRuntimeStatusHandle,
) {
    let Some(daemon) = daemon.and_then(Weak::upgrade) else {
        return;
    };
    if !daemon.update_interface_runtime_metadata_by_record(
        record,
        runtime_iface.to_string().as_str(),
        "udp",
        "status",
        status.to_json(),
    ) {
        log::debug!("[daemon] udp runtime refresh target disappeared iface={runtime_iface}");
    }
}

pub(crate) fn spawn_hot_apply_udp_runtime_status_refresher(
    daemon: Weak<RpcDaemon>,
    refreshes: Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HOT_APPLY_UDP_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Some(daemon) = daemon.upgrade() else {
                break;
            };
            refresh_hot_apply_udp_runtime_status_once(&daemon, &refreshes);
        }
    });
}

pub(crate) fn refresh_hot_apply_udp_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &Arc<StdMutex<HashMap<String, HotApplyUdpRefresh>>>,
) -> usize {
    refreshes
        .lock()
        .expect("udp refresh mutex poisoned")
        .values()
        .filter(|refresh| {
            if daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "udp",
                "status",
                refresh.status.to_json(),
            ) {
                true
            } else {
                daemon.update_interface_runtime_metadata_by_record(
                    &refresh.record,
                    refresh.runtime_iface.to_string().as_str(),
                    "udp",
                    "status",
                    refresh.status.to_json(),
                )
            }
        })
        .count()
}
