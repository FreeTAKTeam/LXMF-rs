use super::outbound_resources::OutboundResourceMap;
use super::receipt_events::persist_receipt_update;
use reticulum_daemon::receipt_bridge::{is_terminal, ReceiptEvent, ReceiptPublisher};
use rns_rpc::RpcDaemon;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc::Receiver, watch};
pub(super) struct ReceiptOwner {
    publisher: ReceiptPublisher,
    shutdown: watch::Sender<bool>,
    failure: watch::Receiver<Option<String>>,
    worker: tokio::task::JoinHandle<Result<(), String>>,
}
impl ReceiptOwner {
    pub(super) async fn wait_failure(&mut self) -> String {
        loop {
            if let Some(error) = self.failure.borrow().clone() {
                return error;
            }
            if self.failure.changed().await.is_err() {
                return "receipt persistence owner terminated unexpectedly".into();
            }
        }
    }
    pub(super) fn stop_admission(&self) {
        self.publisher.stop_admission();
    }
    pub(super) async fn drain(self) -> Result<(), String> {
        self.publisher.stop_admission();
        self.publisher.release_unobserved();
        self.shutdown.send_replace(true);
        self.worker.await.map_err(|error| error.to_string())?
    }
}
pub(super) fn spawn_receipt_worker(
    daemon: Arc<RpcDaemon>,
    mut receipt_rx: Receiver<ReceiptEvent>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: OutboundResourceMap,
    publisher: ReceiptPublisher,
) -> ReceiptOwner {
    let (shutdown, mut ending) = watch::channel(false);
    let worker_publisher = publisher.clone();
    let (failure_tx, failure) = watch::channel(None);
    let worker = tokio::spawn(async move {
        let result = async { loop {
            let event = if *ending.borrow() {
                receipt_rx.close();
                receipt_rx.recv().await
            } else {
                tokio::select! {biased;_ = ending.changed()=>{receipt_rx.close();receipt_rx.recv().await},event=receipt_rx.recv()=>event}
            };
            let Some(event) = event else {
                return Ok(());
            };
            let mut delay = Duration::from_millis(50);
            let mut shutdown_deadline = None;
            loop {
                let attempt = event.clone();
                let daemon = daemon.clone();
                let receipts = receipt_map.clone();
                let resources = outbound_resource_map.clone();
                // Retain this exact blocking completion before taking another receipt.
                let outcome = tokio::task::spawn_blocking(move || {
                    persist_receipt_update(&daemon, attempt, &receipts, &resources)
                })
                .await
                .map_err(|error| error.to_string())?;
                match outcome {
                    Ok(()) => {
                        if is_terminal(&event.status) {
                            worker_publisher.complete_terminal(&event.message_id);
                        }
                        break;
                    }
                    Err(error) if retryable_storage(&error) => {
                        log::warn!(
                            "terminal receipt persistence deferred message_id={}: {error}",
                            event.message_id
                        );
                        if *ending.borrow() {
                            let deadline = shutdown_deadline.get_or_insert_with(|| {
                                tokio::time::Instant::now() + Duration::from_secs(5)
                            });
                            if tokio::time::Instant::now() >= *deadline {
                                return Err(format!(
                                    "receipt {} remains uncommitted during shutdown: {error}",
                                    event.message_id
                                ));
                            }
                        }
                        tokio::time::sleep(delay).await;
                        delay = (delay * 2).min(Duration::from_millis(500));
                    }
                    Err(error) => {
                        return Err(format!(
                            "receipt {} persistence failed: {error}",
                            event.message_id
                        ))
                    }
                }
            }
        }}.await;
        if let Err(error) = &result {
            worker_publisher.fail_owner();
            failure_tx.send_replace(Some(error.clone()));
        }
        result
    });
    ReceiptOwner { publisher, shutdown, failure, worker }
}
fn retryable_storage(error: &std::io::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> =
        error.get_ref().map(|inner| inner as &(dyn std::error::Error + 'static)).or(Some(error));
    while let Some(error) = source {
        if let Some(inner) =
            error.downcast_ref::<std::io::Error>().and_then(std::io::Error::get_ref)
        {
            source = Some(inner);
            continue;
        }
        if let Some(rusqlite::Error::SqliteFailure(code, _)) =
            error.downcast_ref::<rusqlite::Error>()
        {
            if matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy
                    | rusqlite::ErrorCode::DatabaseLocked
                    | rusqlite::ErrorCode::DiskFull
            ) {
                return true;
            }
        }
        if error.to_string().contains("SDK_STORAGE_BROKER_FULL")
            || error.to_string().contains("SDK_STORAGE_BUSY")
            || error.to_string().contains("SDK_STORAGE_WRITE_BUSY")
        {
            return true;
        }
        source = error.source();
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sqlite_busy_kind_is_preserved_through_io_error_wrapping() {
        let error = std::io::Error::other(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".into()),
        ));
        assert!(retryable_storage(&error));
        assert!(retryable_storage(&std::io::Error::other(error)));
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminal_owner_retries_storage_failure_then_drains_exactly_one_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("receipts.db");
        let store = rns_rpc::MessagesStore::open(&path).unwrap();
        store.enable_durable_broker(32 * 1024 * 1024).unwrap();
        store
            .insert_message(&rns_rpc::MessageRecord {
                id: "terminal-owner".into(),
                source: "local".into(),
                destination: "remote".into(),
                title: "".into(),
                content: "Test1234".into(),
                timestamp: 1,
                direction: "out".into(),
                fields: None,
                receipt_status: None,
            })
            .unwrap();
        let daemon = Arc::new(RpcDaemon::with_store(store, "receipts".into()));
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TRIGGER inject_full BEFORE UPDATE OF receipt_status ON messages BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_FULL injected'); END;").unwrap();
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        let publisher = ReceiptPublisher::from(sender);
        publisher.reserve_terminal("terminal-owner").unwrap();
        let receipts =
            Arc::new(Mutex::new(HashMap::from([("packet".into(), "terminal-owner".into())])));
        let resources = Arc::new(Mutex::new(HashMap::new()));
        let owner = spawn_receipt_worker(
            daemon.clone(),
            receiver,
            receipts.clone(),
            resources,
            publisher.clone(),
        );
        publisher.try_send(ReceiptEvent::new("terminal-owner", "delivered")).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(receipts.lock().unwrap().contains_key("packet"));
        assert_eq!(daemon.message_receipt_status("terminal-owner").unwrap(), None);
        conn.execute_batch("DROP TRIGGER inject_full;").unwrap();
        tokio::time::timeout(Duration::from_secs(3), owner.drain()).await.unwrap().unwrap();
        assert_eq!(
            daemon.message_receipt_status("terminal-owner").unwrap().as_deref(),
            Some("delivered")
        );
        assert!(receipts.lock().unwrap().is_empty());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fatal_storage_failure_stops_admission_and_notifies_daemon_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fatal.db");
        let store = rns_rpc::MessagesStore::open(&path).unwrap();
        store.enable_durable_broker(32 * 1024 * 1024).unwrap();
        store
            .insert_message(&rns_rpc::MessageRecord {
                id: "fatal".into(),
                source: "local".into(),
                destination: "remote".into(),
                title: "".into(),
                content: "Test1234".into(),
                timestamp: 1,
                direction: "out".into(),
                fields: None,
                receipt_status: None,
            })
            .unwrap();
        let daemon = Arc::new(RpcDaemon::with_store(store, "fatal".into()));
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TRIGGER inject_fatal BEFORE UPDATE OF receipt_status ON messages BEGIN SELECT RAISE(ABORT,'invalid durable storage'); END;").unwrap();
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        let publisher = ReceiptPublisher::from(sender);
        publisher.reserve_terminal("fatal").unwrap();
        let mut owner = spawn_receipt_worker(
            daemon,
            receiver,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
            publisher.clone(),
        );
        publisher.try_send(ReceiptEvent::new("fatal", "delivered")).unwrap();
        let error =
            tokio::time::timeout(Duration::from_secs(2), owner.wait_failure()).await.unwrap();
        assert!(error.contains("fatal"));
        assert!(publisher.reserve_terminal("next").is_err());
        assert!(publisher.try_send(ReceiptEvent::new("fatal", "delivered")).is_err());
        assert!(owner.drain().await.is_err());
    }
}
