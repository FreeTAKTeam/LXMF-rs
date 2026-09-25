#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkerEvent {
    Connected,
    DeferredWrite(Vec<u8>),
    Closed,
}

struct DetectionTimeoutBackend {
    events: Arc<Mutex<Vec<WorkerEvent>>>,
    deferred_frame: Vec<u8>,
    fallback_sent: bool,
    failure_sent: bool,
}

impl RnodeBleBackend for DetectionTimeoutBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.events.lock().expect("events lock").push(WorkerEvent::Connected);
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn write(&mut self, write: RnodeBleWrite) -> Result<(), String> {
        if write.payload == self.deferred_frame {
            self.fallback_sent = true;
            self.events
                .lock()
                .expect("events lock")
                .push(WorkerEvent::DeferredWrite(write.payload));
        }
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        if self.fallback_sent && !self.failure_sent {
            self.failure_sent = true;
            return Err("scripted post-fallback disconnect".to_string());
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), String> {
        self.events.lock().expect("events lock").push(WorkerEvent::Closed);
        Ok(())
    }
}

#[tokio::test]
async fn worker_detection_timeout_sends_fallback_and_closes_backend_on_cancel() {
    let deferred_frame = encode_command_frame(crate::iface::lora::CMD_BANDWIDTH, &[0x01, 0x02]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let config = RnodeBleKissConfig {
        deferred_frames: vec![deferred_frame.clone()],
        ..RnodeBleKissConfig::default()
    };
    let interface = NativeRnodeBleKissInterface::new(
        "test-rnode",
        NativeRnodeBleSettings::for_peripheral("test"),
        config,
    )
    .with_rnode_validation(crate::iface::lora::LoraConfig::us915_default(), Duration::from_secs(1))
    .with_detection_fallback_timeout(Duration::from_millis(25))
    .with_reconnect_backoff(Duration::from_millis(1));
    let mut manager = crate::iface::InterfaceManager::new(1);
    let context = manager.new_context(interface);
    let cancel = context.cancel.clone();
    let started = Instant::now();
    let worker_events = events.clone();
    let worker = tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(
        context,
        move |_| DetectionTimeoutBackend {
            events: worker_events.clone(),
            deferred_frame: deferred_frame.clone(),
            fallback_sent: false,
            failure_sent: false,
        },
    ));

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if events.lock().expect("events lock").contains(&WorkerEvent::DeferredWrite(
                encode_command_frame(crate::iface::lora::CMD_BANDWIDTH, &[0x01, 0x02]),
            )) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("worker reaches its configured fallback deadline");
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(20), "fallback fired too early: {elapsed:?}");
    assert!(elapsed < Duration::from_millis(500), "fallback was not bounded: {elapsed:?}");

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if events
                .lock()
                .expect("events lock")
                .iter()
                .filter(|event| **event == WorkerEvent::Connected)
                .count()
                >= 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("worker reconnects with a fresh backend after cleanup");
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), worker)
        .await
        .expect("worker exits after cancellation")
        .expect("worker task completes");
    let events = events.lock().expect("events lock");
    assert_eq!(
        events.iter().filter(|event| **event == WorkerEvent::Connected).count(),
        2,
        "worker reuses its factory to start a fresh backend"
    );
    assert_eq!(events.last(), Some(&WorkerEvent::Closed));
    assert!(events.contains(&WorkerEvent::Closed));
}
