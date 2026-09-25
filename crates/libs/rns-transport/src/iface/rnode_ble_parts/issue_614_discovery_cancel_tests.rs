#[derive(Clone, Debug, PartialEq, Eq)]
enum DiscoveryRetryEvent {
    DiscoveryStarted(usize),
    DiscoveryCancelled,
    Subscribed,
    Wrote,
    Closed,
}

struct DiscoveryCancellationBackend {
    attempt: usize,
    events: Arc<Mutex<Vec<DiscoveryRetryEvent>>>,
}

impl RnodeBleBackend for DiscoveryCancellationBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.events
            .lock()
            .expect("events lock")
            .push(DiscoveryRetryEvent::DiscoveryStarted(self.attempt));
        if self.attempt == 1 {
            self.events
                .lock()
                .expect("events lock")
                .push(DiscoveryRetryEvent::DiscoveryCancelled);
            return Err("scripted service discovery cancellation".to_string());
        }
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        self.events.lock().expect("events lock").push(DiscoveryRetryEvent::Subscribed);
        Ok(())
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        self.events.lock().expect("events lock").push(DiscoveryRetryEvent::Wrote);
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        tokio::time::sleep(Duration::from_millis(2)).await;
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), String> {
        self.events.lock().expect("events lock").push(DiscoveryRetryEvent::Closed);
        Ok(())
    }
}

#[tokio::test]
async fn worker_cleans_up_cancelled_service_discovery_before_bounded_retry() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let interface = NativeRnodeBleKissInterface::new(
        "test-rnode-discovery-cancel",
        NativeRnodeBleSettings::for_peripheral("test"),
        RnodeBleKissConfig::default(),
    )
    .with_reconnect_backoff(Duration::from_millis(1))
    .with_max_reconnect_backoff(Duration::from_millis(4));
    let mut manager = crate::iface::InterfaceManager::new(1);
    let context = manager.new_context(interface);
    let cancel = context.cancel.clone();
    let worker_events = events.clone();
    let started = Instant::now();
    let worker = tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(
        context,
        move |_| {
            let attempt = {
                let events = worker_events.lock().expect("events lock");
                let attempt = events
                    .iter()
                    .filter(|event| matches!(event, DiscoveryRetryEvent::DiscoveryStarted(_)))
                    .count()
                    + 1;
                attempt
            };
            DiscoveryCancellationBackend { attempt, events: worker_events.clone() }
        },
    ));

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let recovered = {
                let events = events.lock().expect("events lock");
                events.contains(&DiscoveryRetryEvent::Subscribed)
                    && events.contains(&DiscoveryRetryEvent::Wrote)
            };
            if recovered {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("worker recovers through a fresh backend after discovery cancellation");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(1), "cleanup and retry exceeded bound: {elapsed:?}");

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), worker)
        .await
        .expect("worker exits after cancellation")
        .expect("worker task completes");

    let events = events.lock().expect("events lock");
    assert_eq!(events.first(), Some(&DiscoveryRetryEvent::DiscoveryStarted(1)));
    assert_eq!(events.get(1), Some(&DiscoveryRetryEvent::DiscoveryCancelled));
    let first_cleanup = events
        .iter()
        .position(|event| *event == DiscoveryRetryEvent::Closed)
        .expect("failed discovery session is closed");
    let retry = events
        .iter()
        .position(|event| *event == DiscoveryRetryEvent::DiscoveryStarted(2))
        .expect("worker creates a fresh backend for retry");
    assert!(first_cleanup < retry, "partial discovery resources are released before retry");
    assert!(events[retry + 1..].contains(&DiscoveryRetryEvent::Subscribed));
    assert!(events[retry + 1..].contains(&DiscoveryRetryEvent::Wrote));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, DiscoveryRetryEvent::DiscoveryStarted(_)))
            .count(),
        2,
        "recovery uses one bounded retry, not an unbounded startup loop"
    );
    assert_eq!(events.last(), Some(&DiscoveryRetryEvent::Closed));
}
