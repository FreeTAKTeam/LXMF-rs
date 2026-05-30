use super::bridge_helpers::diagnostics_enabled;
use rns_transport::transport::Transport;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

const RETICULUM_PATH_TABLE_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

pub(super) type PathTableSaveSender = tokio::sync::mpsc::Sender<()>;

pub(super) fn spawn_path_table_persistence_worker(
    transport: Arc<Transport>,
    path: PathBuf,
) -> PathTableSaveSender {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            sleep(RETICULUM_PATH_TABLE_SAVE_DEBOUNCE).await;
            while rx.try_recv().is_ok() {}
            if let Err(err) = transport.save_reticulum_path_table(&path).await {
                if diagnostics_enabled() {
                    log::error!("[daemon] failed to persist Reticulum path table: {err}");
                }
            }
        }
    });
    tx
}
