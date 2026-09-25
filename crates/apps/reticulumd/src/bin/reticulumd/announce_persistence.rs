use rns_transport::transport::Transport;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

const RETICULUM_PATH_TABLE_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

pub(super) type PathTableSaveSender = tokio::sync::mpsc::Sender<()>;

#[derive(Clone)]
pub(super) struct PathTablePersistenceContext {
    transport: Arc<Transport>,
    path: PathBuf,
}

impl PathTablePersistenceContext {
    pub(super) fn new(transport: Arc<Transport>, path: PathBuf) -> Self {
        Self { transport, path }
    }
}

pub(super) async fn flush_reticulum_path_table(
    context: &PathTablePersistenceContext,
) -> io::Result<usize> {
    let packet_hashlist_result = context.transport.save_packet_hashlist(&context.path).await;
    let path_table_result = context.transport.save_reticulum_path_table(&context.path).await;

    match (packet_hashlist_result, path_table_result) {
        (Ok(_), path_table_result) => path_table_result,
        (Err(packet_hashlist_error), Ok(_)) => Err(io::Error::new(
            packet_hashlist_error.kind(),
            format!("failed to persist packet hashlist: {packet_hashlist_error}"),
        )),
        (Err(packet_hashlist_error), Err(path_table_error)) => Err(io::Error::other(format!(
            "failed to persist packet hashlist: {packet_hashlist_error}; failed to persist Reticulum path table: {path_table_error}"
        ))),
    }
}

pub(super) async fn flush_reticulum_path_table_if_configured(
    context: Option<PathTablePersistenceContext>,
) {
    if let Some(context) = context {
        if let Err(err) = flush_reticulum_path_table(&context).await {
            log::error!("[daemon] failed to persist Reticulum path table: {err}");
        }
    }
}

pub(super) fn spawn_path_table_persistence_worker(
    context: PathTablePersistenceContext,
) -> PathTableSaveSender {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            sleep(RETICULUM_PATH_TABLE_SAVE_DEBOUNCE).await;
            while rx.try_recv().is_ok() {}
            if let Err(err) = flush_reticulum_path_table(&context).await {
                log::error!("[daemon] failed to persist Reticulum path table: {err}");
            }
        }
    });
    tx
}
