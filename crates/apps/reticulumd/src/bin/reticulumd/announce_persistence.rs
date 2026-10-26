use crate::persistence_metrics::PersistenceMetrics;
use rns_transport::transport::Transport;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{Duration, Instant};

const RETICULUM_PATH_TABLE_SAVE_MIN_INTERVAL: Duration = Duration::from_secs(300);
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
        Ok(None) => {}
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
    let interval = std::env::var("LXMD_PATH_TABLE_SAVE_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(RETICULUM_PATH_TABLE_SAVE_MIN_INTERVAL);
    spawn_path_table_persistence_worker_with_interval(context, interval)
}

pub(super) fn spawn_path_table_persistence_worker_with_interval(
    context: PathTablePersistenceContext,
    interval: Duration,
) -> PathTableSaveSender {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        let mut maintenance = tokio::time::interval_at(
            Instant::now() + RETICULUM_ANNOUNCE_CACHE_MAINTENANCE_INTERVAL,
            RETICULUM_ANNOUNCE_CACHE_MAINTENANCE_INTERVAL,
        );
        maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut dirty = false;
        let mut next_save = Instant::now() + interval;
        loop {
            tokio::select! {
                _ = maintenance.tick() => maintain_announce_cache(&context).await,
                received = rx.recv() => {
                    if received.is_none() { break; }
                    if dirty { context.metrics.record_coalesced(1); }
                    dirty = true;
                }
                _ = tokio::time::sleep_until(next_save), if dirty => {
                    // Include announces absorbed while the snapshot was pending.
                    while rx.try_recv().is_ok() { context.metrics.record_coalesced(1); }
                    match flush_reticulum_path_table(&context).await {
                        Ok(_) => dirty = false,
                        Err(err) => log::error!("[daemon] failed to persist Reticulum path table: {err}"),
                    }
                    next_save = Instant::now() + interval;
                }
            }
        }
    });
    tx
}
