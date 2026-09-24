use std::sync::atomic::{AtomicBool, Ordering};

struct StartupCancellationBackend {
    entered_connect: Arc<tokio::sync::Notify>,
    closed: Arc<AtomicBool>,
}

impl RnodeBleBackend for StartupCancellationBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.entered_connect.notify_one();
        std::future::pending().await
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), String> {
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn cancelling_during_ble_startup_closes_the_partial_backend() {
    let entered_connect = Arc::new(tokio::sync::Notify::new());
    let closed = Arc::new(AtomicBool::new(false));
    let interface = NativeRnodeBleKissInterface::new(
        "test-rnode-startup-cancel",
        NativeRnodeBleSettings::for_peripheral("test"),
        RnodeBleKissConfig::default(),
    );
    let mut manager = crate::iface::InterfaceManager::new(1);
    let context = manager.new_context(interface);
    let cancel = context.cancel.clone();
    let worker = tokio::spawn(NativeRnodeBleKissInterface::spawn_with_backend_factory(
        context,
        {
            let entered_connect = entered_connect.clone();
            let closed = closed.clone();
            move |_| StartupCancellationBackend {
                entered_connect: entered_connect.clone(),
                closed: closed.clone(),
            }
        },
    ));

    tokio::time::timeout(Duration::from_secs(1), entered_connect.notified())
        .await
        .expect("worker reaches backend connect");
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), worker)
        .await
        .expect("cancelled worker exits promptly")
        .expect("worker task completes");

    assert!(closed.load(Ordering::SeqCst), "partially started backend is closed");
}
