struct EofIdleBackend {
    stream_ended: bool,
    reads: VecDeque<FakeBleRead>,
}

enum FakeBleRead {
    Idle,
    Data(Vec<u8>),
    Eof,
}

impl RnodeBleBackend for EofIdleBackend {
    async fn connect(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        match self.reads.pop_front().expect("scripted BLE read") {
            FakeBleRead::Idle => Ok(None),
            FakeBleRead::Data(data) => Ok(Some(data)),
            FakeBleRead::Eof => {
                self.stream_ended = true;
                Ok(None)
            }
        }
    }

    fn notification_stream_ends_on_none(&self) -> bool {
        self.stream_ended
    }
}

#[tokio::test]
async fn native_stream_eof_is_distinct_from_idle_and_idle_read_recovers() {
    let backend = EofIdleBackend {
        stream_ended: false,
        reads: VecDeque::from([
            FakeBleRead::Idle,
            FakeBleRead::Data(encode_data_frame(b"after-idle")),
            FakeBleRead::Eof,
        ]),
    };
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());
    runtime.startup().await.expect("backend startup");

    let idle = runtime
        .poll_notification_events()
        .await
        .expect("idle timeout is not a stream failure");
    assert!(idle.packets.is_empty());
    assert!(runtime.status().connected, "an idle read preserves the live session");

    let resumed = runtime
        .poll_notification_events()
        .await
        .expect("notification after idle");
    assert_eq!(resumed.packets, vec![b"after-idle".to_vec()]);
    assert!(runtime.status().connected);

    let error = runtime
        .poll_notification_events()
        .await
        .expect_err("native stream EOF must be visible to the caller");
    assert!(matches!(
        error,
        RnodeBleKissError::Backend { operation: "next_notification", .. }
    ));
    assert!(!runtime.status().connected, "EOF resets the active session");
}

struct PartialSetupBackend {
    fail_subscription_once: bool,
    events: Vec<&'static str>,
}

impl RnodeBleBackend for PartialSetupBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.events.push("connect");
        Ok(())
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        self.events.push("subscribe");
        if self.fail_subscription_once {
            self.fail_subscription_once = false;
            Err("scripted partial setup failure".to_string())
        } else {
            Ok(())
        }
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        self.events.push("write");
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), String> {
        self.events.push("close");
        Ok(())
    }
}

#[tokio::test]
async fn failed_partial_setup_is_closed_before_runtime_reconnects() {
    let backend = PartialSetupBackend { fail_subscription_once: true, events: Vec::new() };
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());

    let first = runtime.startup().await.expect_err("first subscription fails");
    assert!(matches!(
        first,
        RnodeBleKissError::Backend { operation: "subscribe_notifications", .. }
    ));
    assert!(!runtime.status().connected);
    assert_eq!(runtime.backend().events, ["connect", "subscribe", "close"]);

    runtime.startup().await.expect("a fresh setup succeeds after partial-session cleanup");
    assert!(runtime.status().connected);
    assert_eq!(
        &runtime.backend().events[..5],
        ["connect", "subscribe", "close", "connect", "subscribe"]
    );
    assert!(runtime.backend().events[5..].iter().all(|event| *event == "write"));

    runtime.close().await.expect("close the recovered session");
    assert_eq!(
        runtime.backend().events.last(),
        Some(&"close"),
        "the recovered connection is also released"
    );
}

struct PartialConnectBackend {
    fail_connect_once: bool,
    events: Vec<&'static str>,
}

impl RnodeBleBackend for PartialConnectBackend {
    async fn connect(&mut self) -> Result<(), String> {
        self.events.push("connect");
        if self.fail_connect_once {
            self.fail_connect_once = false;
            Err("scripted failure after partial connection acquisition".to_string())
        } else {
            Ok(())
        }
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        self.events.push("subscribe");
        Ok(())
    }

    async fn write(&mut self, _write: RnodeBleWrite) -> Result<(), String> {
        self.events.push("write");
        Ok(())
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    async fn close(&mut self) -> Result<(), String> {
        self.events.push("close");
        Ok(())
    }
}

#[tokio::test]
async fn failed_partial_connect_is_closed_before_runtime_retries() {
    let backend = PartialConnectBackend { fail_connect_once: true, events: Vec::new() };
    let mut runtime = RnodeBleKissRuntime::new(backend, RnodeBleKissConfig::default());

    let first = runtime.startup().await.expect_err("first connection attempt fails");
    assert!(matches!(
        first,
        RnodeBleKissError::Backend { operation: "connect", .. }
    ));
    assert!(!runtime.status().connected);
    assert_eq!(runtime.backend().events, ["connect", "close"]);

    runtime.startup().await.expect("fresh connection succeeds after partial-session cleanup");
    assert!(runtime.status().connected);
    assert_eq!(
        &runtime.backend().events[..4],
        ["connect", "close", "connect", "subscribe"]
    );

    runtime.close().await.expect("release recovered session");
    assert_eq!(runtime.backend().events.last(), Some(&"close"));
}
