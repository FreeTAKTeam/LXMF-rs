use rns_rpc::{InterfaceRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::iface::pipe::PipeRuntimeStatusHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;

const HOT_APPLY_PIPE_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct HotApplyPipeRefresh {
    pub(crate) record: InterfaceRecord,
    pub(crate) runtime_iface: AddressHash,
    pub(crate) status: PipeRuntimeStatusHandle,
}

pub(crate) fn attach_hot_apply_pipe_runtime_status(
    daemon: Option<&Weak<RpcDaemon>>,
    record: &InterfaceRecord,
    runtime_iface: AddressHash,
    status: &PipeRuntimeStatusHandle,
) {
    let Some(daemon) = daemon.and_then(Weak::upgrade) else {
        return;
    };
    if !daemon.update_interface_runtime_metadata_by_record(
        record,
        runtime_iface.to_string().as_str(),
        "pipe",
        "status",
        status.to_json(),
    ) {
        log::debug!("[daemon] pipe runtime refresh target disappeared iface={runtime_iface}");
    }
}

pub(crate) fn spawn_hot_apply_pipe_runtime_status_refresher(
    daemon: Weak<RpcDaemon>,
    refreshes: Arc<StdMutex<HashMap<String, HotApplyPipeRefresh>>>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HOT_APPLY_PIPE_STATUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Some(daemon) = daemon.upgrade() else {
                break;
            };
            refresh_hot_apply_pipe_runtime_status_once(&daemon, &refreshes);
        }
    });
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
