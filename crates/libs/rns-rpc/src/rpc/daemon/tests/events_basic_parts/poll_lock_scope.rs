#[test]
fn sdk_poll_identity_lookup_does_not_block_event_publication() {
    struct BlockingIdentityBridge {
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl ServiceIdentityBridge for BlockingIdentityBridge {
        fn list_service_identities(&self) -> Result<Vec<ServiceIdentityRecord>, std::io::Error> {
            self.entered.send(()).expect("identity lookup entered");
            self.release.lock().expect("release mutex").recv().expect("release lookup");
            Ok(Vec::new())
        }
        fn create_service_identity(
            &self,
            _: ServiceIdentitySpec,
        ) -> Result<ServiceIdentityRecord, std::io::Error> {
            unreachable!("unused")
        }
        fn import_service_identity(
            &self,
            _: &[u8],
            _: ServiceIdentitySpec,
        ) -> Result<ServiceIdentityRecord, std::io::Error> {
            unreachable!("unused")
        }
        fn export_service_identity(&self, _: &str) -> Result<Vec<u8>, std::io::Error> {
            unreachable!("unused")
        }
        fn announce_service_identity(
            &self,
            _: &str,
            _: ServiceIdentitySpec,
        ) -> Result<ServiceIdentityRecord, std::io::Error> {
            unreachable!("unused")
        }
    }
    let daemon = Arc::new(RpcDaemon::test_instance());
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    *daemon.service_identity_bridge.lock().expect("bridge") =
        Some(Arc::new(BlockingIdentityBridge {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }));
    let poll_daemon = daemon.clone();
    let poll = std::thread::spawn(move || {
        poll_daemon.handle_sdk_poll_events_v2(rpc_request(
            1,
            "sdk_poll_events_v2",
            json!({"max": 4}),
        ))
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).expect("poll reached identity lookup");
    let publish_daemon = daemon.clone();
    let (published_tx, published_rx) = std::sync::mpsc::channel();
    let publisher = std::thread::spawn(move || {
        publish_daemon.emit_event(RpcEvent {
            event_type: "inbound".into(),
            payload: json!({"message_id": "during-lookup"}),
        });
        published_tx.send(()).expect("published");
    });
    let progress = published_rx.recv_timeout(Duration::from_secs(1));
    // Always release and join owned workers even when exercising the old failure.
    release_tx.send(()).expect("unblock identity lookup");
    assert!(poll.join().expect("poll worker").expect("poll response").error.is_none());
    publisher.join().expect("publisher worker");
    assert!(progress.is_ok(), "event publication was blocked by an identity lookup");
}
