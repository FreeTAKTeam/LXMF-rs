use rns_rpc::{InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::udp::UdpRuntimeStatusHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

#[derive(Clone)]
pub(crate) struct HotApplyUdpRefresh {
    pub(crate) record: InterfaceRecord,
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: UdpRuntimeStatusHandle,
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
