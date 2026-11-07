use rns_rpc::{InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::pipe::PipeRuntimeStatusHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

#[derive(Clone)]
pub(crate) struct HotApplyPipeRefresh {
    pub(crate) record: InterfaceRecord,
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: PipeRuntimeStatusHandle,
}

pub(crate) fn refresh_hot_apply_pipe_runtime_status_once(
    daemon: &RpcDaemon,
    refreshes: &Arc<StdMutex<HashMap<String, HotApplyPipeRefresh>>>,
) -> usize {
    refreshes
        .lock()
        .expect("pipe refresh mutex poisoned")
        .values()
        .filter(|refresh| {
            if daemon.update_interface_runtime_metadata_by_iface(
                refresh.runtime_iface.to_string().as_str(),
                "pipe",
                "status",
                refresh.status.to_json(),
            ) {
                true
            } else {
                daemon.update_interface_runtime_metadata_by_record(
                    &refresh.record,
                    refresh.runtime_iface.to_string().as_str(),
                    "pipe",
                    "status",
                    refresh.status.to_json(),
                )
            }
        })
        .count()
}
