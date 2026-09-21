use rns_transport::iface::rnode_ble::{
    RnodeBleBackend, RnodeBleKissConfig, RnodeBleKissError, RnodeBleKissRuntime, RnodeBleWrite,
};

#[derive(Default)]
struct FaultBackend {
    fail_at: Option<&'static str>,
    close_fails: bool,
    live: bool,
    closes: usize,
    drain: bool,
}

impl FaultBackend {
    fn step(&mut self, operation: &'static str) -> Result<(), String> {
        if self.fail_at == Some(operation) {
            self.fail_at = None;
            Err(format!("injected {operation} failure"))
        } else {
            Ok(())
        }
    }
}

impl RnodeBleBackend for FaultBackend {
    async fn connect(&mut self) -> Result<(), String> {
        assert!(!self.live, "reconnect must release the preceding GATT session");
        self.live = true;
        self.step("connect")
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        self.step("subscribe_notifications")
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        self.step("startup_write")
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.step("drain_startup_notifications")?;
        Ok(None)
    }

    fn notification_stream_ends_on_none(&self) -> bool {
        true
    }

    fn drains_stale_startup_notifications(&self) -> bool {
        self.drain
    }

    async fn close(&mut self) -> Result<(), String> {
        self.closes += 1;
        self.live = false;
        if self.close_fails {
            Err("injected close failure".into())
        } else {
            Ok(())
        }
    }
}

fn assert_backend_error(error: RnodeBleKissError, expected: &str) {
    match error {
        RnodeBleKissError::Backend { operation, message } => {
            assert_eq!(operation, expected);
            assert!(!message.is_empty());
        }
        other => panic!("expected backend error, got {other:?}"),
    }
}

#[tokio::test]
async fn rns_1_5_4_failed_startup_releases_every_partial_session() {
    for operation in
        ["connect", "subscribe_notifications", "startup_write", "drain_startup_notifications"]
    {
        let backend = FaultBackend {
            fail_at: Some(operation),
            drain: operation == "drain_startup_notifications",
            ..Default::default()
        };
        let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());
        assert_backend_error(runtime.startup().await.expect_err(operation), operation);
        assert!(!runtime.status().connected, "{operation}");
        assert!(!runtime.status().subscribed, "{operation}");
        assert!(!runtime.backend().live, "{operation} retained a native session");
        assert_eq!(runtime.backend().closes, 1, "{operation}");
        if operation != "drain_startup_notifications" {
            runtime.startup().await.expect("retry after releasing partial session");
            assert!(runtime.status().connected);
        }
    }
}

#[tokio::test]
async fn rns_1_5_4_cleanup_does_not_hide_the_startup_failure() {
    let backend = FaultBackend {
        fail_at: Some("subscribe_notifications"),
        close_fails: true,
        ..Default::default()
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());
    assert_backend_error(
        runtime.startup().await.expect_err("subscription failure"),
        "subscribe_notifications",
    );
    assert_eq!(runtime.backend().closes, 1);
    assert!(!runtime.status().connected);
}

#[tokio::test]
async fn rns_1_5_4_notification_eof_is_a_disconnect_not_empty_traffic() {
    let mut runtime =
        RnodeBleKissRuntime::new(FaultBackend::default(), RnodeBleKissConfig::default());
    runtime.startup().await.expect("startup");
    assert_backend_error(
        runtime.poll_notification_events().await.expect_err("closed stream"),
        "next_notification",
    );
    assert!(!runtime.status().connected);
    assert!(!runtime.status().subscribed);
}

#[tokio::test]
async fn rns_1_5_4_restarting_a_live_runtime_closes_the_previous_session() {
    let mut runtime =
        RnodeBleKissRuntime::new(FaultBackend::default(), RnodeBleKissConfig::default());
    runtime.startup().await.expect("first startup");
    runtime.startup().await.expect("restart");
    assert_eq!(runtime.backend().closes, 1);
    assert!(runtime.status().connected);
    runtime.close().await.expect("close");
    assert!(!runtime.status().subscribed);
    assert!(!runtime.status().connected);
}

#[tokio::test]
async fn rns_1_5_4_startup_drain_eof_releases_the_session() {
    let backend = FaultBackend { drain: true, ..Default::default() };
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());
    assert_backend_error(
        runtime.startup().await.expect_err("closed stream"),
        "drain_startup_notifications",
    );
    assert_eq!(runtime.backend().closes, 1);
    assert!(!runtime.status().connected);
}
