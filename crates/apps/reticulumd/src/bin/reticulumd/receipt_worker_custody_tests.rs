use super::*;
use reticulum_daemon::receipt_bridge::{CustodyCommit, CustodyCompletion};
use rns_rpc::broker::{AdmitRequest, PropagationCustody};

struct Fixture {
    _directory: tempfile::TempDir,
    daemon: Arc<RpcDaemon>,
    connection: rusqlite::Connection,
    publisher: ReceiptPublisher,
    receiver: Option<Receiver<ReceiptEvent>>,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("worker directory");
        let path = directory.path().join("custody.db");
        let store = rns_rpc::MessagesStore::open(&path).expect("worker store");
        store.enable_durable_broker(32 * 1024 * 1024).expect("broker");
        let result = store
            .broker_command(rns_rpc::BrokerCommand::Admit {
                owner: "owner".into(),
                source: "source".into(),
                request: AdmitRequest {
                    identity: "source".into(),
                    operation_id: "custody".into(),
                    destination: "recipient".into(),
                    title: String::new(),
                    content: "body".into(),
                    fields: None,
                    options: Default::default(),
                },
            })
            .expect("admit custody");
        let id = result["message_id"].as_str().expect("message id").to_owned();
        let daemon = Arc::new(RpcDaemon::with_store(store, "worker".into()));
        let connection = rusqlite::Connection::open(&path).expect("inspection");
        let (sender, receiver) = tokio::sync::mpsc::channel(2);
        let publisher = ReceiptPublisher::from(sender);
        publisher.reserve_completion(&id, true).expect("paired capacity");
        Self { _directory: directory, daemon, connection, publisher, receiver: Some(receiver), id }
    }
    fn publish(&self) -> CustodyCompletion {
        self.publisher
            .publish_custody(ReceiptEvent::propagation_custody(
                &self.id,
                PropagationCustody::Resource,
            ))
            .expect("publish custody")
    }
    fn start(&mut self) -> ReceiptOwner {
        spawn_receipt_worker(
            self.daemon.clone(),
            self.receiver.take().expect("one worker"),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
            self.publisher.clone(),
        )
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custody_acknowledgement_waits_for_commit_and_preserves_pending_terminal() {
    let mut f = Fixture::new();
    f.connection.execute_batch("CREATE TRIGGER fail_custody BEFORE UPDATE OF receipt_status ON messages WHEN NEW.receipt_status LIKE 'sent:%' BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_FULL custody'); END;
        CREATE TRIGGER fail_terminal BEFORE UPDATE OF receipt_status ON messages WHEN NEW.receipt_status='delivered' BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_FULL terminal'); END;").unwrap();
    let mut completion = f.publish();
    f.publisher.try_send(ReceiptEvent::new(&f.id, "delivered")).unwrap();
    let owner = f.start();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(completion.current(), None);
    assert!(f.publisher.terminal_pending(&f.id).unwrap());
    assert_eq!(f.daemon.message_receipt_status(&f.id).unwrap(), None);
    f.connection.execute_batch("DROP TRIGGER fail_custody;").unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), completion.wait()).await.unwrap().unwrap(),
        CustodyCommit::Stored
    );
    assert!(
        f.publisher.terminal_pending(&f.id).unwrap(),
        "custody cannot erase the terminal fence"
    );
    assert_eq!(
        f.daemon.message_receipt_status(&f.id).unwrap().as_deref(),
        Some("sent: propagated resource")
    );
    assert_eq!(
        f.connection
            .query_row("SELECT COUNT(*) FROM broker_completions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    f.connection.execute_batch("DROP TRIGGER fail_terminal;").unwrap();
    tokio::time::timeout(Duration::from_secs(3), owner.drain()).await.unwrap().unwrap();
    assert!(!f.publisher.terminal_pending(&f.id).unwrap());
    assert_eq!(f.daemon.message_receipt_status(&f.id).unwrap().as_deref(), Some("delivered"));
    assert_eq!(
        f.connection
            .query_row("SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_commit_preserves_pending_custody_acknowledgement_and_duplicates() {
    let mut f = Fixture::new();
    f.publisher.try_send(ReceiptEvent::new(&f.id, "delivered")).unwrap();
    let mut completion = f.publish();
    let mut duplicate = f.publish();
    let owner = f.start();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), completion.wait()).await.unwrap().unwrap(),
        CustodyCommit::SupersededByTerminal
    );
    assert_eq!(duplicate.wait().await.unwrap(), CustodyCommit::SupersededByTerminal);
    owner.drain().await.unwrap();
    assert_eq!(f.daemon.message_receipt_status(&f.id).unwrap().as_deref(), Some("delivered"));
    assert_eq!(
        f.connection
            .query_row("SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fatal_worker_failure_closes_custody_observation_without_acknowledging_commit() {
    let mut f = Fixture::new();
    f.connection.execute_batch("CREATE TRIGGER fail_custody BEFORE UPDATE OF receipt_status ON messages BEGIN SELECT RAISE(ABORT,'invalid storage'); END;").unwrap();
    let mut completion = f.publish();
    let mut owner = f.start();
    assert!(tokio::time::timeout(Duration::from_secs(3), completion.wait())
        .await
        .unwrap()
        .is_err());
    assert_eq!(completion.current(), None);
    assert!(owner.wait_failure().await.contains("invalid storage"));
    assert_eq!(f.daemon.message_receipt_status(&f.id).unwrap(), None);
    assert!(owner.drain().await.is_err());
}
