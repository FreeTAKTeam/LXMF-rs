use crate::persistence_metrics::PersistenceMetrics;
use rns_transport::transport::Transport;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

const RETICULUM_PATH_TABLE_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);
const RETICULUM_ANNOUNCE_CACHE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);

pub(super) type PathTableSaveSender = tokio::sync::mpsc::Sender<()>;

#[derive(Clone)]
pub(super) struct PathTablePersistenceContext {
    transport: Arc<Transport>,
    path: PathBuf,
    metrics: Arc<PersistenceMetrics>,
}

impl PathTablePersistenceContext {
    pub(super) fn new(transport: Arc<Transport>, path: PathBuf) -> Self {
        Self { transport, path, metrics: Arc::new(PersistenceMetrics::default()) }
    }

    pub(super) fn metrics(&self) -> Arc<PersistenceMetrics> {
        Arc::clone(&self.metrics)
    }
}

pub(super) async fn flush_reticulum_path_table(
    context: &PathTablePersistenceContext,
) -> io::Result<usize> {
    let _flight = PersistenceFlight::start(&context.metrics);
    let packet_hashlist_result = context.transport.save_packet_hashlist(&context.path).await;
    let path_table_result = context.transport.save_reticulum_path_table_report(&context.path).await;
    match &path_table_result {
        Ok(report) => context.metrics.record_report(report),
        Err(error) => {
            context.metrics.record_error();
            if error.to_string().starts_with("failed to clean announce cache:") {
                context.metrics.record_cleanup_error();
            }
        }
    }
    let path_table_result = path_table_result.map(|report| report.kept_entries);

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

struct PersistenceFlight<'a>(&'a PersistenceMetrics);

impl<'a> PersistenceFlight<'a> {
    fn start(metrics: &'a PersistenceMetrics) -> Self {
        metrics.begin_in_flight();
        Self(metrics)
    }
}

impl Drop for PersistenceFlight<'_> {
    fn drop(&mut self) {
        self.0.end_in_flight();
    }
}

pub(super) async fn maintain_announce_cache(context: &PathTablePersistenceContext) {
    let _flight = PersistenceFlight::start(&context.metrics);
    match context.transport.clean_reticulum_announce_cache_batch(&context.path).await {
        Ok(Some(report)) => context.metrics.record_cleanup(report),
        Ok(None) => {
            if let Err(error) = flush_reticulum_path_table(context).await {
                log::error!("[daemon] failed to persist Reticulum path table: {error}");
            }
        }
        Err(error) => {
            context.metrics.record_error();
            context.metrics.record_cleanup_error();
            log::error!("[daemon] announce cache maintenance failed: {error}");
        }
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
        let mut maintenance = tokio::time::interval(RETICULUM_ANNOUNCE_CACHE_MAINTENANCE_INTERVAL);
        maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = maintenance.tick() => {
                    maintain_announce_cache(&context).await;
                }
                received = rx.recv() => {
                    if received.is_none() {
                        break;
                    }
                    sleep(RETICULUM_PATH_TABLE_SAVE_DEBOUNCE).await;
                    let mut coalesced = 0u64;
                    while rx.try_recv().is_ok() {
                        coalesced += 1;
                    }
                    context.metrics.record_coalesced(coalesced);
                    if let Err(err) = flush_reticulum_path_table(&context).await {
                        log::error!("[daemon] failed to persist Reticulum path table: {err}");
                    }
                }
            }
        }
    });
    tx
}
